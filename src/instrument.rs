use std::{
    collections::{HashMap, HashSet},
    error::Error,
    f32::consts::PI,
    fs,
    iter::{once, repeat},
    path::{Path, PathBuf},
    sync::Arc,
};

use crossbeam::sync::ShardedLock;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use rodio::{Decoder, Source};
use serde_derive::{Deserialize, Serialize};

use crate::U32Lock;
#[cfg(feature = "keyboard")]
use crate::{freq, Keyboard};

pub type SampleType = f32;
pub type InstrId = String;
pub type InstrIdRef<'a> = &'a str;

/// The global sample rate
pub const SAMPLE_RATE: u32 = 44100 / 3;

/// An error bound for the sample type
pub const SAMPLE_EPSILON: SampleType = std::f32::EPSILON;

static SINE_SAMPLES: Lazy<Vec<SampleType>> = Lazy::new(|| {
    (0..SAMPLE_RATE)
        .map(|i| (i as SampleType / SAMPLE_RATE as SampleType * 2.0 * PI).sin())
        .collect()
});

#[derive(Debug)]
pub struct SourceLock<T>(Arc<ShardedLock<T>>);

impl<T> Clone for SourceLock<T> {
    fn clone(&self) -> Self {
        SourceLock(Arc::clone(&self.0))
    }
}

impl<T> SourceLock<T> {
    pub fn new(inner: T) -> Self {
        SourceLock(Arc::new(ShardedLock::new(inner)))
    }
    pub fn get<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        f(&*self.0.read().unwrap())
    }
    pub fn update<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        f(&mut *self.0.write().unwrap())
    }
}

impl<T> Iterator for SourceLock<T>
where
    T: Iterator,
{
    type Item = T::Item;
    fn next(&mut self) -> Option<Self::Item> {
        self.update(Iterator::next)
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

#[derive(Debug, Clone, Copy, Default)]
pub struct Voice {
    pub left: SampleType,
    pub right: SampleType,
    pub velocity: SampleType,
}

#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub first: Voice,
    pub extra: Vec<Voice>,
}

impl Voice {
    pub fn stereo(left: SampleType, right: SampleType) -> Self {
        Voice {
            left,
            right,
            velocity: 1.0,
        }
    }
    pub fn mono(both: SampleType) -> Self {
        Voice::stereo(both, both)
    }
    pub fn velocity(self, velocity: SampleType) -> Self {
        Voice { velocity, ..self }
    }
}

impl Frame {
    pub fn multi<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Voice>,
    {
        let mut iter = iter.into_iter();
        let mut frame = Frame {
            first: Voice::default(),
            extra: Vec::new(),
        };
        if let Some(first) = iter.next() {
            frame.first = first;
        }
        frame.extra.extend(iter);
        frame
    }
    pub fn iter(&self) -> impl Iterator<Item = Voice> + '_ {
        once(self.first).chain(self.extra.iter().copied())
    }
}

impl IntoIterator for Frame {
    type Item = Voice;
    type IntoIter = Box<dyn Iterator<Item = Voice>>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(once(self.first).chain(self.extra.into_iter()))
    }
}

impl From<Voice> for Frame {
    fn from(voice: Voice) -> Self {
        Frame {
            first: voice,
            extra: Vec::new(),
        }
    }
}

struct VoiceCache {
    map: HashMap<InstrId, Frame>,
    visited: HashSet<InstrId>,
    #[allow(dead_code)]
    tempo: SampleType,
}

fn default_voices() -> u32 {
    1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_voices(v: &u32) -> bool {
    v == &default_voices()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaveForm {
    Sine,
    Square,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub path: PathBuf,
    #[serde(skip)]
    pub samples: Vec<SampleType>,
}

impl Sample {
    pub fn open<P>(path: P) -> Result<Self, Box<dyn Error>>
    where
        P: AsRef<Path>,
    {
        let mut sample = Sample {
            path: path.as_ref().into(),
            samples: Vec::new(),
        };
        sample.init()?;
        Ok(sample)
    }
    pub fn init(&mut self) -> Result<(), Box<dyn Error>> {
        if let Ok(decoder) = Decoder::new(fs::File::open(&self.path)?) {
            self.samples = decoder
                .map(|i| i as SampleType / std::i16::MAX as SampleType)
                .collect();
        }
        Ok(())
    }
}

/// An instrument for producing sounds
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Instrument {
    Number(SampleType),
    Wave {
        input: InstrId,
        form: WaveForm,
        #[serde(default = "default_voices", skip_serializing_if = "is_default_voices")]
        voices: u32,
        #[serde(skip)]
        waves: Vec<U32Lock>,
    },
    Mixer(HashMap<InstrId, Balance>),
    #[cfg(feature = "keyboard")]
    Keyboard(Keyboard),
}

impl Instrument {
    #[allow(clippy::single_match)]
    pub fn init(&mut self) {
        match self {
            Instrument::Wave { voices, waves, .. } => {
                waves.resize(*voices as usize, U32Lock::new(0));
            }
            _ => {}
        }
    }
    pub fn wave<I>(id: I, form: WaveForm) -> Self
    where
        I: Into<InstrId>,
    {
        Instrument::Wave {
            input: id.into(),
            form,
            voices: default_voices(),
            waves: vec![U32Lock::new(0)],
        }
    }
    pub fn voices(mut self, v: u32) -> Self {
        if let Instrument::Wave { voices, waves, .. } = &mut self {
            *voices = v;
            waves.resize(v as usize, U32Lock::new(0));
        }
        self
    }
    fn next(&self, cache: &mut VoiceCache, instruments: &Instruments) -> Option<Frame> {
        match self {
            Instrument::Number(n) => Some(Voice::mono(*n).into()),
            Instrument::Wave {
                input, form, waves, ..
            } => {
                let frame = instruments.next_from(&*input, cache).unwrap_or_default();
                let mix_inputs: Vec<(Voice, Balance)> = frame
                    .iter()
                    .zip(waves.iter())
                    .map(|(voice, i)| {
                        let ix = i.load();
                        let s = match form {
                            WaveForm::Sine => {
                                let s = SINE_SAMPLES[ix as usize];
                                i.store((ix + voice.left as u32) % SAMPLE_RATE);
                                s
                            }
                            WaveForm::Square => {
                                if voice.left != 0.0 {
                                    let spc = (SAMPLE_RATE as SampleType / voice.left) as u32;
                                    let s = if ix < spc / 2 { 1.0 } else { -1.0 } * 0.6;
                                    i.store((ix + 1) % spc as u32);
                                    s
                                } else {
                                    0.0
                                }
                            }
                        };
                        Voice::mono(s)
                    })
                    .zip(repeat(Balance::default()))
                    .collect();
                Some(mix(&mix_inputs))
            }
            Instrument::Mixer(list) => {
                let next_frame: Vec<(Voice, Balance)> = list
                    .iter()
                    .map(|(id, bal)| {
                        if let Some(frame) = instruments.next_from(id, cache) {
                            (frame.first, *bal)
                        } else {
                            (Voice::default(), Balance::default())
                        }
                    })
                    .collect();
                Some(mix(&next_frame))
            }
            #[cfg(feature = "keyboard")]
            Instrument::Keyboard(keyboard) => {
                let freqs: Vec<SampleType> = keyboard
                    .pressed(|set| set.iter().map(|&(letter, oct)| freq(letter, oct)).collect());
                Some(Frame::multi(freqs.into_iter().map(Voice::mono)))
            }
        }
    }
    pub fn set(&mut self, num: SampleType) {
        if let Instrument::Number(n) = self {
            *n = num;
        }
    }
}

fn mix(list: &[(Voice, Balance)]) -> Frame {
    let (left_vol_sum, right_vol_sum) = list.iter().fold((0.0, 0.0), |(lacc, racc), (_, bal)| {
        let (l, r) = bal.stereo_volume();
        (lacc + l, racc + r)
    });
    let (left_sum, right_sum) = list.iter().fold((0.0, 0.0), |(lacc, racc), (voice, bal)| {
        let (l, r) = bal.stereo_volume();
        (
            lacc + voice.left * l * voice.velocity,
            racc + voice.right * r * voice.velocity,
        )
    });
    Voice::stereo(left_sum / left_vol_sum, right_sum / right_vol_sum).into()
}

fn default_volume() -> SampleType {
    1.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_volume(v: &SampleType) -> bool {
    (v - default_volume()).abs() < SAMPLE_EPSILON
}

fn default_pan() -> SampleType {
    0.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_pan(v: &SampleType) -> bool {
    (v - default_pan()).abs() < SAMPLE_EPSILON
}

/// A balance wrapper for an `Instrument`
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Balance {
    #[serde(default = "default_volume", skip_serializing_if = "is_default_volume")]
    pub volume: SampleType,
    #[serde(default = "default_pan", skip_serializing_if = "is_default_pan")]
    pub pan: SampleType,
}

impl Default for Balance {
    fn default() -> Self {
        Balance {
            volume: default_volume(),
            pan: default_pan(),
        }
    }
}

impl Balance {
    pub fn stereo_volume(self) -> (SampleType, SampleType) {
        (
            self.volume * (1.0 - self.pan.max(0.0)),
            self.volume * (1.0 + self.pan.min(0.0)),
        )
    }
    pub fn volume(self, volume: SampleType) -> Self {
        Balance { volume, ..self }
    }
    pub fn pan(self, pan: SampleType) -> Self {
        Balance { pan, ..self }
    }
}

fn default_tempo() -> SampleType {
    120.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_tempo(tempo: &SampleType) -> bool {
    (tempo - default_tempo()).abs() < SAMPLE_EPSILON
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Instruments {
    output: Option<InstrId>,
    #[serde(rename = "instruments")]
    map: IndexMap<InstrId, Instrument>,
    #[serde(skip)]
    queue: Option<SampleType>,
    #[serde(default = "default_tempo", skip_serializing_if = "is_default_tempo")]
    tempo: SampleType,
}

impl Instruments {
    pub fn new() -> SourceLock<Self> {
        SourceLock::new(Self::default())
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
    fn next_from<I>(&self, id: I, cache: &mut VoiceCache) -> Option<Frame>
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
}

impl Iterator for Instruments {
    type Item = SampleType;
    fn next(&mut self) -> Option<Self::Item> {
        let mut cache = VoiceCache {
            map: HashMap::new(),
            visited: HashSet::new(),
            tempo: self.tempo,
        };
        self.queue.take().or_else(|| {
            if let Some(output_id) = &self.output {
                if let Some(frame) = self.next_from(output_id, &mut cache) {
                    self.queue = Some(frame.first.right);
                    return Some(frame.first.left);
                }
            }
            self.queue = Some(0.0);
            Some(0.0)
        })
    }
}
