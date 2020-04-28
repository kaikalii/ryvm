use std::{f32::consts::FRAC_2_PI, fmt, str::FromStr, sync::Arc};

use crate::CloneLock;

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
    Noise,
}

impl WaveForm {
    pub const MIN_ENERGY: f32 = 0.5;
    pub fn energy(self) -> f32 {
        match self {
            WaveForm::Sine => FRAC_2_PI,
            WaveForm::Square => 1.0,
            WaveForm::Saw => 0.5,
            WaveForm::Triangle => 0.5,
            WaveForm::Noise => 0.5,
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
            "noise" => WaveForm::Noise,
            _ => return Err(format!("Unknown waveform {:?}", s)),
        })
    }
}
