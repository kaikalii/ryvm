use std::{
    collections::{HashMap, HashSet},
    iter::repeat,
    mem::{discriminant, swap},
    path::PathBuf,
    sync::Arc,
};

use itertools::Itertools;
use outsource::{JobDescription, Outsourcer};
use rodio::Source;
use serde_derive::{Deserialize, Serialize};
use structopt::{clap, StructOpt};

#[cfg(feature = "keyboard")]
use crate::Keyboard;
use crate::{
    default_tempo, is_default_tempo, mix, Balance, ChannelId, Channels, CloneLock, FilterCommand,
    Frame, FrameCache, InstrId, Instrument, LoopFrame, MixerCommand, NumberCommand, RyvmCommand,
    Sample, SampleType, Sampling, SourceLock, Voice, WaveCommand, ADSR, SAMPLE_RATE,
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
pub struct NewScript {
    pub name: InstrId,
    pub args: Vec<String>,
    pub commands: Vec<(bool, Vec<String>)>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Instruments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output: Option<InstrId>,
    #[serde(
        rename = "instruments",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    map: HashMap<InstrId, Instrument>,
    #[serde(default = "default_tempo", skip_serializing_if = "is_default_tempo")]
    tempo: SampleType,
    #[serde(skip)]
    sample_queue: Option<SampleType>,
    #[serde(skip)]
    command_queue: Vec<(Vec<String>, clap::Result<RyvmCommand>)>,
    #[serde(skip)]
    i: u32,
    #[serde(skip)]
    last_drums: Option<InstrId>,
    #[serde(skip)]
    pub sample_bank: Outsourcer<PathBuf, Result<Sample, String>, LoadSamples>,
    #[serde(skip)]
    loops: HashMap<InstrId, HashSet<InstrId>>,
    #[cfg(feature = "keyboard")]
    #[serde(skip)]
    keyboard: CloneLock<Option<Keyboard>>,
    #[cfg(feature = "keyboard")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_keyboard: Option<InstrId>,
    #[serde(skip)]
    new_script_stack: Vec<NewScript>,
}

impl Default for Instruments {
    fn default() -> Self {
        Instruments {
            output: None,
            map: HashMap::new(),
            tempo: 120.0,
            sample_queue: None,
            command_queue: Vec::new(),
            i: 0,
            last_drums: None,
            sample_bank: Outsourcer::default(),
            loops: HashMap::new(),
            #[cfg(feature = "keyboard")]
            keyboard: CloneLock::new(None),
            #[cfg(feature = "keyboard")]
            current_keyboard: None,
            new_script_stack: Vec::new(),
        }
    }
}

impl Instruments {
    pub fn new() -> SourceLock<Self> {
        SourceLock::new(Self::default())
    }
    pub fn frames_per_measure(&self) -> u32 {
        (SAMPLE_RATE as SampleType / (self.tempo / 60.0) * 4.0) as u32
    }
    pub fn i(&self) -> u32 {
        self.i
    }
    pub fn measure_i(&self) -> u32 {
        self.i % self.frames_per_measure()
    }
    pub fn set_output<I>(&mut self, id: I)
    where
        I: Into<InstrId>,
    {
        self.output = Some(id.into());
    }
    pub fn set_tempo(&mut self, tempo: SampleType) {
        self.tempo = tempo;
    }
    pub fn add<I>(&mut self, id: I, instr: Instrument)
    where
        I: Into<InstrId>,
    {
        self.map.insert(id.into(), instr);
    }
    pub fn add_wrapper<F>(&mut self, input: InstrId, id: InstrId, build_instr: F)
    where
        F: FnOnce(InstrId) -> Instrument,
    {
        for instr in self.map.values_mut() {
            instr.replace_input(input.clone(), id.clone());
        }
        if let Some(output) = &mut self.output {
            if output == &input {
                *output = id.clone();
            }
        }
        let new_instr = build_instr(input);
        self.add(id, new_instr);
    }
    pub fn input_devices_of<I>(&self, id: I) -> Vec<InstrId>
    where
        I: Into<InstrId>,
    {
        let id = id.into();
        if let Some(instr) = self.get(&id) {
            if instr.is_input_device() {
                vec![id]
            } else {
                instr
                    .inputs()
                    .into_iter()
                    .flat_map(|id| self.input_devices_of(id))
                    .collect()
            }
        } else {
            Vec::new()
        }
    }
    fn update_loops(&mut self) {
        self.loops.clear();
        for (id, instr) in &self.map {
            if let Instrument::Loop { input, .. } = instr {
                self.loops
                    .entry(input.clone())
                    .or_insert_with(HashSet::new)
                    .insert(id.clone());
            }
        }
    }
    pub fn add_loop<I>(&mut self, input: I, measures: u8)
    where
        I: Into<InstrId>,
    {
        // Stop recording on all other loops
        self.stop_recording_all();
        // Create a loop for every input device of this instrument
        for input in self.input_devices_of(input) {
            self._add_loop(input, measures);
        }
        // Update loops
        self.update_loops();
    }
    fn _add_loop(&mut self, input: InstrId, measures: u8) {
        // Create new loop id
        let mut i = 1;
        let loop_id = loop {
            let possible = input.as_loop(i);
            if self.get(&possible).is_none() {
                break possible;
            }
            i += 1;
        };
        // Create the loop instrument
        let frame_count = self.frames_per_measure() as usize * measures as usize;
        let loop_instr = Instrument::Loop {
            input,
            measures,
            recording: true,
            playing: true,
            frames: CloneLock::new(vec![
                LoopFrame {
                    frame: Frame::None,
                    new: true,
                };
                frame_count
            ]),
        };
        // Insert the loop
        println!("Added loop {}", loop_id);
        self.map.insert(loop_id, loop_instr);
    }
    pub fn get<I>(&self, id: I) -> Option<&Instrument>
    where
        I: Into<InstrId>,
    {
        self.map.get(&id.into())
    }
    pub fn get_mut<I>(&mut self, id: I) -> Option<&mut Instrument>
    where
        I: Into<InstrId>,
    {
        self.map.get_mut(&id.into())
    }
    pub fn next_from<'a, I>(&self, id: I, cache: &'a mut FrameCache) -> &'a Channels
    where
        I: Into<InstrId>,
    {
        let id = id.into();
        if cache.map.contains_key(&id) {
            // Get cached result
            cache.map.get(&id).unwrap()
        } else if cache.visited.contains(&id) {
            // Avoid infinite loops
            &cache.default_channels
        } else {
            cache.visited.insert(id.clone());
            if let Some(instr) = self.get(&id) {
                // Get the next set of channels
                let mut channels = instr.next(cache, self, id.clone());
                // Cache this initial version
                cache.map.insert(id.clone(), channels.clone());
                // Append loop channels
                if let Some(loop_ids) = self.loops.get(&id) {
                    for loop_id in loop_ids {
                        if let Some(instr) = self.map.get(loop_id) {
                            let loop_channel = instr
                                .next(cache, self, loop_id.clone())
                                .into_primary()
                                .map(|frame| (ChannelId::Loop(loop_id.clone()), frame));
                            channels.extend(loop_channel);
                        }
                    }
                    // Cache the result
                    *cache.map.get_mut(&id).unwrap() = channels;
                }
                cache.map.get(&id).unwrap()
            } else {
                &cache.default_channels
            }
        }
    }
    pub fn stop_recording_all(&mut self) {
        for (id, instr) in self.map.iter_mut() {
            if let Instrument::Loop { recording, .. } = instr {
                if *recording {
                    println!("Stopped recording {}", id);
                }
                *recording = false;
            }
        }
    }
    #[cfg_attr(not(feature = "keyboard"), allow(unused_variables))]
    pub fn default_voices_from<I>(&self, id: I) -> u32
    where
        I: Into<InstrId>,
    {
        #[cfg(feature = "keyboard")]
        {
            if let Some(instr) = self.get(id) {
                if let Instrument::Keyboard { .. } = instr {
                    6
                } else {
                    1
                }
            } else {
                1
            }
        }
        #[cfg(not(feature = "keyboard"))]
        1
    }
    #[cfg(feature = "keyboard")]
    pub fn new_keyboard(&mut self, id: Option<InstrId>) -> InstrId {
        let id = id.unwrap_or_else(|| {
            let mut i = 1;
            loop {
                let possible = InstrId::from(format!("kb{}", i));
                if self.get(&possible).is_none() {
                    break possible;
                }
                i += 1;
            }
        });
        self.keyboard(|_| {});
        self.add(&id, Instrument::Keyboard);
        self.current_keyboard = Some(id.clone());
        id
    }
    #[cfg(feature = "keyboard")]
    pub fn keyboard<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Keyboard) -> R,
    {
        let mut keyboard = self.keyboard.lock();
        let keyboard = keyboard.get_or_insert_with(|| Keyboard::new("Ryvm Keyboard"));
        f(keyboard)
    }
    pub fn queue_command(
        &mut self,
        delay: bool,
        args: Vec<String>,
        command: clap::Result<RyvmCommand>,
    ) {
        let end_script = matches!(command, Ok(RyvmCommand::End));
        if let (Some(script), false) = (self.new_script_stack.last_mut(), end_script) {
            script.commands.push((delay, args));
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
                let offender = if let Some([offender, ..]) = e.info.as_ref().map(Vec::as_slice) {
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
    fn process_ryvm_command(&mut self, command: RyvmCommand) {
        match command {
            RyvmCommand::Quit => {}
            RyvmCommand::Output { name } => self.set_output(name),
            RyvmCommand::Tempo { tempo } => self.set_tempo(tempo),
            RyvmCommand::Number { name, num } => self.add(name, Instrument::Number(num)),
            RyvmCommand::Wave {
                waveform,
                name,
                input,
                octave,
                attack,
                decay,
                sustain,
                release,
            } => {
                #[cfg(feature = "keyboard")]
                let input = if let Some(input) = input {
                    input
                } else {
                    self.new_keyboard(input)
                };
                let default_adsr = ADSR::default();
                let instr = Instrument::wave(
                    &input,
                    waveform,
                    octave,
                    ADSR {
                        attack: attack.unwrap_or(default_adsr.attack),
                        decay: decay.unwrap_or(default_adsr.decay),
                        sustain: sustain.unwrap_or(default_adsr.sustain),
                        release: release.unwrap_or(default_adsr.release),
                    },
                )
                .voices(self.default_voices_from(input));
                self.add(name, instr);
            }
            RyvmCommand::Mixer { name, inputs } => self.add(
                name,
                Instrument::Mixer(
                    inputs
                        .into_iter()
                        .map(Into::into)
                        .zip(repeat(Balance::default()))
                        .collect(),
                ),
            ),
            #[cfg(feature = "keyboard")]
            RyvmCommand::Keyboard { name } => {
                self.new_keyboard(Some(name));
            }
            RyvmCommand::Drums { name } => {
                self.add(name.clone(), Instrument::DrumMachine(Vec::new()));
                self.last_drums = Some(name);
            }
            RyvmCommand::Drum {
                machine_id,
                index,
                path,
                beat,
                repeat: rep,
                remove,
            } => {
                let name = if let Some(name) = machine_id {
                    self.last_drums = Some(name.clone());
                    name
                } else {
                    self.last_drums.clone().unwrap_or_default()
                };
                if let Some(Instrument::DrumMachine(samplings)) = self.get_mut(name) {
                    let index = index.unwrap_or_else(|| samplings.len());
                    samplings.resize(index + 1, Sampling::default());
                    if let Some(path) = path {
                        samplings[index].path = path;
                    }
                    if let Some(be) = beat {
                        samplings[index].beat = repeat(be.chars())
                            .take(rep.unwrap_or(1) as usize)
                            .flatten()
                            .collect::<String>()
                            .parse()
                            .unwrap();
                    }
                    if remove {
                        samplings[index] = Sampling::default();
                    }
                }
            }
            RyvmCommand::Loop { input, measures } => self.add_loop(input, measures),
            RyvmCommand::Start { inputs } => {
                for input in inputs {
                    if let Some(Instrument::Loop { playing, .. }) = self.get_mut(&input) {
                        *playing = true;
                    }
                }
            }
            RyvmCommand::Stop { inputs } => {
                for input in inputs {
                    if let Some(Instrument::Loop { playing, .. }) = self.get_mut(&input) {
                        *playing = false;
                    }
                }
            }
            RyvmCommand::Filter { input, value } => {
                let mut i = 1;
                while self.get(input.as_filter(i)).is_some() {
                    i += 1;
                }
                self.add_wrapper(input.clone(), input.as_filter(i), |input| {
                    Instrument::Filter {
                        input,
                        value,
                        avgs: Arc::new(CloneLock::new(Channels::new())),
                    }
                })
            }
            RyvmCommand::Ls { unsorted } => {
                let print = |ids: &mut dyn Iterator<Item = &InstrId>| {
                    for id in ids {
                        println!("    {}", id)
                    }
                };
                if unsorted {
                    print(&mut self.map.keys());
                } else {
                    print(
                        &mut self
                            .map
                            .iter()
                            .sorted_by(|(a_id, a_instr), (b_id, b_instr)| {
                                format!("{:?}", discriminant(*a_instr)).as_bytes()[14]
                                    .cmp(&format!("{:?}", discriminant(*b_instr)).as_bytes()[14])
                                    .then_with(|| a_id.cmp(b_id))
                            })
                            .map(|(id, _)| id),
                    );
                }
            }
            #[cfg(feature = "keyboard")]
            RyvmCommand::Focus { id } => {
                for input in self.input_devices_of(id) {
                    if let Some(Instrument::Keyboard { .. }) = self.get(&input) {
                        self.current_keyboard = Some(input);
                        break;
                    }
                }
            }
            RyvmCommand::Tree => {
                if let Some(output) = &self.output {
                    self.print_tree(output.clone(), 0);
                }
            }
            RyvmCommand::Script { name, args } => self.new_script_stack.push(NewScript {
                name,
                args,
                commands: Vec::new(),
            }),
            RyvmCommand::End => {
                if let Some(new_script) = self.new_script_stack.pop() {
                    self.add(
                        new_script.name,
                        Instrument::Script {
                            args: new_script.args,
                            commands: new_script.commands,
                        },
                    )
                }
            }
        }
    }
    fn process_instr_command(&mut self, name: InstrId, args: Vec<String>) -> Result<(), String> {
        let args = &args[1..];
        if let Some(instr) = self.get_mut(&name) {
            match instr {
                Instrument::Number(num) => {
                    let com = NumberCommand::from_iter_safe(args).map_err(|e| e.to_string())?;
                    *num = com.val;
                }
                Instrument::Mixer(inputs) => {
                    let com = MixerCommand::from_iter_safe(args).map_err(|e| e.to_string())?;

                    if com.remove {
                        for input in com.inputs {
                            inputs.remove(&input);
                        }
                    } else {
                        for input in com.inputs {
                            let balance = inputs.entry(input).or_insert_with(Balance::default);
                            if let Some(volume) = com.volume {
                                balance.volume = volume;
                            }
                            if let Some(pan) = com.pan {
                                balance.pan = pan;
                            }
                        }
                    }
                }
                Instrument::Wave {
                    input,
                    adsr,
                    octave,
                    ..
                } => {
                    let com = WaveCommand::from_iter_safe(args).map_err(|e| e.to_string())?;
                    if let Some(id) = com.input {
                        *input = id;
                    }
                    if let Some(o) = com.octave {
                        *octave = Some(o);
                    }
                    if let Some(attack) = com.attack {
                        adsr.attack = attack;
                    }
                    if let Some(decay) = com.decay {
                        adsr.decay = decay;
                    }
                    if let Some(sustain) = com.sustain {
                        adsr.sustain = sustain;
                    }
                    if let Some(release) = com.release {
                        adsr.release = release;
                    }
                }
                Instrument::Filter { value, .. } => {
                    let com = FilterCommand::from_iter_safe(args).map_err(|e| e.to_string())?;
                    *value = com.value;
                }
                Instrument::Script {
                    args: script_args,
                    commands: unresolved_commands,
                } => {
                    let script_clap_args: Vec<clap::Arg> = script_args
                        .iter()
                        .enumerate()
                        .map(|(i, arg_name)| {
                            clap::Arg::with_name(arg_name)
                                .index(i as u64 + 1)
                                .required(true)
                        })
                        .collect();
                    let script_app = clap::App::new(&name.to_string()).args(&script_clap_args);
                    match script_app.get_matches_from_safe(&*args) {
                        Ok(matches) => {
                            let mut resolved_commands = Vec::new();
                            for (delay, unresolved_command) in unresolved_commands {
                                let resolved_command: Vec<String> = unresolved_command
                                    .iter()
                                    .map(|arg| {
                                        if let Some(script_arg) =
                                            script_args.iter().find(|sa| sa == &arg)
                                        {
                                            matches.value_of(script_arg).unwrap().into()
                                        } else {
                                            arg.clone()
                                        }
                                    })
                                    .collect();
                                let parsed = RyvmCommand::from_iter_safe(&resolved_command);
                                resolved_commands.push((*delay, resolved_command, parsed))
                            }
                            for (delay, args, parsed) in resolved_commands {
                                self.queue_command(delay, args, parsed);
                            }
                        }
                        Err(e) => println!("{}", e),
                    }
                }
                _ => return Err(format!("No commands available for \"{}\"", name)),
            }
            Ok(())
        } else {
            Err(format!("No instrument or command \"{}\"", name))
        }
    }
    fn print_tree(&self, root: InstrId, depth: usize) {
        let exists = self.get(&root).is_some();
        print!(
            "{}{}{}",
            (0..(2 * depth)).map(|_| ' ').collect::<String>(),
            root,
            if exists { "" } else { "?" }
        );
        if let Some(instr) = self.get(&root) {
            if let Some(loops) = self.loops.get(&root) {
                for loop_id in loops {
                    print!(" ({})", loop_id);
                }
            }
            println!();
            for input in instr.inputs() {
                self.print_tree(input.into(), depth + 1);
            }
        } else {
            println!();
        }
    }
}

impl Iterator for Instruments {
    type Item = SampleType;
    fn next(&mut self) -> Option<Self::Item> {
        // Process commands
        if self.measure_i() == 0 && self.sample_queue.is_none() {
            let mut commands = Vec::new();
            swap(&mut commands, &mut self.command_queue);
            for (args, app) in commands {
                self.process_command(args, app);
            }
        }
        // Init cache
        let mut cache = FrameCache::default();
        // Get next sample
        self.sample_queue
            .take()
            .map(|samp| {
                self.i += 1;
                samp
            })
            .or_else(|| {
                if let Some(output_id) = &self.output {
                    let channels = self.next_from(output_id, &mut cache);
                    let voices: Vec<(Voice, Balance)> = channels
                        .frames()
                        .map(|frame| (frame.voice(), Balance::default()))
                        .collect();
                    let frame = mix(&voices);
                    self.sample_queue = Some(frame.right());
                    return Some(frame.left());
                }
                self.sample_queue = Some(0.0);
                Some(0.0)
            })
    }
}

impl Source for SourceLock<Instruments> {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }
    fn total_duration(&self) -> std::option::Option<std::time::Duration> {
        None
    }
}
