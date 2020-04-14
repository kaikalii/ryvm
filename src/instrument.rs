use std::{
    collections::HashMap,
    f32::consts::PI,
    ops::{Index, IndexMut},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, RwLock,
    },
};

use crossbeam::sync::ShardedLock;
use once_cell::sync::Lazy;
use rodio::{Sample, Source};

pub type SampleType = f32;
pub type InstrId = String;
pub type InstrIdRef<'a> = &'a str;

/// The global sample rate
pub const SAMPLE_RATE: u32 = 32000;

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

impl<T> Source for SourceLock<T>
where
    T: Source,
    T::Item: Sample,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.get(|inner| inner.current_frame_len())
    }
    fn channels(&self) -> u16 {
        self.get(|inner| inner.channels())
    }
    fn sample_rate(&self) -> u32 {
        self.get(|inner| inner.sample_rate())
    }
    fn total_duration(&self) -> std::option::Option<std::time::Duration> {
        self.get(|inner| inner.total_duration())
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

type FrameCache = HashMap<InstrId, Frame>;

/// An instrument for producing sounds
#[derive(Debug)]
pub enum Instrument {
    Number(SampleType),
    Sine {
        freq: InstrId,
        li: AtomicU32,
        ri: AtomicU32,
    },
    Square {
        freq: InstrId,
        li: AtomicU32,
        ri: AtomicU32,
    },
    Mixer(Vec<Balanced<InstrId>>),
}

impl Instrument {
    pub fn sine<I>(id: I) -> Self
    where
        I: Into<InstrId>,
    {
        Instrument::Sine {
            freq: id.into(),
            li: AtomicU32::new(0),
            ri: AtomicU32::new(0),
        }
    }
    pub fn square<I>(id: I) -> Self
    where
        I: Into<InstrId>,
    {
        Instrument::Square {
            freq: id.into(),
            li: AtomicU32::new(0),
            ri: AtomicU32::new(0),
        }
    }
    pub fn next(&self, cache: &mut FrameCache, instruments: &Instruments) -> Option<Frame> {
        match self {
            Instrument::Number(n) => Some(Frame::mono(*n)),
            Instrument::Sine { freq, li, ri } => {
                let frame = instruments.next_from(&*freq, cache).unwrap_or_default();
                let lix = li.load(ORDERING);
                let rix = ri.load(ORDERING);
                let ls = SINE_SAMPLES[lix as usize];
                let rs = SINE_SAMPLES[rix as usize];
                li.store((lix + frame.left as u32) % SAMPLE_RATE, ORDERING);
                ri.store((rix + frame.right as u32) % SAMPLE_RATE, ORDERING);
                Some(Frame::stereo(ls, rs))
            }
            Instrument::Square { freq, li, ri } => {
                let frame = instruments.next_from(&*freq, cache).unwrap_or_default();
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
                    list.iter().fold((0.0, 0.0), |(lacc, racc), bal| {
                        let (l, r) = bal.stereo_volume();
                        (lacc + l, racc + r)
                    });
                let (left_sum, right_sum) = list.iter().fold((0.0, 0.0), |(lacc, racc), bal| {
                    let (l, r) = bal.stereo_volume();
                    let id = &bal.instr;
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

impl Index<usize> for Instrument {
    type Output = Balanced<InstrId>;
    fn index(&self, i: usize) -> &Self::Output {
        if let Instrument::Mixer(list) = self {
            &list[i]
        } else {
            panic!("Attempted to index non-mixer instrument")
        }
    }
}

impl IndexMut<usize> for Instrument {
    fn index_mut(&mut self, i: usize) -> &mut Self::Output {
        if let Instrument::Mixer(list) = self {
            &mut list[i]
        } else {
            panic!("Attempted to index non-mixer instrument")
        }
    }
}

/// A balance wrapper for an `Instrument`
#[derive(Debug, Clone)]
pub struct Balanced<T> {
    pub instr: T,
    pub volume: SampleType,
    pub pan: SampleType,
}

impl<T> From<T> for Balanced<T> {
    fn from(instr: T) -> Self {
        Balanced {
            instr,
            volume: 1.0,
            pan: 0.0,
        }
    }
}

impl<T> Balanced<T> {
    pub fn stereo_volume(&self) -> (SampleType, SampleType) {
        (
            self.volume * (1.0 - self.pan.max(0.0)),
            self.volume * (1.0 + self.pan.min(0.0)),
        )
    }
    pub fn volume(self, volume: SampleType) -> Self {
        Balanced { volume, ..self }
    }
    pub fn pan(self, pan: SampleType) -> Self {
        Balanced { pan, ..self }
    }
}

#[derive(Debug, Default)]
pub struct Instruments {
    output: Option<InstrId>,
    map: HashMap<InstrId, Instrument>,
    queue: Option<SampleType>,
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
    pub fn add<I>(&mut self, id: I, instr: Instrument)
    where
        I: Into<InstrId>,
    {
        self.map.insert(id.into(), instr);
    }
    fn next_from<I>(&self, id: I, cache: &mut FrameCache) -> Option<Frame>
    where
        I: Into<InstrId>,
    {
        let id = id.into();
        if let Some(frame) = cache.get(&id) {
            Some(*frame)
        } else if let Some(frame) = self.map.get(&id).and_then(|instr| instr.next(cache, self)) {
            cache.insert(id, frame);
            Some(frame)
        } else {
            None
        }
    }
}

impl Iterator for Instruments {
    type Item = SampleType;
    fn next(&mut self) -> Option<Self::Item> {
        let mut cache = FrameCache::new();
        self.queue.take().or_else(|| {
            if let Some(output_id) = &self.output {
                if let Some(frame) = self.next_from(output_id, &mut cache) {
                    self.queue = Some(frame.right);
                    return Some(frame.left);
                }
            }
            None
        })
    }
}

impl Source for Instruments {
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
