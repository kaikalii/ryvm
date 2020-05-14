use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    iter::once,
    mem::{discriminant, swap},
    path::{Path, PathBuf},
    sync::Arc,
};

use crossbeam_channel as mpmc;
use employer::{Employer, JobDescription};
use indexmap::IndexMap;
use itertools::Itertools;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rodio::Source;
use structopt::StructOpt;

use crate::{
    app,
    channel::{Channel, FrameCache},
    colorprintln, input, library, midi, node, onfly,
    r#loop::{Loop, LoopMaster, LoopState},
    sample,
    spec::{self, Spec},
    ty::{Control, Frame, Name, Port, Voice},
    utility,
};

#[derive(Default)]
pub struct LoadSamples;

impl JobDescription<PathBuf> for LoadSamples {
    type Output = crate::Result<sample::Sample>;
    fn work(&self, input: PathBuf) -> Self::Output {
        let res = sample::Sample::open(input);
        if let Err(e) = &res {
            colorprintln!("{}", bright_red, e)
        }
        res
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StateVars {
    pub sample_rate: u32,
    pub tempo: f32,
    pub master_volume: f32,
    pub i: Frame,
}

/// The main Ryvm state manager
pub struct State {
    pub vars: StateVars,
    frame_queue: Option<f32>,
    channels: HashMap<u8, Channel>,
    command_queue: Vec<app::RyvmCommand>,
    pub loop_master: Option<LoopMaster>,
    pub sample_bank: Employer<PathBuf, crate::Result<sample::Sample>, LoadSamples>,
    pub midis: HashMap<Port, midi::Midi>,
    midi_names: HashMap<Name, Port>,
    pub default_midi: Option<Port>,
    loops: IndexMap<u8, Loop>,
    controls: HashMap<(Port, u8, u8), u8>,
    global_controls: HashMap<(Port, u8), u8>,
    tracked_spec_maps: HashMap<PathBuf, Option<u8>>,
    watcher: RecommendedWatcher,
    watcher_queue: Arc<utility::CloneLock<Vec<notify::Result<Event>>>>,
    fly_control: Option<onfly::FlyControl>,
    send: mpmc::Sender<crate::Result<bool>>,
    recv: mpmc::Receiver<String>,
    input_manager: input::InputManager,
    inputs: HashMap<Name, input::InputDevice>,
    default_input: Option<Name>,
}

impl State {
    /// Create a new state
    ///
    /// # Errors
    ///
    /// Returns an error if it fails to load a startup spec
    pub fn new(
        main_file: Option<PathBuf>,
        sample_rate: u32,
    ) -> crate::Result<(Self, StateInterface)> {
        // Init watcher
        let watcher_queue = Arc::new(utility::CloneLock::new(Vec::new()));
        let watcher_queue_clone = Arc::clone(&watcher_queue);
        let watcher = RecommendedWatcher::new_immediate(move |event: notify::Result<Event>| {
            watcher_queue_clone.lock().push(event);
        })?;
        let (send, inter_recv) = mpmc::unbounded();
        let (inter_send, recv) = mpmc::unbounded();
        // Init state
        let mut state = State {
            vars: StateVars {
                sample_rate,
                tempo: 1.0,
                master_volume: 0.5,
                i: 0,
            },
            frame_queue: None,
            channels: HashMap::new(),
            command_queue: Vec::new(),
            loop_master: None,
            sample_bank: Employer::default(),
            midis: HashMap::new(),
            midi_names: HashMap::new(),
            default_midi: None,
            loops: IndexMap::new(),
            controls: HashMap::new(),
            global_controls: HashMap::new(),
            tracked_spec_maps: HashMap::new(),
            watcher,
            watcher_queue,
            fly_control: None,
            send,
            recv,
            input_manager: input::InputManager::new(),
            inputs: HashMap::new(),
            default_input: None,
        };
        // Load startup
        if let Err(e) = state.load_spec_map(library::startup_path()?, None, true) {
            colorprintln!("{}", bright_red, e);
        }
        // Load main file
        if let Some(main_file) = main_file {
            if let Err(e) = state.load_spec_map(main_file, None, true) {
                colorprintln!("{}", bright_red, e);
            }
        }
        Ok((
            state,
            StateInterface {
                send: inter_send,
                recv: inter_recv,
            },
        ))
    }
    #[allow(dead_code)]
    fn is_debug_frame(&self) -> bool {
        if let Some(master) = self.loop_master {
            self.vars.i % (master.period as Frame / 10) == 0
        } else {
            self.vars.i % (self.vars.sample_rate as Frame / 5) == 0
        }
    }
    /// Start a loop
    pub fn start_loop(&mut self, loop_num: Option<u8>, length: Option<f32>) {
        if loop_num.is_some() {
            self.cancel_recording();
        } else {
            self.finish_recording();
        }
        let loop_num = loop_num.unwrap_or_else(|| {
            let mut i = 0;
            loop {
                if !self.loops.contains_key(&i) {
                    break i;
                }
                i += 1;
            }
        });
        self.loops.remove(&loop_num);
        self.loops
            .insert(loop_num, Loop::new(length.unwrap_or(1.0)));
        colorprintln!("Loop {} ready", cyan, loop_num);
    }
    /// Finish recording any loops
    pub fn finish_recording(&mut self) {
        let mut loop_master = self.loop_master;
        let mut loops_to_delete: Vec<u8> = Vec::new();
        for (&num, lup) in &mut self.loops {
            if let LoopState::Recording = lup.loop_state {
                lup.finish(loop_master.map(|lm| lm.period));
                let period = lup.period();
                if period > 0.0 {
                    loop_master.get_or_insert(LoopMaster { period, num });
                    colorprintln!("Finished recording {}", bright_cyan, num);
                } else {
                    loops_to_delete.push(num);
                    colorprintln!("Cancelled recording {}", bright_yellow, num);
                }
            }
        }
        self.loop_master = self.loop_master.or(loop_master);
        for name in loops_to_delete {
            self.loops.remove(&name);
        }
    }
    /// Cancel all loop recording
    pub fn cancel_recording(&mut self) {
        self.loops.retain(|num, lup| {
            if let LoopState::Recording = lup.loop_state {
                colorprintln!("Cancelled recording {}", bright_yellow, num);
                false
            } else {
                true
            }
        });
    }
    /// Stop a loop from playing
    fn stop_loop(&mut self, num: u8) {
        if let Some(lup) = self.loops.get_mut(&num) {
            for id in lup.note_ids() {
                for node in self.channels.values_mut().flat_map(Channel::nodes_mut) {
                    node.end_envelopes(id);
                }
            }
            lup.loop_state = LoopState::Disabled;
        }
    }
    /// Toggle a loop's playing state
    fn toggle_loop(&mut self, num: u8) {
        if let Some(lup) = self.loops.get_mut(&num) {
            match lup.loop_state {
                LoopState::Recording => {}
                LoopState::Playing => self.stop_loop(num),
                LoopState::Disabled => lup.loop_state = LoopState::Playing,
            }
        }
    }
    /// Load a spec into the state
    #[allow(clippy::cognitive_complexity)]
    fn load_spec(
        &mut self,
        name: Name,
        spec: Spec,
        channel: Option<u8>,
        last_name: Option<Name>,
        do_load_specs: bool,
    ) -> crate::Result<()> {
        // Macro for initializting nodes
        macro_rules! node {
            ($variant:ident, $default:expr) => {{
                let channel = if let Some(channel) = channel {
                    channel
                } else {
                    return Ok(());
                };
                let entry = self
                    .channels
                    .entry(channel)
                    .or_insert_with(Channel::default)
                    .entry(name);
                #[allow(clippy::redundant_closure)]
                let node = entry.or_insert_with(|| $default().into());
                if !matches!(node, node::Node::$variant(_)) {
                    #[allow(clippy::redundant_closure_call)]
                    {
                        *node = ($default)().into();
                    }
                }
                if let node::Node::$variant(node) = node {
                    node
                } else {
                    unreachable!()
                }
            }};
        }
        // Macro for ensuring input is given
        macro_rules! get_input {
            ($input:expr) => {
                $input
                    .or(last_name)
                    .ok_or(crate::Error::NoInputSpecified(name))?
            };
        }
        // Match over different spec types
        match spec {
            Spec::Load { paths } => {
                if do_load_specs {
                    for (path, channel) in paths {
                        self.load_spec_map(path, Some(channel), true)?;
                    }
                }
            }
            Spec::Controller {
                device,
                gamepad,
                output_channel,
                non_globals,
                button,
                slider,
                range,
            } => {
                let ty = if gamepad.is_some() {
                    midi::MidiType::Gamepad
                } else {
                    midi::MidiType::Midi
                };

                let mut buttons: spec::ButtonsMap =
                    button.into_iter().map(|m| (m.action, m.control)).collect();
                let sliders: spec::SlidersMap =
                    slider.into_iter().map(|m| (m.action, m.control)).collect();
                for mapping in range {
                    for (action, button) in mapping.action.zip(mapping.control) {
                        buttons.insert(action, button);
                    }
                }
                let id = match ty {
                    midi::MidiType::Midi => {
                        if let Some(device) = device {
                            if let Some(port) = midi::Midi::port_matching(&device)? {
                                port
                            } else {
                                midi::Midi::first_device()?
                                    .ok_or_else(|| crate::Error::NoMidiPorts(name))?
                            }
                        } else {
                            midi::Midi::first_device()?
                                .ok_or_else(|| crate::Error::NoMidiPorts(name))?
                        }
                    }
                    midi::MidiType::Gamepad => gamepad.unwrap_or(0),
                };
                let port = Port { id, ty };
                let last_notes = self.midis.remove(&port).map(|midi| midi.last_notes);
                let removed = last_notes.is_some();
                let mut midi = midi::Midi::new(
                    port,
                    name,
                    output_channel,
                    non_globals,
                    0.0,
                    buttons,
                    sliders,
                )?;
                midi.last_notes = last_notes.unwrap_or_default();
                colorprintln!(
                    "{}nitialized {}{} on {:?} port {:?}",
                    bright_blue,
                    if removed { "Rei" } else { "I" },
                    midi.name(),
                    if let Some(device) = midi.device() {
                        format!(" ({})", device)
                    } else {
                        String::new()
                    },
                    ty,
                    id
                );

                self.midis.insert(port, midi);
                self.midi_names.insert(name, port);
                self.default_midi.get_or_insert(port);
            }
            Spec::Input { device } => {
                let input = self
                    .input_manager
                    .add_device(device, self.vars.sample_rate)?;
                let removed = self.inputs.remove(&name).is_some();
                colorprintln!(
                    "{}nitialized {} ({})",
                    bright_blue,
                    if removed { "Rei" } else { "I" },
                    name,
                    input.device()
                );
                self.inputs.insert(name, input);
                self.default_input.get_or_insert(name);
            }
            Spec::InputPass { input } => {
                let input = input
                    .or(self.default_input)
                    .ok_or(crate::Error::NoAudioInputForPass)?;
                let pass = node!(InputPass, || node::InputPass::new(input));
                pass.input = input;
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
                let wave = node!(Wave, || node::Wave::new(form));
                wave.form = form;
                wave.octave = octave.into();
                wave.adsr.attack = attack;
                wave.adsr.decay = decay;
                wave.adsr.sustain = sustain;
                wave.adsr.release = release;
                wave.pitch_bend_range = bend;
            }
            Spec::Drums { paths, folder } => {
                let drums = node!(DrumMachine, || node::DrumMachine::new());
                let paths = if let Some(folder) = folder {
                    let folder = library::samples_dir()?.join(folder);
                    let mut paths = Vec::new();
                    for entry in fs::read_dir(&folder)?.filter_map(Result::ok) {
                        let path = entry.path();
                        if path.extension().map_or(false, |ext| ext == "wav") {
                            paths.push(path);
                        }
                    }
                    paths
                } else if let Some(paths) = paths {
                    paths
                } else {
                    Vec::new()
                };
                for path in paths.clone() {
                    self.sample_bank.start(path);
                }
                drums.samples = paths;
            }
            Spec::Filter {
                input,
                value,
                filter: filter_type,
                adsr,
            } => {
                let input = get_input!(input);
                let filter = node!(Filter, || node::Filter::new(input, value, filter_type));
                filter.input = input;
                filter.value = value;
                filter.set_type(filter_type);
                filter.adsr = adsr;
            }
            Spec::Balance { input, volume, pan } => {
                let input = get_input!(input);
                let balance = node!(Balance, || node::Balance::new(input));
                balance.input = input;
                balance.volume = volume;
                balance.pan = pan;
            }
            Spec::Reverb {
                input,
                size,
                energy_mul,
            } => {
                let input = get_input!(input);
                let reverb = node!(Reverb, || node::Reverb::new(input));
                reverb.size = size;
                reverb.energy_mul = energy_mul;
            }
            Spec::Sampler { def, adsr } => {
                self.sample_bank.start(def.path.clone());
                let sampler = node!(Sampler, || node::Sampler::new(def.clone()));
                sampler.def = def;
                sampler.adsr = adsr;
            }
        }
        Ok(())
    }
    /// Queue a command
    ///
    /// # Errors
    ///
    /// Returns an error if it failes to parse or process the command
    pub fn queue_command(&mut self, text: &str) -> crate::Result<bool> {
        if let Some(commands) = utility::parse_commands(&text) {
            for (delay, args) in commands {
                match app::RyvmCommand::from_iter_safe(&args)? {
                    app::RyvmCommand::Quit => return Ok(false),
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
    fn process_command(&mut self, command: app::RyvmCommand) -> crate::Result<()> {
        match command {
            app::RyvmCommand::Quit => {}
            app::RyvmCommand::Midi(app::MidiSubCommand::List) => {
                for (i, name) in midi::Midi::ports_list()?.into_iter().enumerate() {
                    colorprintln!("{}. {}", bright_cyan, i, name);
                }
            }
            app::RyvmCommand::Midi(app::MidiSubCommand::Monitor) => {
                for midi in self.midis.values() {
                    midi.set_monitoring(!midi.monitoring());
                }
            }
            app::RyvmCommand::Loop { num, length, sub } => match sub {
                Some(app::LoopSubcommand::Save { num, name }) => self.save_loop(num, name)?,
                Some(app::LoopSubcommand::Load { name, num, play }) => {
                    self.load_loop(name, num, play)?
                }
                None => self.start_loop(num, length),
            },
            app::RyvmCommand::Play { loops } => {
                for (num, lup) in &mut self.loops {
                    if loops.contains(num) {
                        lup.loop_state = LoopState::Playing;
                    }
                }
            }
            app::RyvmCommand::Stop { loops, all, reset } => {
                if all || reset {
                    let loops: Vec<_> = self.loops.keys().cloned().collect();
                    for num in loops {
                        self.stop_loop(num);
                    }
                }
                if reset {
                    self.loops.clear();
                    self.loop_master = None;
                } else if !all {
                    for num in loops {
                        self.stop_loop(num);
                    }
                }
            }
            app::RyvmCommand::Ls { unsorted } => self.print_ls(unsorted),
            app::RyvmCommand::Tree => {
                for (ch, channel) in self.channels.iter().sorted_by_key(|(ch, _)| *ch) {
                    println!("~~~~ Channel {} ~~~~", ch);
                    let outputs: Vec<String> = channel.outputs().map(Into::into).collect();
                    for output in outputs {
                        self.print_tree(*ch, &output, 0);
                    }
                }
                self.print_loops();
            }
            app::RyvmCommand::Rm {
                id,
                channel,
                recursive,
            } => {
                if let Some(ch) = channel {
                    if let Some(channel) = self.channels.get_mut(&ch) {
                        channel.remove(&id, recursive);
                    }
                } else {
                    for channel in self.channels.values_mut() {
                        channel.remove(&id, recursive);
                    }
                }
                if let Ok(num) = id.parse::<u8>() {
                    self.stop_loop(num);
                    self.loops.remove(&num);
                    if self.loops.is_empty() {
                        self.loop_master = None;
                    }
                }
            }
            app::RyvmCommand::Load { name, channel } => {
                self.load_spec_map_or_on_fly(name.as_str(), channel, false, true)?
            }
            app::RyvmCommand::Specs => {
                open::that(library::specs_dir()?)?;
            }
            app::RyvmCommand::Samples => {
                open::that(library::samples_dir()?)?;
            }
            app::RyvmCommand::Loops => {
                open::that(library::loops_dir()?)?;
            }
            app::RyvmCommand::Inputs => utility::list_input_devices()?,
            app::RyvmCommand::Output(app::OutputSubcommand::List) => {
                utility::list_output_devices()?
            }
        }
        Ok(())
    }
    /// Load a spec map into the state from a file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or parsed or if a spec load fails
    pub fn load_spec_map<P>(
        &mut self,
        name: P,
        channel: Option<u8>,
        do_load_specs: bool,
    ) -> crate::Result<()>
    where
        P: AsRef<Path>,
    {
        let path = library::spec_path(name)?;
        let channel = channel.or_else(|| self.tracked_spec_maps.get(&path).copied().flatten());
        println!("Loading {:?}", path);
        // Load the bytes
        let bytes = fs::read(&path)?;
        // The file at least exists, so the path can be added to the watcher
        self.watcher.watch(&path, RecursiveMode::NonRecursive)?;
        // Deserialize the data
        let specs = toml::from_slice::<IndexMap<Name, Spec>>(&bytes)?;

        // Add the path to the list of tracked maps
        self.tracked_spec_maps.insert(path, channel);
        if let Some(ch) = channel {
            // Remove specs no longer present for this channel
            let channel = self.channels.entry(ch).or_insert_with(Channel::default);
            channel.retain(|name, _| specs.contains_key(name));
        }
        // Load each spec
        let mut last_name = None;
        for (name, spec) in specs {
            self.load_spec(name, spec, channel, last_name, do_load_specs)?;
            last_name = Some(name);
        }
        Ok(())
    }
    fn load_spec_map_or_on_fly<P>(
        &mut self,
        path: P,
        channel: Option<u8>,
        delay: bool,
        do_load_specs: bool,
    ) -> crate::Result<()>
    where
        P: AsRef<Path>,
    {
        if let Err(e) = self.load_spec_map(&path, channel, do_load_specs) {
            match onfly::FlyControl::find(&path, channel, delay) {
                Ok(Some(fly)) => {
                    println!("Activate the control you would like to map");
                    self.fly_control = Some(fly)
                }
                Ok(None) | Err(_) => return Err(e),
            }
        }
        Ok(())
    }
    fn save_loop(&mut self, num: u8, name: Option<Name>) -> crate::Result<()> {
        if let Some(lup) = self.loops.get(&num) {
            let name = if let Some(name) = name {
                name
            } else {
                let mut i = 0;
                loop {
                    let possible = Name::from(&format!("loop-{}", i)).unwrap();
                    if !library::loop_path(possible.as_str())?.exists() {
                        break possible;
                    }
                    i += 1;
                }
            };
            let path = library::loop_path(name.as_str())?;
            let file = File::create(path)?;
            serde_cbor::to_writer(file, lup)?;
            println!("Saved loop {} as {:?}", num, name);
        }
        Ok(())
    }
    fn load_loop(&mut self, name: Name, num: Option<u8>, play: bool) -> crate::Result<()> {
        let path = library::loop_path(name.as_str())?;
        let file = File::open(path)?;
        let mut lup: Loop = serde_cbor::from_reader(file)?;
        let num = num.unwrap_or_else(|| {
            let mut i = 0;
            loop {
                if !self.loops.contains_key(&i) {
                    break i;
                }
                i += 1;
            }
        });
        if let Some(master) = self.loop_master {
            lup.set_period(master.period);
            if let Some(master) = self.loops.get(&master.num) {
                lup.set_t(master.t());
            }
        } else {
            self.loop_master = Some(LoopMaster {
                period: lup.base_period(),
                num,
            });
        }
        if play {
            lup.loop_state = LoopState::Playing;
        }
        self.loops.insert(num, lup);
        println!("Loaded {:?} as loop {}", name, num);
        Ok(())
    }
    fn print_ls(&mut self, unsorted: bool) {
        let print = |ids: &mut dyn Iterator<Item = &Name>| {
            for id in ids {
                println!("  {}", id)
            }
        };
        for (ch, channel) in self.channels.iter().sorted_by_key(|(ch, _)| *ch) {
            println!("~~~~ Channel {} ~~~~", ch);
            if unsorted {
                print(&mut channel.node_names());
            } else {
                print(
                    &mut channel
                        .names_nodes()
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
                    "  Loop {:width$} {}",
                    name,
                    match self.loops[name].loop_state {
                        LoopState::Recording => '●',
                        LoopState::Playing => '~',
                        LoopState::Disabled => '-',
                    },
                    width = 2
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
    pub fn resolve_dynamic_value(
        &self,
        dyn_val: &spec::DynamicValue,
        ch: u8,
        cache: &mut FrameCache,
    ) -> Option<f32> {
        match dyn_val {
            spec::DynamicValue::Static(f) => Some(*f),
            spec::DynamicValue::Control {
                controller,
                index,
                bounds,
                default,
            } => (|| {
                let index = u8::from(*index);
                let port = if let Some(controller) = controller {
                    *self.midi_names.get(controller)?
                } else {
                    self.default_midi?
                };
                let midi = self.midis.get(&port)?;
                let value = if midi.control_is_global(index) {
                    *self.global_controls.get(&(port, index))?
                } else {
                    *self.controls.get(&(port, ch, index))?
                };
                let (min, max) = bounds;
                Some(f32::from(value) / 127.0 * (max - min) + min)
            })()
            .or(*default),
            spec::DynamicValue::Output(name) => self
                .channels
                .get(&ch)
                .map(|channel| channel.next_from(ch, name, self, cache).left),
        }
    }
    fn check_cli_commands(&mut self) {
        while let Ok(command) = self.recv.try_recv() {
            let res = self.queue_command(&command);
            let _ = self.send.send(res);
        }
    }
    fn process_delayed_cli_commands(&mut self) {
        if let Some(master) = self.loop_master {
            if self.vars.i % master.period as Frame == 0 && self.frame_queue.is_none() {
                let mut commands = Vec::new();
                swap(&mut commands, &mut self.command_queue);
                for command in commands {
                    if let Err(e) = self.process_command(command) {
                        colorprintln!("{}", bright_red, e)
                    }
                }
            }
        }
    }
    fn process_watcher(&mut self) -> crate::Result<()> {
        let events: Vec<_> = self.watcher_queue.lock().drain(..).collect();
        for res in events {
            let event = res?;
            match event.kind {
                EventKind::Modify(_) => {
                    for path in event.paths {
                        self.load_spec_map_or_on_fly(path, None, false, false)?;
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
            colorprintln!("{}", bright_red, e);
        }
        // Process delayed CLI commands
        self.process_delayed_cli_commands();

        // Get next frame
        // Try the queue
        if let Some(voice) = self.frame_queue.take() {
            self.vars.i += 1;
            return Some(voice);
        }
        // Calculate next frame
        // Get controls from midis
        let raw_controls: Vec<(Port, u8, Control)> = self
            .midis
            .iter_mut()
            .filter_map(|(&port, midi)| {
                midi.controls()
                    .map_err(|e| colorprintln!("{}", bright_red, e))
                    .ok()
                    .map(|controls| (port, controls))
            })
            .flat_map(|(port, controls)| controls.into_iter().map(move |(ch, con)| (port, ch, con)))
            .collect();

        // Map of port-channel pairs to control lists
        let mut controls: HashMap<(Port, u8), Vec<Control>> = HashMap::new();
        let default_midi = self.default_midi;
        for (port, mut channel, control) in raw_controls {
            // Process action controls separate from the rest
            let control = match control {
                Control::Action(action, vel) => match action {
                    spec::Action::Record => {
                        self.start_loop(None, None);
                        None
                    }
                    spec::Action::StopRecording => {
                        self.cancel_recording();
                        None
                    }
                    spec::Action::DeleteLastLoop => {
                        self.cancel_recording();
                        self.loops.pop();
                        if self.loops.is_empty() {
                            self.loop_master = None;
                        }
                        None
                    }
                    spec::Action::RecordLoop { num } => {
                        self.start_loop(Some(num), None);
                        None
                    }
                    spec::Action::PlayLoop { num } => {
                        if let Some(lup) = self.loops.get_mut(&num) {
                            lup.loop_state = LoopState::Playing;
                        }
                        None
                    }
                    spec::Action::StopLoop { num } => {
                        self.stop_loop(num);
                        None
                    }
                    spec::Action::ToggleLoop { num } => {
                        self.toggle_loop(num);
                        None
                    }
                    spec::Action::Drum { channel: ch, index } => {
                        channel = ch;
                        Some(Control::Pad(index.into(), vel))
                    }
                    spec::Action::SetOutputChannel { name, channel: ch } => {
                        if let Some(midi) = self
                            .midi_names
                            .get(&name)
                            .and_then(|port| self.midis.get(port))
                        {
                            midi.set_output_channel(ch);
                        }
                        None
                    }
                },
                Control::ValuedAction(action, val) => {
                    match action {
                        spec::ValuedAction::Tempo => self.vars.tempo = f32::from(val) / 0x3f as f32,
                        spec::ValuedAction::MasterVolume => {
                            self.vars.master_volume = f32::from(val) / 0x7f as f32
                        }
                        spec::ValuedAction::LoopSpeed { num } => {
                            if let Some(lup) = self.loops.get_mut(&num) {
                                lup.set_speed(
                                    2_f32.powf((f32::from(val) / 0x7f as f32 * 3.0).round()),
                                );
                            }
                        }
                    }
                    None
                }
                control => Some(control),
            };
            if let Some(control) = control {
                // Check if a fly mapping can be processed
                let midis = &self.midis;
                match self.fly_control.as_mut().map(|fly| {
                    fly.process(control, || {
                        if default_midi.map_or(true, |p| p == port) {
                            None
                        } else {
                            Some(utility::name_from_str(midis[&port].name()))
                        }
                    })
                }) {
                    // Pass the control on
                    Some(Ok(false)) | None => controls
                        .entry((port, channel))
                        .or_insert_with(Vec::new)
                        .push(control),
                    // Reset the fly
                    Some(Ok(true)) => {
                        if let Some(fly_control) = self.fly_control.take() {
                            if let Err(e) = self.load_spec_map_or_on_fly(
                                fly_control.file,
                                fly_control.channel,
                                true,
                                false,
                            ) {
                                colorprintln!("{}", bright_red, e);
                            }
                        }
                    }
                    Some(Err(e)) => colorprintln!("{}", bright_red, e),
                }
            }
        }
        // Collect audio input samples
        let audio_input: HashMap<Name, Voice> = self
            .inputs
            .iter_mut()
            .map(|(name, input)| (*name, input.sample().unwrap_or(Voice::SILENT)))
            .collect();
        // Record loops
        let midis = &mut self.midis;
        for lup in self.loops.values_mut() {
            if lup.loop_state == LoopState::Recording {
                lup.record(controls.clone(), |port| {
                    midis.get(&port).map(midi::Midi::advance)
                });
            }
        }
        // Collect loop controls
        let state_tempo = self.vars.tempo;
        let loop_period = self.loop_master.map(|lm| lm.period);
        let loop_controls: Vec<_> = self
            .loops
            .values_mut()
            .filter_map(|lup| lup.controls(state_tempo, loop_period))
            .collect();
        // Initialize voice for this frame
        let mut voice = Voice::SILENT;
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
                audio_input: audio_input.clone(),
                visited: HashSet::new(),
                from_loop: i != 0,
            };
            // Mix output voices for each channel
            for (&channel_num, channel) in &self.channels {
                let outputs: Vec<String> = channel.outputs().map(Into::into).collect();
                for name in outputs {
                    cache.visited.clear();
                    voice += channel.next_from(channel_num, &name, self, &mut cache)
                        * self.vars.master_volume;
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
        self.vars.sample_rate
    }
    fn total_duration(&self) -> std::option::Option<std::time::Duration> {
        None
    }
}

/// An interface for sending commands to a running ryvm state
pub struct StateInterface {
    send: mpmc::Sender<String>,
    recv: mpmc::Receiver<crate::Result<bool>>,
}

impl StateInterface {
    /// Send a command to the state corresponding to this interface
    ///
    /// # Errors
    ///
    /// This function returns an error if there is an error executing the command
    /// or if the state was dropped
    pub fn send_command<S>(&self, command: S) -> crate::Result<bool>
    where
        S: Into<String>,
    {
        self.send
            .send(command.into())
            .map_err(|_| crate::Error::StateDropped)?;
        self.recv.recv().unwrap_or(Err(crate::Error::StateDropped))
    }
}
