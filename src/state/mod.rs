mod script;
use script::*;

use std::{
    collections::{HashMap, HashSet},
    mem::{discriminant, swap},
    path::PathBuf,
};

use itertools::Itertools;
use outsource::{JobDescription, Outsourcer};
use rodio::Source;
use structopt::{clap, StructOpt};

use crate::{
    load_script, Channel, CloneCell, CloneLock, Device, DrumMachine, Enveloper, FilterCommand,
    FrameCache, LoopState, Midi, MidiSubcommand, PadBounds, RyvmApp, RyvmCommand, Sample,
    SourceLock, Voice, Wave, WaveCommand, ADSR,
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

#[derive(Debug)]
pub struct State {
    pub sample_rate: u32,
    pub tempo: f32,
    curr_channel: u8,
    sample_queue: Option<f32>,
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
}

impl Default for State {
    fn default() -> Self {
        let app = RyvmApp::from_iter_safe(std::env::args()).unwrap_or_default();
        let mut instruments = State {
            sample_rate: app.sample_rate.unwrap_or(44100),
            tempo: 1.0,
            sample_queue: None,
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
        };

        if let Some((script_args, unresolved_commands)) = load_script("startup.ryvm") {
            let command_args = &["startup".to_string()];
            if let Err(e) =
                instruments.run_script(command_args, "startup", script_args, unresolved_commands)
            {
                println!("{}", e);
            }
        }

        instruments
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
    pub fn insert_loop(&mut self, input: String, name: Option<String>, length: Option<f32>) {
        let tempo = self.tempo;
        let channel = self.channel();
        let name = name.unwrap_or_else(|| {
            let mut i = 1;
            loop {
                let possible = format!("l{}", i);
                if channel.get(&possible).is_none() {
                    break possible;
                }
                i += 1;
            }
        });
        channel.insert_wrapper(input, name, |input| Device::Loop {
            input,
            start_i: CloneCell::new(None),
            frames: CloneLock::new(Vec::new()),
            tempo,
            loop_state: LoopState::Recording,
            length: length.unwrap_or(1.0),
        });
    }
    pub fn stop_recording(&mut self) {
        let mut loop_period = self.loop_period;
        let mut loops_to_delete: Vec<String> = Vec::new();
        for (name, device) in self.channel().names_devices_mut() {
            if let Device::Loop {
                loop_state, frames, ..
            } = device
            {
                if let LoopState::Recording = loop_state {
                    let len = frames.lock().len() as u32;
                    if len > 0 {
                        loop_period.get_or_insert(len);
                        *loop_state = LoopState::Playing;
                        println!("Finished recording {:?}", name);
                    } else {
                        loops_to_delete.push(name.clone());
                        println!("Cancelled recording {:?}", name)
                    }
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
            RyvmCommand::Tempo { tempo } => {
                self.set_tempo(tempo);
                println!("Tempo set to {}x", tempo);
            }
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
            RyvmCommand::Loop {
                input,
                name,
                length,
            } => self.insert_loop(input, name, length),
            RyvmCommand::Filter { input, value } => {
                let channel = self.channel();
                let mut i = 1;
                let filter_name = loop {
                    let possible = format!("filter-{}", i);
                    if channel.get(&possible).is_none() {
                        break possible;
                    }
                    i += 1;
                };
                channel.insert_wrapper(input, filter_name, |input| Device::Filter {
                    input,
                    value,
                    avg: CloneCell::new(Voice::SILENT),
                })
            }
            RyvmCommand::Play { names } => {
                for (name, device) in self.channel().names_devices_mut() {
                    if let Device::Loop { loop_state, .. } = device {
                        if names.contains(name) {
                            *loop_state = LoopState::Playing;
                        }
                    }
                }
            }
            RyvmCommand::Stop { names, all, reset } => {
                if reset {
                    for channel in self.channels.values_mut() {
                        channel.retain(|_, device| !matches!(device, Device::Loop{..}));
                    }
                    self.loop_period = None;
                } else if all {
                    for channel in self.channels.values_mut() {
                        for (name, device) in channel.names_devices_mut() {
                            if let Device::Loop { loop_state, .. } = device {
                                if names.contains(name) {
                                    *loop_state = LoopState::Disabled;
                                }
                            }
                        }
                    }
                } else {
                    for (name, device) in self.channel().names_devices_mut() {
                        if let Device::Loop { loop_state, .. } = device {
                            if names.contains(name) {
                                *loop_state = LoopState::Disabled;
                            }
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
                fn channel_loops(channel: &Channel) -> impl Iterator<Item = &Device> {
                    channel
                        .devices()
                        .filter(|device| matches!(device, Device::Loop {..}))
                }
                if self.channels.values().flat_map(channel_loops).count() == 0 {
                    self.loop_period = None;
                }
            }
            RyvmCommand::Load { name } => self.load_script(&name, true),
            RyvmCommand::Run { name, args } => {
                self.load_script(&name, false);
                if let Err(e) = self.run_script_by_name(&name, &args) {
                    println!("{}", e)
                }
            }
            RyvmCommand::Ch { channel } => self.set_curr_channel(channel),
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
}

impl Iterator for State {
    type Item = f32;
    fn next(&mut self) -> Option<Self::Item> {
        // Process commands
        if let Some(period) = self.loop_period {
            if self.i % period == 0 && self.sample_queue.is_none() {
                let mut commands = Vec::new();
                swap(&mut commands, &mut self.command_queue);
                for (args, app) in commands {
                    self.process_command(args, app);
                }
            }
        }
        // Get next sample
        self.sample_queue
            .take()
            .map(|samp| {
                self.i += 1;
                samp
            })
            .or_else(|| {
                // Init cache
                let mut controls = HashMap::new();
                for (port, midi) in self.midis.iter() {
                    for (channel, control) in midi.controls() {
                        controls
                            .entry((*port, channel))
                            .or_insert_with(Vec::new)
                            .push(control);
                    }
                }
                let mut cache = FrameCache {
                    voices: HashMap::new(),
                    controls,
                    visited: HashSet::new(),
                };
                // Mix output voices
                let mut voice = Voice::SILENT;
                for (&channel_num, channel) in self.channels.iter() {
                    let outputs: Vec<String> = channel.outputs().map(Into::into).collect();
                    let pass_thrus: HashSet<String> = outputs
                        .iter()
                        .filter_map(|name| channel.pass_thru_of(name))
                        .map(Into::into)
                        .collect();
                    for name in outputs.into_iter().chain(pass_thrus) {
                        cache.visited.clear();
                        voice += channel.next_from(channel_num, &name, self, &mut cache) * 0.5;
                    }
                }
                self.sample_queue = Some(voice.right);
                Some(voice.left)
            })
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
