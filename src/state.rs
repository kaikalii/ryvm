use std::{
    collections::{HashMap, HashSet},
    fs::File,
    iter::once,
    mem::{discriminant, swap},
    path::{Path, PathBuf},
    sync::Arc,
};

use crossbeam_channel as mpmc;
use employer::{Employer, JobDescription};
use itertools::Itertools;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rodio::Source;
use ryvm_spec::{Action, DynamicValue, Spec, Supplied};
use structopt::StructOpt;

use crate::{
    parse_commands, Channel, CloneLock, Control, Device, FlyControl, Frame, FrameCache, Loop,
    LoopState, Midi, MidiSubCommand, PadBounds, RyvmCommand, RyvmError, RyvmResult, Sample, Voice,
    ADSR,
};

#[derive(Default)]
pub(crate) struct LoadSamples;

impl JobDescription<PathBuf> for LoadSamples {
    type Output = RyvmResult<Sample>;
    fn work(&self, input: PathBuf) -> Self::Output {
        let res = Sample::open(input);
        if let Err(e) = &res {
            println!("{}", e);
        }
        res
    }
}

/// The main Ryvm state manager
pub struct State {
    pub(crate) sample_rate: u32,
    pub(crate) tempo: f32,
    curr_channel: u8,
    frame_queue: Option<f32>,
    channels: HashMap<u8, Channel>,
    command_queue: Vec<RyvmCommand>,
    /// The index of the current frame
    pub(crate) i: Frame,
    pub(crate) loop_period: Option<Frame>,
    pub(crate) sample_bank: Employer<PathBuf, RyvmResult<Sample>, LoadSamples>,
    pub(crate) midis: HashMap<usize, Midi>,
    midi_names: HashMap<String, usize>,
    pub(crate) default_midi: Option<usize>,
    loops: HashMap<String, Loop>,
    controls: HashMap<(usize, u8, u8), u8>,
    global_controls: HashMap<(usize, u8), u8>,
    tracked_spec_maps: HashMap<PathBuf, u8>,
    watcher: RecommendedWatcher,
    watcher_queue: Arc<CloneLock<Vec<notify::Result<Event>>>>,
    fly_control: Option<FlyControl>,
    send: mpmc::Sender<RyvmResult<bool>>,
    recv: mpmc::Receiver<String>,
}

impl State {
    /// Create a new state
    ///
    /// # Errors
    ///
    /// Returns an error if it fails to load a startup spec
    pub fn new(sample_rate: u32) -> RyvmResult<(Self, StateInterface)> {
        let watcher_queue = Arc::new(CloneLock::new(Vec::new()));
        let watcher_queue_clone = Arc::clone(&watcher_queue);
        let watcher = RecommendedWatcher::new_immediate(move |event: notify::Result<Event>| {
            watcher_queue_clone.lock().push(event);
        })?;
        let (send, inter_recv) = mpmc::unbounded();
        let (inter_send, recv) = mpmc::unbounded();
        let mut state = State {
            sample_rate,
            tempo: 1.0,
            frame_queue: None,
            curr_channel: 0,
            channels: HashMap::new(),
            command_queue: Vec::new(),
            i: 0,
            loop_period: None,
            sample_bank: Employer::default(),
            midis: HashMap::new(),
            midi_names: HashMap::new(),
            default_midi: None,
            loops: HashMap::new(),
            controls: HashMap::new(),
            global_controls: HashMap::new(),
            tracked_spec_maps: HashMap::new(),
            watcher,
            watcher_queue,
            fly_control: None,
            send,
            recv,
        };
        state.load_spec_map_from_file("specs/startup.ron", None)?;
        Ok((
            state,
            StateInterface {
                send: inter_send,
                recv: inter_recv,
            },
        ))
    }
    /// Create a watcher queue to
    /// Get the current channel id
    pub fn curr_channel(&self) -> u8 {
        self.curr_channel
    }
    /// Set the current channel id
    pub fn set_curr_channel(&mut self, ch: u8) {
        self.curr_channel = ch;
        println!("Channel {}", ch);
    }
    /// Get a mutable reference to the current channel
    pub fn channel(&mut self) -> &mut Channel {
        self.channels
            .entry(self.curr_channel)
            .or_insert_with(Channel::default)
    }
    #[allow(dead_code)]
    fn is_debug_frame(&self) -> bool {
        if let Some(period) = self.loop_period {
            self.i % (period / 10) == 0
        } else {
            self.i % (self.sample_rate as Frame / 5) == 0
        }
    }
    /// Start a loop
    pub fn start_loop(&mut self, name: Option<String>, length: Option<f32>) {
        self.finish_recording();
        let name = name.unwrap_or_else(|| {
            let mut i = 1;
            loop {
                let possible = format!("l{}", i);
                if !self.loops.contains_key(&possible) {
                    break possible;
                }
                i += 1;
            }
        });
        self.loops
            .insert(name, Loop::new(self.tempo, length.unwrap_or(1.0)));
        println!("Loop ready");
    }
    /// Finish recording any loops
    pub fn finish_recording(&mut self) {
        let mut loop_period = self.loop_period;
        let mut loops_to_delete: Vec<String> = Vec::new();
        for (name, lup) in &mut self.loops {
            if let LoopState::Recording = lup.loop_state {
                let len = lup.controls.len() as Frame;
                if len > 0 {
                    loop_period.get_or_insert(len);
                    lup.finish();
                    println!("Finished recording {:?}", name);
                } else {
                    loops_to_delete.push(name.clone());
                    println!("Cancelled recording {:?}", name)
                }
            }
        }
        self.loop_period = self.loop_period.or(loop_period);
        for name in loops_to_delete {
            self.loops.remove(&name);
        }
    }
    /// Cancel all loop recording
    pub fn cancel_recording(&mut self) {
        self.loops.retain(|name, lup| {
            if let LoopState::Recording = lup.loop_state {
                println!("Cancelled recording {:?}", name);
                false
            } else {
                true
            }
        });
    }
    fn stop_loop(&mut self, name: &str) {
        if let Some(lup) = self.loops.get_mut(name) {
            for id in lup.note_ids() {
                for device in self.channels.values_mut().flat_map(|ch| ch.devices_mut()) {
                    device.end_envelopes(id);
                }
            }
            lup.loop_state = LoopState::Disabled;
        }
    }
    /// Load a spec into the state
    #[allow(clippy::cognitive_complexity)]
    fn load_spec(&mut self, name: String, spec: Spec, channel: Option<u8>) -> RyvmResult<()> {
        let channel = channel.unwrap_or(self.curr_channel);
        // Macro for initializting devices
        macro_rules! device {
            ($variant:ident, $default:expr) => {{
                let entry = self
                    .channels
                    .entry(channel)
                    .or_insert_with(Channel::default)
                    .entry(name);
                #[allow(clippy::redundant_closure)]
                let device = entry.or_insert_with($default);
                if !matches!(device, Device::$variant(_)) {
                    #[allow(clippy::redundant_closure_call)]
                    {
                        *device = ($default)();
                    }
                }
                if let Device::$variant(device) = device {
                    device
                } else {
                    unreachable!()
                }
            }};
        }
        // Match over different spec types
        match spec {
            Spec::Load(channel, path) => {
                self.load_spec_map_from_file(path, Some(channel))?;
            }
            Spec::Controller {
                device,
                pad_channel,
                pad_range,
                manual,
                non_globals,
                buttons,
            } => {
                let port = if let Supplied(device) = device {
                    if let Some(port) = Midi::port_matching(&device)? {
                        port
                    } else {
                        Midi::first_device()?.ok_or(RyvmError::NoMidiPorts)?
                    }
                } else {
                    Midi::first_device()?.ok_or(RyvmError::NoMidiPorts)?
                };
                let pad =
                    if let (Supplied(channel), Supplied((start, end))) = (pad_channel, pad_range) {
                        Some(PadBounds {
                            channel,
                            start,
                            end,
                        })
                    } else {
                        None
                    };
                let midi = Midi::new(name.clone(), port, manual, pad, buttons, non_globals)?;
                let removed = self.midis.remove(&port).is_some();
                println!(
                    "{}nitialized {} ({}) on port {}",
                    if removed { "Rei" } else { "I" },
                    midi.name(),
                    midi.device(),
                    port
                );
                self.midis.insert(port, midi);
                self.midi_names.insert(name, port);
                if self.default_midi.is_none() {
                    self.default_midi = Some(port);
                }
            }
            Spec::Wave {
                form,
                octave,
                attack,
                decay,
                sustain,
                release,
                bend,
            } => {
                let wave = device!(Wave, || Device::new_wave(form));
                wave.form = form;
                wave.octave = octave.into();
                wave.adsr.attack = attack.or_else(|| ADSR::default().attack.into());
                wave.adsr.decay = decay.or_else(|| ADSR::default().decay.into());
                wave.adsr.sustain = sustain.or_else(|| ADSR::default().sustain.into());
                wave.adsr.release = release.or_else(|| ADSR::default().release.into());
                wave.pitch_bend_range = bend.or(12.0);
            }
            Spec::Drums(paths) => {
                let drums = device!(DrumMachine, || Device::new_drum_machine());
                for path in paths.clone() {
                    self.sample_bank.start(path);
                }
                drums.samples = paths;
            }
            Spec::Filter { input, value } => {
                let filter = device!(Filter, || Device::new_filter(input.clone(), value.clone()));
                filter.input = input;
                filter.value = value;
            }
            Spec::Balance { input, volume, pan } => {
                let balance = device!(Balance, || Device::new_balance(input.clone()));
                balance.input = input;
                balance.volume = volume.or(1.0);
                balance.pan = pan.or(0.0);
            }
        }
        Ok(())
    }
    /// Queue a command
    ///
    /// # Errors
    ///
    /// Returns an error if it failes to parse or process the command
    pub fn queue_command(&mut self, text: &str) -> RyvmResult<bool> {
        if let Some(commands) = parse_commands(&text) {
            for (delay, args) in commands {
                match RyvmCommand::from_iter_safe(&args)? {
                    RyvmCommand::Quit => return Ok(false),
                    command => {
                        if delay {
                            self.command_queue.push(command);
                        } else {
                            self.process_command(command)?;
                        }
                    }
                }
            }
        } else {
            self.finish_recording();
        }
        Ok(true)
    }
    fn process_command(&mut self, command: RyvmCommand) -> RyvmResult<()> {
        match command {
            RyvmCommand::Quit => {}
            RyvmCommand::Midi(MidiSubCommand::List) => {
                for (i, name) in Midi::ports_list()?.into_iter().enumerate() {
                    println!("{}. {}", i, name);
                }
            }
            RyvmCommand::Midi(MidiSubCommand::Monitor) => {
                for midi in self.midis.values() {
                    midi.set_monitoring(!midi.monitoring());
                }
            }
            RyvmCommand::Loop { name, length } => self.start_loop(name, length),
            RyvmCommand::Play { names } => {
                for (name, lup) in &mut self.loops {
                    if names.contains(name) {
                        lup.loop_state = LoopState::Playing;
                    }
                }
            }
            RyvmCommand::Stop { names, all, reset } => {
                if all || reset {
                    let names: Vec<_> = self.loops.keys().cloned().collect();
                    for name in names {
                        self.stop_loop(&name);
                    }
                }
                if reset {
                    self.loops.clear();
                    self.loop_period = None;
                } else if !all {
                    for name in names {
                        self.stop_loop(&name);
                    }
                }
            }
            RyvmCommand::Ls { unsorted } => self.print_ls(unsorted),
            RyvmCommand::Tree => {
                for (ch, channel) in self.channels.iter().sorted_by_key(|(ch, _)| *ch) {
                    println!("~~~~ Channel {} ~~~~", ch);
                    let outputs: Vec<String> = channel.outputs().map(Into::into).collect();
                    for output in outputs {
                        self.print_tree(*ch, &output, 0);
                    }
                }
                self.print_loops();
            }
            RyvmCommand::Rm { id, recursive } => {
                self.channel().remove(&id, recursive);
                self.loops.remove(&id);
            }
            RyvmCommand::Ch { channel } => self.set_curr_channel(channel),
            RyvmCommand::Load { name, channel } => {
                self.load_spec_map_from_file(format!("specs/{}.ron", name), channel)?
            }
        }
        Ok(())
    }
    /// Load a spec map into the state from a file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or parsed or if a spec load fails
    pub fn load_spec_map_from_file<P>(&mut self, path: P, channel: Option<u8>) -> RyvmResult<()>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref().canonicalize()?;
        println!("loading {:?}", path);
        // Load and deserialize the map
        let file = File::open(&path)?;
        let specs = ron::de::from_reader::<_, HashMap<String, Spec>>(file)?;
        // Add it to the watcher
        self.watcher.watch(&path, RecursiveMode::NonRecursive)?;

        if let Some(channel) = channel {
            // Add the path to the list of tracked maps
            self.tracked_spec_maps.insert(path, channel);
            // Remove specs no longer present for this channel
            self.channels
                .entry(channel)
                .or_insert_with(Channel::default)
                .retain(|name, _| specs.contains_key(name));
        }
        // Load each spec
        for (name, spec) in specs {
            self.load_spec(name, spec, channel)?;
        }
        Ok(())
    }
    fn print_ls(&mut self, unsorted: bool) {
        let print = |ids: &mut dyn Iterator<Item = &String>| {
            for id in ids {
                println!("  {}", id)
            }
        };
        for (ch, channel) in self.channels.iter().sorted_by_key(|(ch, _)| *ch) {
            println!("~~~~ Channel {} ~~~~", ch);
            if unsorted {
                print(&mut channel.device_names());
            } else {
                print(
                    &mut channel
                        .names_devices()
                        .sorted_by(|(a_id, a_dev), (b_id, b_dev)| {
                            format!("{:?}", discriminant(*a_dev)).as_bytes()[14..]
                                .cmp(&format!("{:?}", discriminant(*b_dev)).as_bytes()[14..])
                                .then_with(|| a_id.cmp(b_id))
                        })
                        .map(|(id, _)| id),
                );
            }
        }
        self.print_loops();
    }
    fn print_loops(&self) {
        if !self.loops.is_empty() {
            println!("~~~~~~ Loops ~~~~~~");
            for name in self.loops.keys().sorted() {
                println!(
                    "  {} {}",
                    name,
                    match self.loops[name].loop_state {
                        LoopState::Recording => '●',
                        LoopState::Playing => '~',
                        LoopState::Disabled => '-',
                    }
                );
            }
        }
    }
    fn print_tree(&self, ch: u8, root: &str, depth: usize) {
        let channel = if let Some(channel) = self.channels.get(&ch) {
            channel
        } else {
            return;
        };
        let exists = channel.get(root).is_some();
        print!(
            "{}{}{}",
            (0..(2 * depth)).map(|_| ' ').collect::<String>(),
            root,
            if exists { "" } else { "?" }
        );
        println!();
        if let Some(dev) = channel.get(&root) {
            for input in dev.inputs().into_iter().map(Into::<String>::into).sorted() {
                self.print_tree(ch, &input, depth + 1);
            }
        }
    }
    pub(crate) fn resolve_dynamic_value(&self, dyn_val: &DynamicValue, channel: u8) -> Option<f32> {
        match dyn_val {
            DynamicValue::Static(f) => Some(*f),
            DynamicValue::Control {
                controller,
                number,
                bounds,
            } => {
                let port = if let Supplied(controller) = controller {
                    *self.midi_names.get(controller)?
                } else {
                    self.default_midi?
                };
                let midi = self.midis.get(&port)?;
                let value = if midi.control_is_global(*number) {
                    *self.global_controls.get(&(port, *number))?
                } else {
                    *self.controls.get(&(port, channel, *number))?
                };
                let (min, max) = bounds.or((0.0, 1.0));
                Some(f32::from(value) / 127.0 * (max - min) + min)
            }
        }
    }
    fn check_cli_commands(&mut self) {
        while let Ok(command) = self.recv.try_recv() {
            let res = self.queue_command(&command);
            let _ = self.send.send(res);
        }
    }
    fn process_delayed_cli_commands(&mut self) {
        if let Some(period) = self.loop_period {
            if self.i % period == 0 && self.frame_queue.is_none() {
                let mut commands = Vec::new();
                swap(&mut commands, &mut self.command_queue);
                for command in commands {
                    if let Err(e) = self.process_command(command) {
                        println!("{}", e)
                    }
                }
            }
        }
    }
    fn process_watcher(&mut self) -> RyvmResult<()> {
        let events: Vec<_> = self.watcher_queue.lock().drain(..).collect();
        for res in events {
            let event = res?;
            match event.kind {
                EventKind::Modify(_) => {
                    for path in event.paths {
                        let channel = self.tracked_spec_maps.get(&path).copied();
                        let canonical_path = path.canonicalize();
                        if let Err(e) = canonical_path
                            .map_err(Into::into)
                            .and_then(|can_path| self.load_spec_map_from_file(&can_path, channel))
                        {
                            match FlyControl::find(&path) {
                                Ok(Some(fly)) => {
                                    println!("Activate the control you would like to map");
                                    self.fly_control = Some(fly)
                                }
                                Ok(None) | Err(_) => return Err(e),
                            }
                        }
                    }
                }
                EventKind::Remove(_) => {}
                _ => {}
            }
        }
        Ok(())
    }
}

impl Iterator for State {
    type Item = f32;
    fn next(&mut self) -> Option<Self::Item> {
        // Check for CLI commands
        self.check_cli_commands();
        // Check for file watcher events
        if let Err(e) = self.process_watcher() {
            println!("{}", e);
        }
        // Process delayed CLI commands
        self.process_delayed_cli_commands();

        // Get next frame
        // Try the queue
        if let Some(voice) = self.frame_queue.take() {
            self.i += 1;
            return Some(voice);
        }
        // Calculate next frame
        // Get controls from midis
        let raw_controls: Vec<(usize, u8, Control)> = self
            .midis
            .iter_mut()
            .filter_map(|(&port, midi)| {
                midi.controls()
                    .map_err(|e| println!("{}", e))
                    .ok()
                    .map(|controls| (port, controls))
            })
            .flat_map(|(port, controls)| controls.map(move |(ch, con)| (port, ch, con)))
            .collect();

        // Map of port-channel pairs to control lists
        let mut controls = HashMap::new();
        let default_midi = self.default_midi;
        for (port, channel, control) in raw_controls {
            // Process action controls separate from the rest
            if let Control::Action(action) = control {
                match action {
                    Action::Record => self.start_loop(None, None),
                    Action::StopRecording => self.cancel_recording(),
                }
            } else {
                // Check if a fly mapping can be processed
                let midis = &self.midis;
                match self.fly_control.as_mut().map(|fly| {
                    fly.process(control, || {
                        if default_midi.map(|p| p == port).unwrap_or(true) {
                            None
                        } else {
                            Some(midis[&port].name().into())
                        }
                    })
                }) {
                    // Pass the control on
                    Some(Ok(false)) | None => controls
                        .entry((port, channel))
                        .or_insert_with(Vec::new)
                        .push(control),
                    // Reset the fly
                    Some(Ok(true)) => self.fly_control = None,
                    Some(Err(e)) => println!("{}", e),
                }
            }
        }
        // Record loops
        for lup in self.loops.values_mut() {
            if lup.loop_state == LoopState::Recording {
                lup.record(controls.clone(), self.tempo, self.loop_period);
            }
        }
        let mut voice = Voice::SILENT;
        // Collect loop controls
        let state_tempo = self.tempo;
        let loop_period = self.loop_period;
        let loop_controls: Vec<_> = self
            .loops
            .values_mut()
            .filter_map(|lup| lup.controls(state_tempo, loop_period))
            .collect();
        // Iterator through the main controls as well as all playing loop controls
        for (i, controls) in once(controls).chain(loop_controls).enumerate() {
            for (&(port, channel), controls) in &controls {
                for control in controls {
                    if let Control::Control(i, v) = control {
                        self.controls.insert((port, channel, *i), *v);
                        self.global_controls.insert((port, *i), *v);
                    }
                }
            }
            // Init cache
            let mut cache = FrameCache {
                voices: HashMap::new(),
                controls,
                visited: HashSet::new(),
                from_loop: i != 0,
            };
            // Mix output voices for each channel
            for (&channel_num, channel) in &self.channels {
                let outputs: Vec<String> = channel.outputs().map(Into::into).collect();
                for name in outputs {
                    cache.visited.clear();
                    voice += channel.next_from(channel_num, &name, self, &mut cache) * 0.5;
                }
            }
        }
        self.frame_queue = Some(voice.right);
        Some(voice.left)
    }
}

impl Source for State {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn total_duration(&self) -> std::option::Option<std::time::Duration> {
        None
    }
}

/// An interface for sending commands to a running ryvm state
pub struct StateInterface {
    send: mpmc::Sender<String>,
    recv: mpmc::Receiver<RyvmResult<bool>>,
}

impl StateInterface {
    /// Send a command to the state corresponding to this interface
    pub fn send_command<S>(&self, command: S) -> RyvmResult<bool>
    where
        S: Into<String>,
    {
        self.send
            .send(command.into())
            .map_err(|_| RyvmError::StateDropped)?;
        self.recv.recv().unwrap_or(Err(RyvmError::StateDropped))
    }
}
