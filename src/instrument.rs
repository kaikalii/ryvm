use std::{
    collections::{HashMap, HashSet},
    f32::consts::PI,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use crossbeam::sync::ShardedLock;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use rodio::Source;
use serde_derive::{Deserialize, Serialize};

pub type SampleType = f32;
pub type InstrId = String;
pub type InstrIdRef<'a> = &'a str;

/// The global sample rate
pub const SAMPLE_RATE: u32 = 44100 / 3;

/// An error bound for the sample type
pub const SAMPLE_EPSILON: SampleType = std::f32::EPSILON;

const ORDERING: Ordering = Ordering::Relaxed;

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
pub struct Frame {
    pub left: SampleType,
    pub right: SampleType,
    pub velocity: SampleType,
}

impl Frame {
    pub fn stereo(left: SampleType, right: SampleType) -> Self {
        Frame {
            left,
            right,
            velocity: 1.0,
        }
    }
    pub fn mono(both: SampleType) -> Self {
        Frame::stereo(both, both)
    }
    pub fn velocity(self, velocity: SampleType) -> Self {
        Frame { velocity, ..self }
    }
}

struct FrameCache {
    map: HashMap<InstrId, Frame>,
    visited: HashSet<InstrId>,
    tempo: SampleType,
}

/// An instrument for producing sounds
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Instrument {
    Number(SampleType),
    Sine {
        input: InstrId,
        #[serde(skip)]
        li: AtomicU32,
        #[serde(skip)]
        ri: AtomicU32,
    },
    Square {
        input: InstrId,
        #[serde(skip)]
        li: AtomicU32,
        #[serde(skip)]
        ri: AtomicU32,
    },
    Mixer(HashMap<InstrId, Balance>),
}

impl Instrument {
    pub fn sine<I>(id: I) -> Self
    where
        I: Into<InstrId>,
    {
        Instrument::Sine {
            input: id.into(),
            li: AtomicU32::new(0),
            ri: AtomicU32::new(0),
        }
    }
    pub fn square<I>(id: I) -> Self
    where
        I: Into<InstrId>,
    {
        Instrument::Square {
            input: id.into(),
            li: AtomicU32::new(0),
            ri: AtomicU32::new(0),
        }
    }
    fn next(&self, cache: &mut FrameCache, instruments: &Instruments) -> Option<Frame> {
        match self {
            Instrument::Number(n) => Some(Frame::mono(*n)),
            Instrument::Sine { input, li, ri } => {
                let frame = instruments.next_from(&*input, cache).unwrap_or_default();
                let lix = li.load(ORDERING);
                let rix = ri.load(ORDERING);
                let ls = SINE_SAMPLES[lix as usize];
                let rs = SINE_SAMPLES[rix as usize];
                li.store((lix + frame.left as u32) % SAMPLE_RATE, ORDERING);
                ri.store((rix + frame.right as u32) % SAMPLE_RATE, ORDERING);
                Some(Frame::stereo(ls, rs))
            }
            Instrument::Square { input, li, ri } => {
                let frame = instruments.next_from(&*input, cache).unwrap_or_default();
                // spc = samples per cycles
                let lspc = (SAMPLE_RATE as SampleType / frame.left) as u32;
                let rspc = (SAMPLE_RATE as SampleType / frame.right) as u32;
                let lix = li.load(ORDERING);
                let rix = ri.load(ORDERING);
                let ls = if lix < lspc / 2 { 1.0 } else { -1.0 } * 0.6;
                li.store((lix + 1) % lspc as u32, ORDERING);
                let rs = if rix < rspc / 2 { 1.0 } else { -1.0 } * 0.6;
                ri.store((rix + 1) % rspc as u32, ORDERING);
                Some(Frame::stereo(ls, rs))
            }
            Instrument::Mixer(list) => {
                let (left_vol_sum, right_vol_sum) =
                    list.values().fold((0.0, 0.0), |(lacc, racc), bal| {
                        let (l, r) = bal.stereo_volume();
                        (lacc + l, racc + r)
                    });
                let (left_sum, right_sum) =
                    list.iter().fold((0.0, 0.0), |(lacc, racc), (id, bal)| {
                        let (l, r) = bal.stereo_volume();
                        if let Some(frame) = instruments.next_from(id, cache) {
                            (
                                lacc + frame.left * l * frame.velocity,
                                racc + frame.right * r * frame.velocity,
                            )
                        } else {
                            (lacc, racc)
                        }
                    });
                Some(Frame::stereo(
                    left_sum / left_vol_sum,
                    right_sum / right_vol_sum,
                ))
            }
        }
    }
    pub fn set(&mut self, num: SampleType) {
        if let Instrument::Number(n) = self {
            *n = num;
        }
    }
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
    fn next_from<I>(&self, id: I, cache: &mut FrameCache) -> Option<Frame>
    where
        I: Into<InstrId>,
    {
        let id = id.into();
        if let Some(frame) = cache.map.get(&id) {
            Some(*frame)
        } else if cache.visited.contains(&id) {
            None
        } else {
            cache.visited.insert(id.clone());
            if let Some(frame) = self.map.get(&id).and_then(|instr| instr.next(cache, self)) {
                cache.map.insert(id, frame);
                Some(frame)
            } else {
                None
            }
        }
    }
}

impl Iterator for Instruments {
    type Item = SampleType;
    fn next(&mut self) -> Option<Self::Item> {
        let mut cache = FrameCache {
            map: HashMap::new(),
            visited: HashSet::new(),
            tempo: self.tempo,
        };
        self.queue.take().or_else(|| {
            if let Some(output_id) = &self.output {
                if let Some(frame) = self.next_from(output_id, &mut cache) {
                    self.queue = Some(frame.right);
                    return Some(frame.left);
                }
            }
            self.queue = Some(0.0);
            Some(0.0)
        })
    }
}
