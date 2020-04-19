use std::{
    collections::{HashMap, HashSet},
    iter::repeat,
    mem::swap,
    path::PathBuf,
};

use indexmap::IndexMap;
use outsource::{JobDescription, Outsourcer};
use rodio::Source;
use serde_derive::{Deserialize, Serialize};

#[cfg(feature = "keyboard")]
use crate::Keyboard;
use crate::{
    mix, Balance, CloneLock, Frame, FrameCache, InstrId, Instrument, LoopFrame, RyvmApp,
    RyvmCommand, Sample, SampleBank, SampleType, Sampling, SourceLock, Voice, WaveForm,
    SAMPLE_EPSILON, SAMPLE_RATE,
};

fn default_tempo() -> SampleType {
    120.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_tempo(tempo: &SampleType) -> bool {
    (tempo - default_tempo()).abs() < SAMPLE_EPSILON
}

#[derive(Default)]
struct LoadSamples {}

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
    output: Option<InstrId>,
    #[serde(rename = "instruments")]
    map: IndexMap<InstrId, Instrument>,
    #[serde(skip)]
    sample_queue: Option<SampleType>,
    #[serde(skip)]
    command_queue: Vec<RyvmApp>,
    #[serde(default = "default_tempo", skip_serializing_if = "is_default_tempo")]
    tempo: SampleType,
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
            sample_queue: None,
            command_queue: Vec::new(),
            tempo: 120.0,
            i: 0,
            last_drums: None,
            sample_bank: SampleBank::new(),
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
    pub fn add_loop<I>(&mut self, loop_id: Option<String>, id: I, measures: u8)
    where
        I: Into<InstrId>,
    {
        // Create new loop id
        let id = id.into();
        let mut i = 1;
        let loop_id = loop_id.unwrap_or_else(|| loop {
            let possible = format!("loop{}", i);
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
        println!("Added loop {:?}", loop_id);
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
    pub fn get_mut_skip_loops<I>(&mut self, id: I) -> Option<&mut Instrument>
    where
        I: Into<InstrId>,
    {
        let mut id = id.into();
        loop {
            if let Some(Instrument::Loop { input, .. }) = self.get_mut(&id) {
                id = input.clone();
            } else {
                break self.get_mut(&id);
            }
        }
    }
    pub(crate) fn next_from<'a, I>(&self, id: I, cache: &'a mut FrameCache) -> &'a [Frame]
    where
        I: Into<InstrId>,
    {
        let id = id.into();
        if cache.map.contains_key(&id) {
            // Get cached result
            cache.map.get(&id).unwrap()
        } else if cache.visited.contains(&id) {
            // Avoid infinite loops
            &cache.default_frames
        } else {
            cache.visited.insert(id.clone());
            if let Some(instr) = self.map.get(&id) {
                // Get the next set of frames
                let mut frames = instr.next(cache, self);
                // Cache this initial version
                cache.map.insert(id.clone(), frames.clone());
                // Append loop frames
                if let Some(loop_ids) = self.loops.get(&id) {
                    for loop_id in loop_ids {
                        if let Some(instr) = self.map.get(loop_id) {
                            frames.extend(instr.next(cache, self));
                        }
                    }
                    // Cache the result
                    *cache.map.get_mut(&id).unwrap() = frames;
                }
                cache.map.get(&id).unwrap()
            } else {
                &cache.default_frames
            }
        }
    }
    pub fn queue_command(&mut self, app: RyvmApp) {
        if let Some(RyvmCommand::Drum {
            path: Some(path), ..
        }) = &app.command
        {
            self.sample_bank.load(path.clone(), true);
        }
        self.command_queue.push(app);
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
    fn process_command(&mut self, app: RyvmApp) {
        self.stop_recording_all();
        let name = app.name.clone().unwrap_or_default();
        if let Some(command) = app.command {
            match command {
                RyvmCommand::Quit => {}
                RyvmCommand::Output { name } => self.set_output(name),
                RyvmCommand::Tempo { tempo } => self.set_tempo(tempo),
                RyvmCommand::Number { num } => self.add(name, Instrument::Number(num)),
                RyvmCommand::Sine { input, voices } => {
                    let instr = Instrument::wave(&input, WaveForm::Sine)
                        .voices(voices.unwrap_or_else(|| self.default_voices_from(input)));
                    self.add(name, instr);
                }
                RyvmCommand::Square { input, voices } => {
                    let instr = Instrument::wave(&input, WaveForm::Square)
                        .voices(voices.unwrap_or_else(|| self.default_voices_from(input)));
                    self.add(name, instr);
                }
                RyvmCommand::Saw { input, voices } => {
                    let instr = Instrument::wave(&input, WaveForm::Saw)
                        .voices(voices.unwrap_or_else(|| self.default_voices_from(input)));
                    self.add(name, instr);
                }
                RyvmCommand::Triangle { input, voices } => {
                    let instr = Instrument::wave(&input, WaveForm::Triangle)
                        .voices(voices.unwrap_or_else(|| self.default_voices_from(input)));
                    self.add(name, instr);
                }
                RyvmCommand::Mixer { inputs } => self.add(
                    name,
                    Instrument::Mixer(inputs.into_iter().zip(repeat(Balance::default())).collect()),
                ),
                #[cfg(feature = "keyboard")]
                RyvmCommand::Keyboard { octave } => self.add(
                    name.clone(),
                    Instrument::Keyboard(Keyboard::new(&name, octave.unwrap_or(4))),
                ),
                RyvmCommand::Drums => {
                    self.add(name.clone(), Instrument::DrumMachine(Vec::new()));
                    self.last_drums = Some(name);
                }
                RyvmCommand::Drum {
                    index,
                    path,
                    beat,
                    remove,
                } => {
                    let name = if let Some(name) = app.name {
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
                            samplings[index].beat = be.parse().unwrap();
                        }
                        if remove {
                            samplings[index] = Sampling::default();
                        }
                    }
                }
                RyvmCommand::Loop { input, measures } => self.add_loop(app.name, input, measures),
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
            }
        } else if let Some(instr) = self.get_mut_skip_loops(name) {
            match instr {
                Instrument::Number(num) => {
                    if let Some(n) = app
                        .inputs
                        .into_iter()
                        .next()
                        .and_then(|input| input.parse::<f32>().ok())
                    {
                        *num = n
                    }
                }
                Instrument::Mixer(inputs) => {
                    for input in app.inputs {
                        let balance = inputs.entry(input).or_insert_with(Balance::default);
                        if let Some(volume) = app.volume {
                            balance.volume = volume;
                        }
                        if let Some(pan) = app.pan {
                            balance.pan = pan;
                        }
                    }
                    for input in app.remove {
                        inputs.remove(&input);
                    }
                }
                Instrument::Wave { input, .. } => {
                    if let Some(new_input) = app.inputs.into_iter().next() {
                        *input = new_input;
                    }
                }
                #[cfg(feature = "keyboard")]
                Instrument::Keyboard(keyboard) => {
                    if let Some(octave) = app.octave {
                        keyboard.set_base_octave(octave);
                    }
                }
                Instrument::Loop { .. } => unreachable!(),
                _ => {}
            }
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
            for app in commands {
                self.process_command(app);
            }
        }
        // Init cache
        let mut cache = FrameCache {
            map: HashMap::new(),
            visited: HashSet::new(),
            default_frames: Default::default(),
        };
        // Get next sample
        self.sample_queue
            .take()
            .map(|samp| {
                self.i += 1;
                samp
            })
            .or_else(|| {
                if let Some(output_id) = &self.output {
                    let frames = self.next_from(output_id, &mut cache);
                    let next_frame: Vec<(Voice, Balance)> = frames
                        .iter()
                        .map(|frame| (frame.first, Balance::default()))
                        .collect();
                    if let Some(frame) = mix(&next_frame) {
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
