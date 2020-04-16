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
    AddApp, Balance, Frame, FrameCache, InstrId, Instrument, RyvmApp, SampleType, Sampling,
    SourceLock, WaveForm, MAX_BEATS, SAMPLE_EPSILON, SAMPLE_RATE,
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
        }
    }
}

impl Instruments {
    pub fn new() -> SourceLock<Self> {
        SourceLock::new(Self::default())
    }
    pub fn samples_per_measure(&self) -> SampleType {
        SAMPLE_RATE as SampleType / (self.tempo / 60.0) * 4.0
    }
    pub fn i(&self) -> u32 {
        self.i
    }
    pub fn measure_i(&self) -> u32 {
        self.i % self.samples_per_measure() as u32
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
    pub fn get_mut<I>(&mut self, id: I) -> Option<&mut Instrument>
    where
        I: Into<InstrId>,
    {
        self.map.get_mut(&id.into())
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
        self.command_queue.push(app);
    }
    fn process_command(&mut self, app: RyvmApp) {
        match app {
            RyvmApp::Quit => {}
            RyvmApp::Output { name } => self.set_output(name),
            RyvmApp::Tempo { tempo } => self.set_tempo(tempo),
            RyvmApp::Add { name, app } => self.add(
                name.clone(),
                match app {
                    AddApp::Number { num } => Instrument::Number(num),
                    AddApp::Sine { input, voices } => {
                        let mut instr = Instrument::wave(input, WaveForm::Sine);
                        if let Some(voices) = voices {
                            instr = instr.voices(voices);
                        }
                        instr
                    }
                    AddApp::Square { input, voices } => {
                        let mut instr = Instrument::wave(input, WaveForm::Square);
                        if let Some(voices) = voices {
                            instr = instr.voices(voices);
                        }
                        instr
                    }
                    AddApp::Mixer { inputs } => Instrument::Mixer(
                        inputs.into_iter().zip(repeat(Balance::default())).collect(),
                    ),
                    #[cfg(feature = "keyboard")]
                    AddApp::Keyboard { base_octave } => {
                        Instrument::Keyboard(Keyboard::new(&name, base_octave.unwrap_or(4)))
                    }
                    AddApp::Drums => Instrument::DrumMachine(Vec::new()),
                },
            ),
            RyvmApp::Edit {
                name,
                set,
                inputs,
                volume,
                pan,
            } => {
                if let Some(instr) = self.get_mut(name) {
                    match instr {
                        Instrument::Number(n) => {
                            if let Some(num) = set {
                                *n = num;
                            }
                        }
                        Instrument::Wave { input, .. } => {
                            if let Some(new_input) = inputs.into_iter().next() {
                                *input = new_input;
                            }
                        }
                        Instrument::Mixer(map) => {
                            if let Some(volume) = volume {
                                for id in &inputs {
                                    map.entry(id.clone())
                                        .or_insert_with(Balance::default)
                                        .volume = volume;
                                }
                            }
                            if let Some(pan) = pan {
                                for id in &inputs {
                                    map.entry(id.clone()).or_insert_with(Balance::default).pan =
                                        pan;
                                }
                            }
                            for input in inputs {
                                map.entry(input).or_insert_with(Balance::default);
                            }
                        }
                        #[cfg(feature = "keyboard")]
                        Instrument::Keyboard(_) => {}
                        Instrument::DrumMachine { .. } => {}
                    }
                }
            }
            RyvmApp::Drum {
                machine,
                index,
                path,
                beat,
            } => {
                if let Some(Instrument::DrumMachine(samplings)) = self.get_mut(machine) {
                    samplings.resize(index + 1, Sampling::default());
                    if let Some(path) = path {
                        if let Err(e) = samplings[index].sample.set_path(path) {
                            println!("{}", e);
                        }
                    }
                    if let Some(be) = beat {
                        samplings[index].beat = be.parse().unwrap();
                    }
                }
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
