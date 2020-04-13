use std::{
    collections::HashMap,
    f32::consts::PI,
    ops::{Index, IndexMut},
    sync::Arc,
};

use crossbeam::sync::ShardedLock;
use once_cell::sync::Lazy;
use rodio::{Sample, Source};

pub type SampleType = f32;
pub type InstrId = String;
pub type InstrIdRef<'a> = &'a str;

/// The global sample rate
pub const SAMPLE_RATE: u32 = 32000;

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

/// An instrument for producing sounds
#[derive(Debug, Clone)]
pub enum Instrument {
    Sine { f: SampleType, i: u32 },
    Square { f: SampleType, i: u32 },
    Mixer(Vec<Balanced<InstrId>>),
}

impl Instrument {
    pub fn sine(f: SampleType) -> Self {
        Instrument::Sine { f, i: 0 }
    }
    pub fn square(f: SampleType) -> Self {
        Instrument::Square { f, i: 0 }
    }
    pub fn next(&mut self, instruments: &Instruments) -> Option<(SampleType, SampleType)> {
        match self {
            Instrument::Sine { f, i } => {
                let s = SINE_SAMPLES[*i as usize];
                *i = (*i + *f as u32) % SAMPLE_RATE;
                Some((s, s))
            }
            Instrument::Square { f, i } => {
                let samples_per_cycle = (SAMPLE_RATE as SampleType / *f) as u32;
                let s = if *i < samples_per_cycle / 2 {
                    1.0
                } else {
                    -1.0
                } * 0.6;
                *i = (*i + 1) % samples_per_cycle as u32;
                Some((s, s))
            }
            Instrument::Mixer(list) => {
                let (left_vol_sum, right_vol_sum) =
                    list.iter().fold((0.0, 0.0), |(lacc, racc), instr| {
                        let (l, r) = instr.stereo_volume();
                        (lacc + l, racc + r)
                    });
                let (left_sum, right_sum) =
                    list.iter_mut().fold((0.0, 0.0), |(lacc, racc), bal| {
                        let (l, r) = bal.stereo_volume();
                        let id = &bal.instr;
                        let instr_next = instruments
                            .map
                            .get(id)
                            .and_then(|instr| instr.write().unwrap().next(instruments));
                        if let Some((ls, rs)) = instr_next {
                            (lacc + ls * l, racc + rs * r)
                        } else {
                            (lacc, racc)
                        }
                    });
                Some((left_sum / left_vol_sum, right_sum / right_vol_sum))
            }
        }
    }
    pub fn set_freq(&mut self, freq: SampleType) {
        match self {
            Instrument::Sine { f, .. } => *f = freq,
            Instrument::Square { f, .. } => *f = freq,
            _ => {}
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
    map: HashMap<InstrId, ShardedLock<Instrument>>,
    queue: Option<SampleType>,
    i: u32,
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
        self.map.insert(id.into(), ShardedLock::new(instr));
    }
}

impl Iterator for Instruments {
    type Item = SampleType;
    fn next(&mut self) -> Option<Self::Item> {
        self.queue
            .take()
            .map(|sample| {
                self.i += 1;
                sample
            })
            .or_else(|| {
                if let Some(output_id) = &self.output {
                    if let Some(output_instr) = self.map.get(output_id) {
                        if let Some((left, right)) = output_instr.write().unwrap().next(self) {
                            self.queue = Some(right);
                            return Some(left);
                        }
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
