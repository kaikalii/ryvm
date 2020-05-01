use std::{
    collections::{HashMap, HashSet},
    fs::File,
    iter::once,
    mem::{discriminant, swap},
    path::PathBuf,
};

use itertools::Itertools;
use outsource::{JobDescription, Outsourcer};
use rodio::Source;
use ryvm_spec::{Spec, Supplied};
use structopt::StructOpt;

use crate::{
    Channel, Device, FrameCache, Loop, LoopState, Midi, MidiSubcommand, PadBounds, RyvmApp,
    RyvmCommand, Sample, SourceLock, Voice,
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
    frame_queue: Option<f32>,
    channels: HashMap<u8, Channel>,
    command_queue: Vec<RyvmCommand>,
    pub i: u32,
    pub loop_period: Option<u32>,
    pub sample_bank: Outsourcer<PathBuf, Result<Sample, String>, LoadSamples>,
    pub midis: HashMap<usize, Midi>,
    midi_names: HashMap<String, usize>,
    pub default_midi: Option<usize>,
    loops: HashMap<String, Loop>,
}

impl Default for State {
    fn default() -> Self {
        let app = RyvmApp::from_iter_safe(std::env::args()).unwrap_or_default();
        State {
            sample_rate: app.sample_rate.unwrap_or(44100),
            tempo: 1.0,
            frame_queue: None,
            curr_channel: 0,
            channels: HashMap::new(),
            command_queue: Vec::new(),
            i: 0,
            loop_period: None,
            sample_bank: Outsourcer::default(),
            midis: HashMap::new(),
            midi_names: HashMap::new(),
            default_midi: None,
            loops: HashMap::new(),
        }
    }
}

impl State {
    pub fn new() -> SourceLock<Self> {
        SourceLock::new(Self::default())
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
    fn process_spec(&mut self, channel: u8, name: String, spec: Spec) {
        let entry = self
            .channels
            .entry(channel)
            .or_insert_with(Channel::default)
            .entry(name);
        macro_rules! inner {
            ($variant:ident, $default:expr) => {{
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
        match spec {
            Spec::Wave {
                form,
                octave,
                attack,
                decay,
                sustain,
                release,
                bend,
            } => {
                let wave = inner!(Wave, || Device::default_wave(form));
                wave.form = form;
                if let Supplied(octave) = octave {
                    wave.octave = Some(octave);
                }
                if let Supplied(attack) = attack {
                    wave.adsr.attack = attack;
                }
                if let Supplied(decay) = decay {
                    wave.adsr.decay = decay;
                }
                if let Supplied(sustain) = sustain {
                    wave.adsr.sustain = sustain;
                }
                if let Supplied(release) = release {
                    wave.adsr.release = release;
                }
                if let Supplied(bend) = bend {
                    wave.pitch_bend_range = bend;
                }
            }
            Spec::Drums(paths) => {
                let drums = inner!(DrumMachine, || Device::default_drum_machine());
                drums.samples = paths;
            }
            Spec::Filter { .. } => todo!(),
            Spec::Balance { .. } => todo!(),
        }
    }
    pub fn queue_command(&mut self, delay: bool, command: RyvmCommand) {
        if delay {
            self.command_queue.push(command);
        } else {
            self.process_command(command);
        }
    }
    fn process_command(&mut self, command: RyvmCommand) {
        match command {
            RyvmCommand::Quit => {}
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
            RyvmCommand::Loop { name, length } => self.insert_loop(name, length),
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
                let outputs: Vec<String> = self.channel().outputs().map(Into::into).collect();
                if !outputs.is_empty() {
                    println!("~~~~~ Devices ~~~~~");
                }
                for output in outputs {
                    self.print_tree(&output, 0);
                }
            }
            RyvmCommand::Rm { id, recursive } => {
                self.channel().remove(&id, recursive);
                self.loops.remove(&id);
            }
            RyvmCommand::Ch { channel } => self.set_curr_channel(channel),
            RyvmCommand::Load { name, channel } => {
                match File::open(format!("specs/{}.ron", name)) {
                    Ok(file) => match ron::de::from_reader::<_, HashMap<String, Spec>>(file) {
                        Ok(specs) => {
                            for (name, spec) in specs {
                                self.process_spec(channel.unwrap_or(self.curr_channel), name, spec)
                            }
                        }
                        Err(e) => println!("{}", e),
                    },
                    Err(e) => println!("{}", e),
                }
            }
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
            if self.i % period == 0 && self.frame_queue.is_none() {
                let mut commands = Vec::new();
                swap(&mut commands, &mut self.command_queue);
                for command in commands {
                    self.process_command(command);
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
            // Init cache
            let mut cache = FrameCache {
                voices: HashMap::new(),
                controls,
                visited: HashSet::new(),
                from_loop: i != 0,
            };
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
