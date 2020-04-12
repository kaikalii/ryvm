use std::{
    f32::consts::PI,
    ops::{Index, IndexMut},
    sync::Arc,
};

use crossbeam::sync::ShardedLock;
use once_cell::sync::Lazy;
use rodio::Source;

pub type SampleType = f32;

/// The global sample rate
pub const SAMPLE_RATE: u32 = 32000;

static SINE_SAMPLES: Lazy<Vec<SampleType>> = Lazy::new(|| {
    (0..SAMPLE_RATE)
        .map(|i| (i as SampleType / SAMPLE_RATE as SampleType * 2.0 * PI).sin())
        .collect()
});

#[derive(Debug, Clone)]
pub struct SourceLock<T>(Arc<ShardedLock<T>>);

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

/// An instrument for producing sounds
#[derive(Debug, Clone)]
pub enum Instrument {
    Sine { f: SampleType, i: u32 },
    Square { f: SampleType, i: u32 },
    Mixer(Vec<Balanced>),
}

impl Instrument {
    pub fn balanced(self) -> Balanced {
        Balanced::from(self)
    }
    pub fn sine(f: SampleType) -> Self {
        Instrument::Sine { f, i: 0 }
    }
    pub fn square(f: SampleType) -> Self {
        Instrument::Square { f, i: 0 }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<(SampleType, SampleType)> {
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
                    list.iter_mut().fold((0.0, 0.0), |(lacc, racc), instr| {
                        let (l, r) = instr.stereo_volume();
                        if let Some((ls, rs)) = instr.instr.next() {
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
    pub fn source(self) -> (InstrumentSource, SourceLock<Instrument>) {
        let lock = SourceLock::new(self);
        let lock_clone = lock.clone();
        let source = InstrumentSource {
            inner: lock,
            queue: None,
            i: 0,
        };
        (source, lock_clone)
    }
}

impl Index<usize> for Instrument {
    type Output = Balanced;
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
pub struct Balanced {
    pub instr: Instrument,
    pub volume: SampleType,
    pub pan: SampleType,
}

impl From<Instrument> for Balanced {
    fn from(instr: Instrument) -> Self {
        Balanced {
            instr,
            volume: 1.0,
            pan: 0.0,
        }
    }
}

impl Balanced {
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

/// A wrapper around an `Instrument` to implement `rodio::Source`
pub struct InstrumentSource {
    inner: SourceLock<Instrument>,
    queue: Option<SampleType>,
    i: u32,
}

impl Iterator for InstrumentSource {
    type Item = SampleType;
    fn next(&mut self) -> Option<Self::Item> {
        self.queue
            .take()
            .map(|sample| {
                self.i += 1;
                sample
            })
            .or_else(|| {
                let queue = &mut self.queue;
                self.inner.update(|inner| {
                    inner.next().map(|(left, right)| {
                        *queue = Some(right);
                        left
                    })
                })
            })
    }
}

impl Source for InstrumentSource {
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
