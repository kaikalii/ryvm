use std::{
    collections::{HashMap, HashSet},
    iter::repeat,
    mem::swap,
    path::PathBuf,
    sync::Arc,
};

use indexmap::IndexMap;
use outsource::{JobDescription, Outsourcer};
use rodio::Source;
use serde_derive::{Deserialize, Serialize};
use structopt::{clap, StructOpt};

#[cfg(feature = "keyboard")]
use crate::Keyboard;
use crate::{
    mix, Balance, ChannelId, Channels, CloneLock, FilterSetting, FrameCache, InstrId, InstrIdType,
    Instrument, KeyboardCommand, LoopFrame, MixerCommand, NumberCommand, RyvmApp, RyvmCommand,
    Sample, SampleType, Sampling, SourceLock, Voice, WaveCommand, WaveForm, SAMPLE_EPSILON,
    SAMPLE_RATE,
};

fn default_tempo() -> SampleType {
    120.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_tempo(tempo: &SampleType) -> bool {
    (tempo - default_tempo()).abs() < SAMPLE_EPSILON
}

#[derive(Default)]
pub(crate) struct LoadSamples {}

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

#[derive(Debug, Serialize, Deserialize)]
pub struct Instruments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output: Option<InstrId>,
    #[serde(
        rename = "instruments",
        default,
        skip_serializing_if = "IndexMap::is_empty"
    )]
    map: IndexMap<InstrId, Instrument>,
    #[serde(default = "default_tempo", skip_serializing_if = "is_default_tempo")]
    tempo: SampleType,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    groups: HashMap<String, InstrId>,
    #[serde(skip)]
    sample_queue: Option<SampleType>,
    #[serde(skip)]
    command_queue: Vec<(Vec<String>, clap::Result<RyvmApp>)>,
    #[serde(skip)]
    i: u32,
    #[serde(skip)]
    last_drums: Option<InstrId>,
    #[serde(skip)]
    pub(crate) sample_bank: Outsourcer<PathBuf, Result<Sample, String>, LoadSamples>,
    #[serde(skip)]
    loops: HashMap<InstrId, HashSet<InstrId>>,
}

impl Default for Instruments {
    fn default() -> Self {
        Instruments {
            output: None,
            map: IndexMap::new(),
            tempo: 120.0,
            groups: HashMap::new(),
            sample_queue: None,
            command_queue: Vec::new(),
            i: 0,
            last_drums: None,
            sample_bank: Outsourcer::default(),
            loops: HashMap::new(),
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
    pub fn add_wrapper<F>(&mut self, input: String, sub_type: InstrIdType, build_instr: F)
    where
        F: FnOnce(InstrId) -> Instrument,
    {
        let mut id = InstrId::new_base(&input);
        id.ty = sub_type;
        let old_top = if let Some(top) = self.groups.get(&input) {
            top.clone()
        } else if self.get(&input).is_some() {
            input.parse().unwrap()
        } else {
            return;
        };
        let instr = build_instr(old_top);
        self.groups.insert(input, id.clone());
        self.add(id, instr);
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
    pub fn add_loop<I>(&mut self, loop_id: Option<InstrId>, id: I, measures: u8)
    where
        I: Into<InstrId>,
    {
        // Create new loop id
        let id = id.into();
        let mut i = 0;
        let loop_id = loop_id.unwrap_or_else(|| loop {
            let possible = id.as_loop(i);
            if self.get(&possible).is_none() {
                break possible;
            }
            i += 1;
        });
        // Create the loop instrument
        let frame_count = self.frames_per_measure() as usize * measures as usize;
        let loop_instr = Instrument::Loop {
            input: id,
            measures,
            recording: true,
            playing: true,
            frames: CloneLock::new(vec![
                LoopFrame {
                    frame: None,
                    new: true,
                };
                frame_count
            ]),
        };
        // Stop recording on all other loops
        self.stop_recording_all();
        // Insert the loop
        println!("Added loop {}", loop_id);
        self.map.insert(loop_id, loop_instr);
        // Update loops
        self.update_loops();
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
    pub fn get_skip_loops<I>(&self, id: I) -> Option<&Instrument>
    where
        I: Into<InstrId>,
    {
        let mut id = id.into();
        loop {
            if let Some(Instrument::Loop { input, .. }) = self.get(&id) {
                id = input.clone();
            } else {
                break self.get(&id);
            }
        }
    }
    pub(crate) fn next_from<'a, I>(
        &self,
        id: I,
        cache: &'a mut FrameCache,
        caller: Option<InstrId>,
    ) -> &'a Channels
    where
        I: Into<InstrId>,
    {
        let id = id.into();
        // Get the correct instrument
        let use_groups = caller
            .as_ref()
            .map(|caller| caller.name != id.name)
            .unwrap_or(true);
        let id = if use_groups {
            self.groups.get(&id.name).unwrap_or(&id)
        } else {
            &id
        };
        if cache.map.contains_key(&id) {
            // Get cached result
            cache.map.get(&id).unwrap()
        } else if cache.visited.contains(&id) {
            // Avoid infinite loops
            &cache.default_channels
        } else {
            cache.visited.insert(id.clone());
            if let Some(instr) = self.get(id) {
                // Get the next set of channels
                let mut channels = instr.next(cache, self, Some(id.clone()));
                // Cache this initial version
                cache.map.insert(id.clone(), channels.clone());
                // Append loop channels
                if let Some(loop_ids) = self.loops.get(&id) {
                    for loop_id in loop_ids {
                        if let Some(instr) = self.map.get(loop_id) {
                            channels.extend(
                                instr
                                    .next(cache, self, Some(id.clone()))
                                    .into_primary()
                                    .map(|frame| (ChannelId::Loop(loop_id.clone()), frame)),
                            );
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
    pub fn queue_command(&mut self, args: Vec<String>, app: clap::Result<RyvmApp>) {
        if let Ok(RyvmApp {
            command: Some(RyvmCommand::Drum {
                path: Some(path), ..
            }),
            ..
        }) = &app
        {
            self.sample_bank.restart_if(path.clone(), |r| r.is_err());
        }
        self.command_queue.push((args, app));
    }
    pub fn stop_recording_all(&mut self) {
        for instr in self.map.values_mut() {
            if let Instrument::Loop { recording, .. } = instr {
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
            if let Some(instr) = self.get_skip_loops(id) {
                if let Instrument::Keyboard(_) = instr {
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
    fn process_ryvm_command(&mut self, name: Option<InstrId>, command: RyvmCommand) {
        match command {
            RyvmCommand::Quit => {}
            RyvmCommand::Output { name } => self.set_output(name),
            RyvmCommand::Tempo { tempo } => self.set_tempo(tempo),
            RyvmCommand::Number { num } => {
                if let Some(name) = name {
                    self.add(name, Instrument::Number(num))
                }
            }
            RyvmCommand::Sine { input, voices } => {
                if let Some(name) = name {
                    let instr = Instrument::wave(&input, WaveForm::Sine)
                        .voices(voices.unwrap_or_else(|| self.default_voices_from(input)));
                    self.add(name, instr);
                }
            }
            RyvmCommand::Square { input, voices } => {
                if let Some(name) = name {
                    let instr = Instrument::wave(&input, WaveForm::Square)
                        .voices(voices.unwrap_or_else(|| self.default_voices_from(input)));
                    self.add(name, instr);
                }
            }
            RyvmCommand::Saw { input, voices } => {
                if let Some(name) = name {
                    let instr = Instrument::wave(&input, WaveForm::Saw)
                        .voices(voices.unwrap_or_else(|| self.default_voices_from(input)));
                    self.add(name, instr);
                }
            }
            RyvmCommand::Triangle { input, voices } => {
                if let Some(name) = name {
                    let instr = Instrument::wave(&input, WaveForm::Triangle)
                        .voices(voices.unwrap_or_else(|| self.default_voices_from(input)));
                    self.add(name, instr);
                }
            }
            RyvmCommand::Mixer { inputs } => {
                if let Some(name) = name {
                    self.add(
                        name,
                        Instrument::Mixer(
                            inputs
                                .into_iter()
                                .map(Into::into)
                                .zip(repeat(Balance::default()))
                                .collect(),
                        ),
                    )
                }
            }
            #[cfg(feature = "keyboard")]
            RyvmCommand::Keyboard { octave } => {
                let name = name.unwrap_or_else(|| "kb".into());
                self.add(
                    name.clone(),
                    Instrument::Keyboard(Keyboard::new(&name.to_string(), octave.unwrap_or(4))),
                )
            }
            RyvmCommand::Drums => {
                if let Some(name) = name {
                    self.add(name.clone(), Instrument::DrumMachine(Vec::new()));
                    self.last_drums = Some(name);
                }
            }
            RyvmCommand::Drum {
                index,
                path,
                beat,
                repeat: rep,
                remove,
            } => {
                let name = if let Some(name) = name {
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
            RyvmCommand::Loop { input, measures } => self.add_loop(name, input, measures),
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
            RyvmCommand::Filter { input, setting } => {
                let mut i = 0;
                let test_id = InstrId::new_base(&input);
                while self.get(test_id.filter(i)).is_some() {
                    i += 1;
                }
                self.add_wrapper(input, InstrIdType::Filter(i), |input| Instrument::Filter {
                    input,
                    setting: FilterSetting::Static(setting),
                    avgs: Arc::new(CloneLock::new(Channels::new())),
                })
            }
        }
    }
    fn process_instr_command(&mut self, name: InstrId, args: Vec<String>) {
        let args = &args[1..];
        if let Some(instr) = self.get_mut(name) {
            match instr {
                Instrument::Number(num) => match NumberCommand::from_iter_safe(args) {
                    Ok(com) => *num = com.val,
                    Err(e) => println!("{}", e),
                },
                Instrument::Mixer(inputs) => match MixerCommand::from_iter_safe(args) {
                    Ok(com) => {
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
                    Err(e) => println!("{}", e),
                },
                Instrument::Wave { input, .. } => match WaveCommand::from_iter_safe(args) {
                    Ok(com) => *input = com.input,
                    Err(e) => println!("{}", e),
                },
                #[cfg(feature = "keyboard")]
                Instrument::Keyboard(keyboard) => match KeyboardCommand::from_iter_safe(args) {
                    Ok(com) => keyboard.set_base_octave(com.octave),
                    Err(e) => println!("{}", e),
                },
                Instrument::Loop { .. } => unreachable!(),
                _ => {}
            }
        }
    }
    fn process_command(&mut self, args: Vec<String>, app: clap::Result<RyvmApp>) {
        self.stop_recording_all();
        match app {
            Ok(RyvmApp {
                name,
                command: Some(command),
                ..
            }) => self.process_ryvm_command(name, command),
            Ok(RyvmApp {
                name: Some(name),
                command: None,
                ..
            }) => self.process_instr_command(name, args),
            Ok(_) => {}
            Err(e) => match args.as_slice() {
                [_, name, ..] => self.process_instr_command(name.parse().unwrap(), args),
                _ => println!("{}", e),
            },
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
                    let channels = self.next_from(output_id, &mut cache, None);
                    let voices: Vec<(Voice, Balance)> = channels
                        .iter()
                        .map(|(_, frame)| (frame.first, Balance::default()))
                        .collect();
                    if let Some(frame) = mix(&voices) {
                        self.sample_queue = Some(frame.first.right);
                        return Some(frame.first.left);
                    }
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
