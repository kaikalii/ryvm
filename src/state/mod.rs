mod script;
use script::*;

use std::{
    collections::{HashMap, HashSet},
    iter::once,
    mem::{discriminant, swap},
    path::PathBuf,
};

use itertools::Itertools;
use outsource::{JobDescription, Outsourcer};
use rodio::Source;
use structopt::{clap, StructOpt};

use crate::{
    load_script, parse_commands, BalanceCommand, Channel, CloneCell, CloneLock, Control, ControlId,
    Device, DrumMachine, Enveloper, FilterCommand, FrameCache, Loop, LoopState, Midi,
    MidiSubcommand, OrString, PadBounds, RyvmApp, RyvmCommand, Sample, SourceLock, Voice, Wave,
    WaveCommand, ADSR,
};

#[derive(Default)]
pub struct LoadSamples {}

impl JobDescription<PathBuf> for LoadSamples {
    type Output = Result<Sample, String>;
    fn work(&self, input: PathBuf) -> Self::Output {
        let res = Sample::open(input).map_err(|e| e.to_string());
        if let Err(e) = &res {
            println!("{}", e);
        }
        res
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mapping {
    pub port: usize,
    pub channel: u8,
    pub control: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalMapping {
    pub port: usize,
    pub control: u8,
}

#[derive(Debug, Clone)]
pub struct MappingItem {
    pub command: String,
    pub min: Option<f32>,
    pub max: Option<f32>,
}

impl MappingItem {
    pub fn bounds(&self) -> (f32, f32) {
        match (self.min, self.max) {
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) => (a, a + 1.0),
            (None, Some(b)) => (b - 1.0, b),
            (None, None) => (0.0, 1.0),
        }
    }
}

#[derive(Debug)]
pub struct State {
    pub sample_rate: u32,
    pub tempo: f32,
    curr_channel: u8,
    frame_queue: Option<f32>,
    channels: HashMap<u8, Channel>,
    command_queue: Vec<(Vec<String>, clap::Result<RyvmCommand>)>,
    pub i: u32,
    last_drums: Option<String>,
    pub loop_period: Option<u32>,
    pub sample_bank: Outsourcer<PathBuf, Result<Sample, String>, LoadSamples>,
    pub midis: HashMap<usize, Midi>,
    midi_names: HashMap<String, usize>,
    pub default_midi: Option<usize>,
    new_script_stack: Vec<Script>,
    scripts: HashMap<String, Script>,
    mappings: HashMap<Mapping, MappingItem>,
    global_mappings: HashMap<GlobalMapping, MappingItem>,
    loops: HashMap<String, Loop>,
}

impl Default for State {
    fn default() -> Self {
        let app = RyvmApp::from_iter_safe(std::env::args()).unwrap_or_default();
        let mut state = State {
            sample_rate: app.sample_rate.unwrap_or(44100),
            tempo: 1.0,
            frame_queue: None,
            curr_channel: 0,
            channels: HashMap::new(),
            command_queue: Vec::new(),
            i: 0,
            last_drums: None,
            loop_period: None,
            sample_bank: Outsourcer::default(),
            midis: HashMap::new(),
            midi_names: HashMap::new(),
            default_midi: None,
            new_script_stack: Vec::new(),
            scripts: HashMap::new(),
            mappings: HashMap::new(),
            global_mappings: HashMap::new(),
            loops: HashMap::new(),
        };

        if let Some((script_args, unresolved_commands)) = load_script("startup.ryvm") {
            let command_args = &["startup".to_string()];
            if let Err(e) =
                state.run_script(command_args, "startup", script_args, unresolved_commands)
            {
                println!("{}", e);
            }
        }

        state
    }
}

impl State {
    pub fn new() -> SourceLock<Self> {
        SourceLock::new(Self::default())
    }
    pub fn set_tempo(&mut self, tempo: f32) {
        self.tempo = tempo;
    }
    pub fn curr_channel(&self) -> u8 {
        self.curr_channel
    }
    pub fn set_curr_channel(&mut self, ch: u8) {
        self.curr_channel = ch;
        println!("Channel {}", ch);
    }
    pub fn channel(&mut self) -> &mut Channel {
        self.channels
            .entry(self.curr_channel)
            .or_insert_with(Channel::default)
    }
    #[allow(dead_code)]
    pub fn is_debug_frame(&self) -> bool {
        if let Some(period) = self.loop_period {
            self.i % (period / 10) == 0
        } else {
            self.i % (self.sample_rate / 5) == 0
        }
    }
    pub fn find_new_name(&self, base: &str) -> String {
        let mut i = 1;
        let channel = self.channels.get(&self.curr_channel);
        loop {
            let possible = format!("{}{}", base, i);
            if channel
                .as_ref()
                .map(|channel| channel.get(&possible).is_none())
                .unwrap_or(true)
            {
                break possible;
            }
            i += 1;
        }
    }
    pub fn insert_loop(&mut self, name: Option<String>, length: Option<f32>) {
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
    }
    pub fn stop_recording(&mut self) {
        let mut loop_period = self.loop_period;
        let mut loops_to_delete: Vec<String> = Vec::new();
        for (name, lup) in self.loops.iter_mut() {
            if let LoopState::Recording = lup.loop_state {
                let len = lup.controls.lock().len() as u32;
                if len > 0 {
                    loop_period.get_or_insert(len);
                    lup.loop_state = LoopState::Playing;
                    println!("Finished recording {:?}", name);
                } else {
                    loops_to_delete.push(name.clone());
                    println!("Cancelled recording {:?}", name)
                }
            }
        }
        self.loop_period = self.loop_period.or(loop_period);
        for name in loops_to_delete {
            self.channel().remove(&name, false)
        }
    }
    fn process_command(&mut self, args: Vec<String>, app: clap::Result<RyvmCommand>) {
        match app {
            Ok(command) => self.process_ryvm_command(command),
            Err(
                e
                @
                clap::Error {
                    kind: clap::ErrorKind::HelpDisplayed,
                    ..
                },
            ) => println!("{}", e),
            Err(e) => {
                let offender = if let Some([offender, ..]) = e.info.as_deref() {
                    offender
                } else {
                    ""
                };
                match args.as_slice() {
                    [_, name, ..] if name == offender => {
                        if let Err(e) = self.process_instr_command(name.parse().unwrap(), args) {
                            println!("{}", e);
                        }
                    }
                    _ => println!("{}", e),
                }
            }
        }
    }
    pub fn queue_command(
        &mut self,
        delay: bool,
        args: Vec<String>,
        command: clap::Result<RyvmCommand>,
    ) {
        let end_script = matches!(command, Ok(RyvmCommand::End));
        if let (Some(script), false) = (self.new_script_stack.last_mut(), end_script) {
            script.unresolved_commands.push((delay, args));
        } else {
            if let Ok(RyvmCommand::Drum {
                path: Some(path), ..
            }) = &command
            {
                self.sample_bank.restart_if(path.clone(), |r| r.is_err());
            }
            if delay {
                self.command_queue.push((args, command));
            } else {
                self.process_command(args, command);
            }
        }
    }
    #[allow(clippy::cognitive_complexity)]
    fn process_ryvm_command(&mut self, command: RyvmCommand) {
        match command {
            RyvmCommand::Quit => {}
            RyvmCommand::Tempo { tempo } => self.set_tempo(tempo),
            RyvmCommand::Midi(MidiSubcommand::List) => match Midi::ports_list() {
                Ok(list) => {
                    for (i, name) in list.into_iter().enumerate() {
                        println!("{}. {}", i, name);
                    }
                }
                Err(e) => println!("{}", e),
            },
            RyvmCommand::Midi(MidiSubcommand::Init {
                port,
                name,
                manual,
                pad_channel,
                pad_start,
            }) => {
                let port = port.or_else(|| match Midi::first_device() {
                    Ok(p) => p,
                    Err(e) => {
                        println!("{}", e);
                        None
                    }
                });
                if let Some(port) = port {
                    let pad = if let (Some(channel), Some(start)) = (pad_channel, pad_start) {
                        Some(PadBounds { channel, start })
                    } else {
                        None
                    };
                    match Midi::new(port, name.clone(), manual, pad) {
                        Ok(midi) => {
                            if self.midis.remove(&port).is_some() {
                                println!("Reinitialized midi {}", port);
                            } else {
                                println!("Initialized midi {}", port);
                            }
                            self.midis.insert(port, midi);
                            if let Some(name) = name {
                                self.midi_names.insert(name, port);
                            }
                            if self.default_midi.is_none() {
                                self.default_midi = Some(port);
                            }
                        }
                        Err(e) => println!("{}", e),
                    }
                } else {
                    println!("No available port")
                }
            }
            RyvmCommand::Wave {
                waveform,
                name,
                octave,
                attack,
                decay,
                sustain,
                release,
                bend,
            } => {
                let name = name.unwrap_or_else(|| self.find_new_name(&format!("{}", waveform)));
                let default_adsr = ADSR::default();
                let instr = Device::Wave(Box::new(Wave {
                    form: waveform,
                    octave,
                    pitch_bend_range: bend.unwrap_or(12.0),
                    adsr: ADSR {
                        attack: attack.unwrap_or(default_adsr.attack),
                        decay: decay.unwrap_or(default_adsr.decay),
                        sustain: sustain.unwrap_or(default_adsr.sustain),
                        release: release.unwrap_or(default_adsr.release),
                    },
                    enveloper: CloneLock::new(Enveloper::default()),
                    voices: 10,
                    waves: CloneLock::new(Vec::new()),
                }));
                println!("Added wave {:?} to channel {}", name, self.curr_channel);
                self.channel().insert(name, instr);
            }
            RyvmCommand::Drums { name } => {
                let name = name.unwrap_or_else(|| self.find_new_name("drums"));
                self.channel().insert(
                    name.clone(),
                    Device::DrumMachine(Box::new(DrumMachine {
                        samples: Vec::new(),
                        samplings: CloneLock::new(Vec::new()),
                    })),
                );
                println!("Added drums {:?} to channel {}", name, self.curr_channel);
                self.last_drums = Some(name);
            }
            RyvmCommand::Drum {
                machine_id,
                index,
                path,
                remove,
            } => {
                let name = if let Some(name) = machine_id {
                    self.last_drums = Some(name.clone());
                    name
                } else {
                    self.last_drums.clone().unwrap_or_default()
                };
                if let Some(Device::DrumMachine(drums)) = self.channel().get_mut(&name) {
                    let index = index.unwrap_or_else(|| drums.samples_len());
                    if let Some(path) = path {
                        drums.set_path(index, path);
                    }
                    if remove {
                        drums.set_path(index, PathBuf::new());
                    }
                }
            }
            RyvmCommand::Loop { name, length } => self.insert_loop(name, length),
            RyvmCommand::Filter { input, value } => {
                let name = self.find_new_name("filter");
                self.channel()
                    .insert_wrapper(input, name.clone(), |input| Device::Filter {
                        input,
                        value: value.unwrap_or(1.0),
                        avg: CloneCell::new(Voice::SILENT),
                    });
                println!("Added filter {:?} to channel {}", name, self.curr_channel);
            }
            RyvmCommand::Balance { input, volume, pan } => {
                let name = self.find_new_name("bal");
                self.channel()
                    .insert_wrapper(input, name.clone(), |input| Device::Balance {
                        input,
                        volume: volume.unwrap_or(1.0),
                        pan: pan.unwrap_or(0.0),
                    });
                println!("Added balance {:?} to channel {}", name, self.curr_channel);
            }
            RyvmCommand::Play { names } => {
                for (name, lup) in self.loops.iter_mut() {
                    if names.contains(name) {
                        lup.loop_state = LoopState::Playing;
                    }
                }
            }
            RyvmCommand::Stop { names, all, reset } => {
                if reset {
                    self.loops.clear();
                    self.loop_period = None;
                } else if all {
                    for lup in self.loops.values_mut() {
                        lup.loop_state = LoopState::Disabled;
                    }
                } else {
                    for (name, lup) in self.loops.iter_mut() {
                        if names.contains(name) {
                            lup.loop_state = LoopState::Disabled;
                        }
                    }
                }
            }
            RyvmCommand::Ls { unsorted } => self.print_ls(unsorted),
            RyvmCommand::Tree => {
                if !self.scripts.is_empty() {
                    println!("~~~~~ Scripts ~~~~~");
                    for script_name in self.scripts.keys() {
                        println!("{}", script_name)
                    }
                }
                let outputs: Vec<String> = self.channel().outputs().map(Into::into).collect();
                if !outputs.is_empty() {
                    println!("~~~~~ Devices ~~~~~");
                }
                for output in outputs {
                    self.print_tree(&output, 0);
                }
            }
            RyvmCommand::Script { name, args } => self.new_script_stack.push(Script {
                name,
                arguments: args,
                unresolved_commands: Vec::new(),
            }),
            RyvmCommand::End => {
                if let Some(new_script) = self.new_script_stack.pop() {
                    self.scripts.insert(new_script.name.clone(), new_script);
                }
            }
            RyvmCommand::Rm { id, recursive } => {
                self.channel().remove(&id, recursive);
                self.loops.remove(&id);
            }
            RyvmCommand::Load { name } => self.load_script(&name, true),
            RyvmCommand::Run { name, args } => {
                self.load_script(&name, false);
                if let Err(e) = self.run_script_by_name(&name, &args) {
                    println!("{}", e)
                }
            }
            RyvmCommand::Ch { channel } => self.set_curr_channel(channel),
            RyvmCommand::Map {
                control:
                    ControlId {
                        controller,
                        control,
                    },
                command,
                global,
                min,
                max,
            } => {
                let port = match controller {
                    Some(OrString::First(port)) => port,
                    Some(OrString::Second(name)) => {
                        if let Some(port) = self.midi_names.get(&name).copied() {
                            port
                        } else {
                            println!("Unknown midi device {:?}", name);
                            return;
                        }
                    }
                    None => {
                        if let Some(port) = self.default_midi {
                            port
                        } else {
                            println!("No default midi device");
                            return;
                        }
                    }
                };
                let control = match control {
                    OrString::First(con) => con,
                    OrString::Second(_) => todo!(),
                };
                if global {
                    self.global_mappings.insert(
                        GlobalMapping { port, control },
                        MappingItem { command, min, max },
                    );
                } else {
                    self.mappings.insert(
                        Mapping {
                            port,
                            channel: self.curr_channel,
                            control,
                        },
                        MappingItem { command, min, max },
                    );
                }
            }
        }
    }
    fn process_instr_command(&mut self, name: String, args: Vec<String>) -> Result<(), String> {
        let args = &args[1..];
        if let (true, Ok(ch)) = (args.len() == 1, name.parse::<u8>()) {
            self.set_curr_channel(ch);
            Ok(())
        } else if let Some(device) = self.channel().get_mut(&name) {
            match device {
                Device::Wave(wave) => {
                    let com = WaveCommand::from_iter_safe(args).map_err(|e| e.to_string())?;
                    if let Some(o) = com.octave {
                        wave.octave = Some(o);
                    }
                    if let Some(attack) = com.attack {
                        wave.adsr.attack = attack;
                    }
                    if let Some(decay) = com.decay {
                        wave.adsr.decay = decay;
                    }
                    if let Some(sustain) = com.sustain {
                        wave.adsr.sustain = sustain;
                    }
                    if let Some(release) = com.release {
                        wave.adsr.release = release;
                    }
                    if let Some(wf) = com.form {
                        wave.form = wf;
                    }
                    if let Some(bend) = com.bend {
                        wave.pitch_bend_range = bend;
                    }
                }
                Device::Filter { value, .. } => {
                    let com = FilterCommand::from_iter_safe(args).map_err(|e| e.to_string())?;
                    *value = com.value;
                }
                Device::Balance { volume, pan, .. } => {
                    let com = BalanceCommand::from_iter_safe(args).map_err(|e| e.to_string())?;
                    if let Some(vol) = com.volume {
                        *volume = vol;
                    }
                    if let Some(p) = com.pan {
                        *pan = p;
                    }
                }
                _ => {
                    if let Some(script) = self.scripts.get(&name).cloned() {
                        self.run_script(args, &name, script.arguments, script.unresolved_commands)?
                    } else {
                        return Err(format!("No commands available for \"{}\"", name));
                    }
                }
            }
            Ok(())
        } else {
            Err(format!(
                "No device, script, channel, or command \"{}\"",
                name
            ))
        }
    }
    fn print_ls(&mut self, unsorted: bool) {
        let print = |ids: &mut dyn Iterator<Item = &String>| {
            for id in ids {
                println!("    {}", id)
            }
        };
        if unsorted {
            print(&mut self.channel().device_names());
        } else {
            print(
                &mut self
                    .channel()
                    .names_devices()
                    .sorted_by(|(a_id, a_instr), (b_id, b_instr)| {
                        format!("{:?}", discriminant(*a_instr)).as_bytes()[14]
                            .cmp(&format!("{:?}", discriminant(*b_instr)).as_bytes()[14])
                            .then_with(|| a_id.cmp(b_id))
                    })
                    .map(|(id, _)| id),
            );
        }
    }
    fn print_tree(&mut self, root: &str, depth: usize) {
        let exists = self.channel().get(&root).is_some();
        print!(
            "{}{}{}",
            (0..(2 * depth)).map(|_| ' ').collect::<String>(),
            root,
            if exists { "" } else { "?" }
        );
        if let Some(instr) = self.channel().get(&root) {
            println!();
            for input in instr
                .inputs()
                .into_iter()
                .map(Into::<String>::into)
                .sorted()
            {
                self.print_tree(&input, depth + 1);
            }
        } else {
            println!();
        }
    }
    fn run_mapping(&mut self, value: u8, mut item: MappingItem) {
        let (min, max) = item.bounds();
        let f = value as f32 / 127.0 * (max - min) + min;
        item.command.push_str(&format!(" {}", f));
        if let Some(commands) = parse_commands(&item.command) {
            for (delay, args) in commands {
                let app = RyvmCommand::from_iter_safe(&args);
                self.queue_command(delay, args, app);
            }
        }
    }
}

impl Iterator for State {
    type Item = f32;
    fn next(&mut self) -> Option<Self::Item> {
        // Process commands
        if let Some(period) = self.loop_period {
            if self.i % period == 0 && self.frame_queue.is_none() {
                let mut commands = Vec::new();
                swap(&mut commands, &mut self.command_queue);
                for (args, app) in commands {
                    self.process_command(args, app);
                }
            }
        }
        // Get next frame
        // Try the queue
        if let Some(voice) = self.frame_queue.take() {
            self.i += 1;
            return Some(voice);
        }
        // Calculate next frame
        // Map of port-channel pairs to control lists
        let mut controls = HashMap::new();
        // Get controls from midis
        for (&port, midi) in self.midis.iter() {
            for (channel, control) in midi.controls() {
                // Collect control
                controls
                    .entry((port, channel))
                    .or_insert_with(Vec::new)
                    .push(control);
            }
        }
        // Record loops
        for lup in self.loops.values_mut() {
            if lup.loop_state == LoopState::Recording {
                lup.record(controls.clone(), self.i, self.tempo, self.loop_period);
            }
        }
        let mut voice = Voice::SILENT;
        let loop_controls: Vec<_> = self
            .loops
            .values()
            .filter_map(|lup| lup.controls(self.i, self.tempo, self.loop_period))
            .collect();
        // Iterator through the main controls as well as all playing loop controls
        for (i, controls) in once(controls).chain(loop_controls).enumerate() {
            // List of invoked mappings
            let mut mappings = Vec::new();
            // Init cache
            let mut cache = FrameCache {
                voices: HashMap::new(),
                controls,
                visited: HashSet::new(),
                from_loop: i != 0,
            };
            for (&(port, _), controls) in &cache.controls {
                for &control in controls {
                    // Check for Control::Control
                    if let Control::Control(control, value) = control {
                        // Collect mappings
                        let mapping = Mapping {
                            port,
                            channel: self.curr_channel,
                            control,
                        };
                        if let Some(command) = self.mappings.get(&mapping).cloned() {
                            mappings.push((value, command));
                        }
                        // Collect global mappings
                        if let Some(command) = self
                            .global_mappings
                            .get(&GlobalMapping { port, control })
                            .cloned()
                        {
                            mappings.push((value, command));
                        }
                    }
                }
            }
            // Execute mappings
            for (value, command) in mappings {
                self.run_mapping(value, command);
            }
            // Mix output voices for each channel
            for (&channel_num, channel) in self.channels.iter() {
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

impl Source for SourceLock<State> {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.update(|instrs| instrs.sample_rate)
    }
    fn total_duration(&self) -> std::option::Option<std::time::Duration> {
        None
    }
}
