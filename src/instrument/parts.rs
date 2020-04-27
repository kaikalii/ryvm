use std::{f32::consts::FRAC_2_PI, fmt, str::FromStr, sync::Arc};

use crate::{CloneLock, SampleType, DEFAULT_VOLUME};

/// A lock used primarily to allow the manipulation of a rodio::Source
/// while it is already playing
#[derive(Debug)]
pub struct SourceLock<T>(Arc<CloneLock<T>>);

impl<T> Clone for SourceLock<T> {
    fn clone(&self) -> Self {
        SourceLock(Arc::clone(&self.0))
    }
}

impl<T> SourceLock<T> {
    pub fn new(inner: T) -> Self {
        SourceLock(Arc::new(CloneLock::new(inner)))
    }
    pub fn update<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        f(&mut *self.0.lock())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveForm {
    Sine,
    Square,
    Saw,
    Triangle,
}

impl WaveForm {
    pub const MIN_ENERGY: SampleType = 0.5;
    pub fn energy(self) -> SampleType {
        match self {
            WaveForm::Sine => FRAC_2_PI,
            WaveForm::Square => 1.0,
            WaveForm::Saw => 0.5,
            WaveForm::Triangle => 0.5,
        }
    }
}

impl fmt::Display for WaveForm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", format!("{:?}", self))
    }
}

impl FromStr for WaveForm {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "square" | "sq" => WaveForm::Square,
            "saw" => WaveForm::Saw,
            "triangle" | "tri" => WaveForm::Triangle,
            "sine" | "sin" => WaveForm::Sine,
            _ => return Err(format!("Unknown waveform {:?}", s)),
        })
    }
}

/// A balance wrapper for an `Instrument`
#[derive(Debug, Clone, Copy)]
pub struct Balance {
    pub volume: SampleType,
    pub pan: SampleType,
}

impl Default for Balance {
    fn default() -> Self {
        Balance {
            volume: DEFAULT_VOLUME,
            pan: 0.0,
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
}

#[derive(Debug, Clone, Copy)]
pub struct ActiveSampling {
    pub index: usize,
    pub i: usize,
    pub velocity: SampleType,
}

#[derive(Debug, Clone, Copy)]
pub struct LoopMaster {
    pub id: u8,
    pub start_i: u32,
    pub period: u32,
}
