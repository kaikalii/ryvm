use std::{
    collections::{HashMap, HashSet},
    iter::repeat,
    mem::swap,
};

use indexmap::IndexMap;
use rodio::Source;
use serde_derive::{Deserialize, Serialize};

#[cfg(feature = "keyboard")]
use crate::Keyboard;
use crate::{
    Balance, CloneLock, Frame, FrameCache, InstrId, Instrument, LoopFrame, RyvmApp, RyvmCommand,
    SampleBank, SampleType, Sampling, SourceLock, WaveForm, SAMPLE_EPSILON, SAMPLE_RATE,
};

fn default_tempo() -> SampleType {
    120.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_tempo(tempo: &SampleType) -> bool {
    (tempo - default_tempo()).abs() < SAMPLE_EPSILON
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
    pub(crate) sample_bank: SampleBank,
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
    pub fn add_loop<I>(&mut self, id: I, measures: u8)
    where
        I: Into<InstrId>,
    {
        // Create new input id
        let id = id.into();
        let input_id = format!("{}-input", id);
        // Create the loop instrument
        let frame_count = self.frames_per_measure() as usize * measures as usize;
        let loop_instr = Instrument::Loop {
            input: input_id.clone(),
            measures,
            recording: true,
            frames: CloneLock::new(vec![
                LoopFrame {
                    frame: None,
                    new: true,
                };
                frame_count
            ]),
        };
        // Stop recording on all other loops
        for instr in self.map.values_mut() {
            if let Instrument::Loop { recording, .. } = instr {
                *recording = false;
            }
        }
        // Remove the input
        let input_instr = self.map.remove(&input_id).or_else(|| self.map.remove(&id));
        // Insert the loop
        self.map.insert(id, loop_instr);
        // Insert the input
        if let Some(instr) = input_instr {
            self.add(input_id, instr);
        }
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
    pub(crate) fn next_from<I>(&self, id: I, cache: &mut FrameCache) -> Option<Frame>
    where
        I: Into<InstrId>,
    {
        let id = id.into();
        if let Some(voice) = cache.map.get(&id) {
            Some(voice.clone())
        } else if cache.visited.contains(&id) {
            None
        } else {
            cache.visited.insert(id.clone());
            if let Some(voice) = self.map.get(&id).and_then(|instr| instr.next(cache, self)) {
                cache.map.insert(id, voice.clone());
                Some(voice)
            } else {
                None
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
                RyvmCommand::Keyboard { base_octave } => self.add(
                    name.clone(),
                    Instrument::Keyboard(Keyboard::new(&name, base_octave.unwrap_or(4))),
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
                RyvmCommand::Loop { input, measures } => self.add_loop(input, measures),
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
                    if let Some(frame) = self.next_from(output_id, &mut cache) {
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
