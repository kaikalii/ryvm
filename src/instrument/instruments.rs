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
use structopt::{clap, StructOpt};

#[cfg(feature = "keyboard")]
use crate::Keyboard;
use crate::{
    adjust_i, load_script, mix, Balance, ChannelId, Channels, CloneCell, CloneLock, DrumMachine,
    FilterCommand, FocusType, FrameCache, InstrId, Instrument, LoopMaster, Midi, MidiSubcommand,
    MixerCommand, RyvmApp, RyvmCommand, Sample, SourceLock, Voice, WaveCommand, ADSR,
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

#[derive(Debug)]
pub struct Instruments {
    pub sample_rate: u32,
    output: Option<InstrId>,
    map: HashMap<InstrId, Instrument>,
    pub tempo: f32,
    sample_queue: Option<f32>,
    command_queue: Vec<(Vec<String>, clap::Result<RyvmCommand>)>,
    i: u32,
    last_drums: Option<InstrId>,
    pub sample_bank: Outsourcer<PathBuf, Result<Sample, String>, LoadSamples>,
    loops: HashMap<InstrId, HashSet<u8>>,
    pub loop_validators: HashMap<u8, InstrId>,
    pub loop_master: Option<LoopMaster>,
    #[cfg(feature = "keyboard")]
    keyboard: CloneLock<Option<Keyboard>>,
    pub midis: HashMap<usize, Midi>,
    #[cfg(feature = "keyboard")]
    pub current_keyboard: Option<InstrId>,
    pub default_midi: Option<InstrId>,
    pub focused: HashMap<FocusType, InstrId>,
    new_script_stack: Vec<NewScript>,
    pub debug: bool,
    pub debug_live: bool,
}

impl Default for Instruments {
    fn default() -> Self {
        let app = RyvmApp::from_iter_safe(std::env::args()).unwrap_or_default();
        let mut instruments = Instruments {
            sample_rate: app.sample_rate.unwrap_or(44100),
            output: None,
            map: HashMap::new(),
            tempo: 1.0,
            sample_queue: None,
            command_queue: Vec::new(),
            i: 0,
            last_drums: None,
            sample_bank: Outsourcer::default(),
            loops: HashMap::new(),
            loop_master: None,
            loop_validators: HashMap::new(),
            #[cfg(feature = "keyboard")]
            keyboard: CloneLock::new(None),
            midis: HashMap::new(),
            #[cfg(feature = "keyboard")]
            current_keyboard: None,
            default_midi: None,
            focused: HashMap::new(),
            new_script_stack: Vec::new(),
            debug: false,
            debug_live: false,
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

impl Instruments {
    pub fn new() -> SourceLock<Self> {
        SourceLock::new(Self::default())
    }
    pub fn frames_per_measure(&self) -> u32 {
        (self.sample_rate as f32 / (self.tempo / 60.0) * 4.0) as u32
    }
    pub fn i(&self) -> u32 {
        self.i
    }
    pub fn measure_i(&self) -> u32 {
        self.i % self.frames_per_measure()
    }
    pub fn set_output(&mut self, id: InstrId) {
        self.output = Some(id);
    }
    pub fn set_tempo(&mut self, tempo: f32) {
        self.tempo = tempo;
    }
    pub fn add(&mut self, id: InstrId, instr: Instrument) {
        self.map.insert(id, instr);
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
    pub fn input_devices_of(&self, id: &InstrId) -> Vec<InstrId> {
        if let Some(instr) = self.get(&id) {
            if instr.is_input_device() {
                vec![id.clone()]
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
            if let InstrId::Loop(i) = id {
                if let Instrument::Loop { input, .. } | Instrument::InitialLoop { input, .. } =
                    instr
                {
                    self.loops
                        .entry(input.clone())
                        .or_insert_with(HashSet::new)
                        .insert(*i);
                }
            }
        }
    }
    pub fn add_loop(&mut self, number: u8, input: InstrId, size: f32) {
        // Stop recording on all other loops
        self.stop_recording_all();
        // Clear the loop master if its id matches the number
        if let Some(master) = self.loop_master {
            if master.id == number {
                self.loop_master = None;
            }
        }
        // Create a loop for every input device of this instrument
        for input in self.input_devices_of(&input) {
            self._add_loop(number, input, size);
        }
        self.loop_validators.insert(number, input.clone());
        self.set_focus(input);
        // Update loops
        self.update_loops();
    }
    fn _add_loop(&mut self, number: u8, input: InstrId, size: f32) {
        // Create new loop id
        let loop_id = InstrId::Loop(number);
        // Create the loop instrument
        let loop_instr = if self.loop_master.is_some() {
            Instrument::Loop {
                input,
                recording: true,
                playing: true,
                tempo: self.tempo,
                last_frames: CloneLock::new(Default::default()),
                frames: CloneLock::new(Default::default()),
                size,
            }
        } else {
            Instrument::InitialLoop {
                input,
                frames: Some(CloneLock::new(Default::default())),
                start_i: CloneCell::new(None),
            }
        };
        // Insert the loop
        println!("Added loop {}", loop_id);
        self.map.insert(loop_id, loop_instr);
    }
    pub fn get(&self, id: &InstrId) -> Option<&Instrument> {
        self.map.get(id)
    }
    pub fn get_mut(&mut self, id: &InstrId) -> Option<&mut Instrument> {
        self.map.get_mut(id)
    }
    pub fn next_from<'a>(&self, id: &InstrId, cache: &'a mut FrameCache) -> &'a Channels {
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
                if let Some(loop_i) = self.loops.get(&id) {
                    for &loop_i in loop_i {
                        let loop_id = InstrId::Loop(loop_i);
                        if let Some(instr) = self.map.get(&loop_id) {
                            let loop_channels = instr
                                .next(cache, self, loop_id.clone())
                                .into_iter()
                                .map(|(_, channel)| (ChannelId::Loop(false, loop_i), channel));
                            channels.extend(loop_channels);
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
        let self_i = self.i();
        let mut loop_master = None;
        let curr_tempo = self.tempo;
        let lm = self.loop_master;
        for (id, instr) in self.map.iter_mut() {
            if let InstrId::Loop(loop_id) = id {
                match instr {
                    Instrument::Loop {
                        recording,
                        frames,
                        last_frames,
                        tempo,
                        ..
                    } => {
                        if *recording {
                            let mut frames = frames.lock();
                            let lm = lm.expect("logic error: Loop is running with no master set");
                            let loop_i =
                                adjust_i(self_i - lm.start_i, *tempo, curr_tempo) % lm.period;
                            frames.split_off(&loop_i);
                            frames.append(&mut last_frames.lock().split_off(&loop_i));
                            println!("Stopped recording {}", id);
                            *recording = false;
                        }
                    }
                    Instrument::InitialLoop {
                        input,
                        frames,
                        start_i,
                        ..
                    } => {
                        let start_i = start_i.load();
                        if let Some(start_i) = start_i {
                            let input = input.clone();
                            let frames = frames.take().unwrap();
                            let period = self_i - start_i;
                            loop_master = Some(LoopMaster {
                                id: *loop_id,
                                start_i,
                                period,
                            });
                            *instr = Instrument::Loop {
                                input,
                                frames,
                                recording: false,
                                playing: true,
                                tempo: self.tempo,
                                last_frames: CloneLock::new(Default::default()),
                                size: 1.0,
                            };
                            println!("Finished recording {}", id);
                        } else {
                            println!("Cancelled recording {}", id);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.loop_master = self.loop_master.or(loop_master);
        self.update_loops();
    }
    #[cfg_attr(not(feature = "keyboard"), allow(unused_variables))]
    pub fn default_voices_from(&self, id: &InstrId) -> u32 {
        if let Some(instr) = self.get(id) {
            match instr {
                #[cfg(feature = "keyboard")]
                Instrument::Keyboard { .. } => 5,
                Instrument::Midi { .. } => 8,
                _ => 1,
            }
        } else {
            1
        }
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
        self.add(id.clone(), Instrument::Keyboard);
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
    fn process_ryvm_command(&mut self, command: RyvmCommand) {
        match command {
            RyvmCommand::Quit => {}
            RyvmCommand::Output { name } => self.set_output(name),
            RyvmCommand::Tempo { tempo } => self.set_tempo(tempo),
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
                let input = input.unwrap_or_else(InstrId::default_input_device);
                let default_adsr = ADSR::default();
                let instr = Instrument::wave(
                    input.clone(),
                    waveform,
                    octave,
                    ADSR {
                        attack: attack.unwrap_or(default_adsr.attack),
                        decay: decay.unwrap_or(default_adsr.decay),
                        sustain: sustain.unwrap_or(default_adsr.sustain),
                        release: release.unwrap_or(default_adsr.release),
                    },
                    self.default_voices_from(&input),
                );
                self.focused.insert(FocusType::Keyboard, name.clone());
                self.add(name, instr);
            }
            RyvmCommand::Mixer { name, inputs } => self.add(
                name,
                Instrument::Mixer(
                    inputs
                        .into_iter()
                        .map(Into::into)
                        .zip(repeat(Balance::mixer_default()))
                        .collect(),
                ),
            ),
            #[cfg(feature = "keyboard")]
            RyvmCommand::Keyboard { name } => {
                self.new_keyboard(Some(name));
            }
            RyvmCommand::Midi(MidiSubcommand::List) => match Midi::ports_list() {
                Ok(list) => {
                    for (i, name) in list.into_iter().enumerate() {
                        println!("{}. {}", i, name);
                    }
                }
                Err(e) => println!("{}", e),
            },
            RyvmCommand::Midi(MidiSubcommand::Init { port }) => {
                let port = port.unwrap_or(0);
                let name = InstrId::from(format!("midi{}", port));
                if self.midis.contains_key(&port) {
                    self.default_midi = Some(name.clone());
                    self.add(name, Instrument::Midi { port });
                } else {
                    match Midi::new(&name.to_string(), port) {
                        Ok(midi) => {
                            self.midis.entry(port).or_insert(midi);
                            self.default_midi = Some(name.clone());
                            self.add(name, Instrument::Midi { port });
                        }
                        Err(e) => println!("{}", e),
                    }
                }
            }
            RyvmCommand::Knob {
                name,
                number,
                min,
                max,
                input,
            } => {
                let knob_instr = Instrument::Knob {
                    control_id: number,
                    input: input.unwrap_or_else(InstrId::default_input_device),
                    min: min.unwrap_or(0.0),
                    max: max.unwrap_or(1.0),
                    state: CloneCell::new(0x40),
                };
                self.add(name, knob_instr);
            }
            RyvmCommand::Drums { name, input } => {
                let input = input.unwrap_or_else(InstrId::default_input_device);
                self.add(
                    name.clone(),
                    Instrument::DrumMachine(Box::new(DrumMachine {
                        samples: Vec::new(),
                        input,
                        samplings: CloneLock::new(HashMap::new()),
                    })),
                );
                self.focused.insert(FocusType::Drum, name.clone());
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
                if let Some(Instrument::DrumMachine(drums)) = self.get_mut(&name) {
                    let index = index.unwrap_or_else(|| drums.samples.len());
                    drums.samples.resize(index + 1, PathBuf::from(""));
                    if let Some(path) = path {
                        drums.samples[index] = path;
                    }
                    if remove {
                        drums.samples[index] = PathBuf::from("");
                    }
                }
            }
            RyvmCommand::Loop {
                number,
                input,
                size,
            } => self.add_loop(number, input, size.unwrap_or(1.0)),
            RyvmCommand::Start { loops } => {
                for i in loops {
                    if let Some(Instrument::Loop { playing, .. }) = self.get_mut(&InstrId::Loop(i))
                    {
                        *playing = true;
                    }
                }
            }
            RyvmCommand::Stop { loops } => {
                for i in loops {
                    if let Some(Instrument::Loop { playing, .. }) = self.get_mut(&InstrId::Loop(i))
                    {
                        *playing = false;
                    }
                }
            }
            RyvmCommand::Filter { input, value } => {
                let mut i = 1;
                while self.get(&input.as_filter(i)).is_some() {
                    i += 1;
                }
                self.add_wrapper(input.clone(), input.as_filter(i), |input| {
                    Instrument::Filter {
                        input,
                        value,
                        avgs: Arc::new(CloneLock::new(HashMap::new())),
                    }
                })
            }
            RyvmCommand::Ls { unsorted } => self.print_ls(unsorted),
            RyvmCommand::Focus { id } => self.set_focus(id),
            RyvmCommand::Tree => {
                let any_scripts = self
                    .map
                    .values()
                    .any(|instr| matches!(instr, Instrument::Script{..}));
                if any_scripts {
                    println!("~~~~~ Scripts ~~~~~");
                    for (id, _) in self
                        .map
                        .iter()
                        .filter(|(_, instr)| matches!(instr, Instrument::Script{..}))
                    {
                        println!("{}", id)
                    }
                }
                if let Some(output) = &self.output {
                    println!("~~~ Instruments ~~~");
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
            RyvmCommand::Rm { id, recursive } => {
                self.remove(&id, recursive);
                self.update_loops();
            }
            RyvmCommand::Load { name } => self.load_script(&name, true),
            RyvmCommand::Run { name, args } => {
                self.load_script(&name, false);
                if let Err(e) = self.run_script_by_name(&name, &args) {
                    println!("{}", e)
                }
            }
            RyvmCommand::Debug { live } => {
                self.debug = !self.debug;
                if live {
                    self.debug_live = !self.debug_live;
                }
            }
        }
    }
    fn process_instr_command(&mut self, name: InstrId, args: Vec<String>) -> Result<(), String> {
        let args = &args[1..];
        if let Some(instr) = self.get_mut(&name) {
            match instr {
                Instrument::Mixer(inputs) => {
                    let com = MixerCommand::from_iter_safe(args).map_err(|e| e.to_string())?;

                    if com.remove {
                        for input in com.inputs {
                            inputs.remove(&input);
                        }
                    } else {
                        for input in com.inputs {
                            let balance =
                                inputs.entry(input).or_insert_with(Balance::mixer_default);
                            if let Some(volume) = com.volume {
                                balance.volume = volume;
                            }
                            if let Some(pan) = com.pan {
                                balance.pan = pan;
                            }
                        }
                    }
                }
                Instrument::Wave(wave) => {
                    let com = WaveCommand::from_iter_safe(args).map_err(|e| e.to_string())?;
                    if let Some(id) = com.input {
                        wave.input = id;
                    }
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
                }
                Instrument::Filter { value, .. } => {
                    let com = FilterCommand::from_iter_safe(args).map_err(|e| e.to_string())?;
                    *value = com.value;
                }
                Instrument::Script {
                    args: script_args,
                    commands: unresolved_commands,
                } => {
                    let script_args = script_args.clone();
                    let unresolved_commands = unresolved_commands.clone();
                    self.run_script(args, &name.to_string(), script_args, unresolved_commands)?
                }
                _ => return Err(format!("No commands available for \"{}\"", name)),
            }
            Ok(())
        } else {
            Err(format!("No instrument or command \"{}\"", name))
        }
    }
    fn set_focus(&mut self, id: InstrId) {
        if let Some(instr) = self.get(&id) {
            match instr {
                Instrument::Wave(_) => {
                    self.focused.insert(FocusType::Keyboard, id);
                }
                Instrument::DrumMachine(_) => {
                    self.focused.insert(FocusType::Drum, id);
                }
                _ => {}
            }
        }
    }
    fn load_script(&mut self, name: &str, reload: bool) {
        if self.get(&name.into()).is_none() || reload {
            if let Some((args, commands)) = load_script(&name) {
                let instr = Instrument::Script { args, commands };
                self.add(name.into(), instr);
            }
        }
    }
    fn run_script_by_name(&mut self, name: &str, args: &[String]) -> Result<(), String> {
        if let Some(Instrument::Script {
            args: script_args,
            commands,
        }) = self.get(&name.into())
        {
            let script_args = script_args.clone();
            let unresolved_commands = commands.clone();
            self.run_script(args, name, script_args, unresolved_commands)
        } else {
            Ok(())
        }
    }
    fn run_script(
        &mut self,
        command_args: &[String],
        script_name: &str,
        script_args: Vec<String>,
        unresolved_commands: Vec<(bool, Vec<String>)>,
    ) -> Result<(), String> {
        let script_clap_args: Vec<clap::Arg> = script_args
            .iter()
            .enumerate()
            .map(|(i, arg_name)| {
                clap::Arg::with_name(arg_name)
                    .index(i as u64 + 1)
                    .required(true)
            })
            .collect();
        let script_app = clap::App::new(script_name).args(&script_clap_args);
        let matches = script_app
            .get_matches_from_safe(command_args)
            .map_err(|e| e.to_string())?;
        let mut resolved_commands = Vec::new();
        for (delay, unresolved_command) in unresolved_commands {
            let resolved_command: Vec<String> = unresolved_command
                .iter()
                .map(|arg| {
                    if let Some(script_arg) = script_args.iter().find(|sa| sa == &arg) {
                        matches.value_of(script_arg).unwrap().into()
                    } else {
                        arg.clone()
                    }
                })
                .collect();
            let parsed = RyvmCommand::from_iter_safe(&resolved_command);
            resolved_commands.push((delay, resolved_command, parsed))
        }
        let mut depth = 0;
        for (delay, args, parsed) in resolved_commands {
            if let Some("end") = args.get(1).map(|s| s.as_str()) {
                depth -= 1;
            }
            print!("> {}", (0..depth).map(|_| "  ").collect::<String>());
            for arg in args.iter().skip(1) {
                print!("{} ", arg);
            }
            println!();
            if let Some("script") = args.get(1).map(|s| s.as_str()) {
                depth += 1;
            }
            self.queue_command(delay, args, parsed);
        }
        Ok(())
    }
    fn remove(&mut self, id: &InstrId, recursive: bool) {
        if let Some(instr) = self.get(id) {
            if recursive {
                let inputs: Vec<_> = instr.inputs().into_iter().cloned().collect();
                for input in inputs {
                    if !self
                        .map
                        .iter()
                        .filter(|(i, _)| i != &id)
                        .any(|(_, instr)| instr.inputs().contains(&&input))
                    {
                        self.remove(&input, recursive);
                    }
                }
            }
            self.map.remove(&id);
        }
    }
    fn print_ls(&self, unsorted: bool) {
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
                for loop_id in loops.iter().sorted() {
                    print!(" ({})", loop_id);
                }
            }
            println!();
            for input in instr.inputs().into_iter().sorted() {
                self.print_tree(input.clone(), depth + 1);
            }
        } else {
            println!();
        }
    }
}

impl Iterator for Instruments {
    type Item = f32;
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
                    // if self.debug_live {
                    //     for id in channels.keys() {
                    //         print!("{:?}, ", id);
                    //     }
                    //     println!();
                    // }
                    let voices: Vec<(Voice, Balance)> = channels
                        .iter()
                        .filter(|(id, _)| id.is_validated())
                        .map(|(_, frame)| (frame.voice(), Balance::default()))
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
        self.update(|instrs| instrs.sample_rate)
    }
    fn total_duration(&self) -> std::option::Option<std::time::Duration> {
        None
    }
}
