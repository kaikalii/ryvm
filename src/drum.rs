use std::{
    convert::Infallible,
    env::{current_dir, current_exe},
    fmt, fs,
    path::Path,
    str::FromStr,
};

use find_folder::Search;
use itertools::Itertools;
use rodio::{Decoder, Source};

use crate::{Frame, RyvmResult, Voice};

#[derive(Debug, Clone, Copy)]
pub struct ActiveSampling {
    pub index: usize,
    pub i: Frame,
    pub velocity: f32,
}

/// Data for an audio sample
#[derive(Clone)]
pub(crate) struct Sample {
    sample_rate: u32,
    samples: Vec<Voice>,
}

impl Default for Sample {
    fn default() -> Self {
        Sample {
            sample_rate: 1,
            samples: Vec::new(),
        }
    }
}

impl Sample {
    pub fn open<P>(path: P) -> RyvmResult<Self>
    where
        P: AsRef<Path>,
    {
        let search = Search::KidsThenParents(2, 1);
        let path = path.as_ref().to_string_lossy();
        let path = search
            .of(current_dir()?)
            .for_folder(&path)
            .or_else(|_| search.of(current_exe()?).for_folder(&path))?;
        let decoder = Decoder::new(fs::File::open(path)?)?;
        let sample_rate = decoder.sample_rate();
        let channels = decoder.channels();
        let samples = if channels == 1 {
            decoder
                .map(|i| Voice::mono(f32::from(i) / f32::from(i16::MAX)))
                .collect()
        } else {
            decoder
                .chunks(channels as usize)
                .into_iter()
                .map(|mut lr| {
                    Voice::stereo(
                        f32::from(lr.next().unwrap_or(0)) / f32::from(i16::MAX),
                        f32::from(lr.next().unwrap_or(0)) / f32::from(i16::MAX),
                    )
                })
                .collect()
        };
        Ok(Sample {
            sample_rate,
            samples,
        })
    }
    pub fn voice(&self, index: Frame, sample_rate: u32) -> &Voice {
        let adjusted = index as usize * self.sample_rate as usize / sample_rate as usize;
        &self.samples[adjusted as usize]
    }
    pub fn len(&self, sample_rate: u32) -> Frame {
        (u64::from(sample_rate) * self.samples.len() as Frame / Frame::from(self.sample_rate))
            as Frame
    }
}

#[derive(Clone, PartialEq, Eq, Default)]
pub struct BeatPattern(pub Vec<bool>);

impl fmt::Debug for BeatPattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl FromStr for BeatPattern {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(BeatPattern(s.chars().map(|c| c == '-').collect()))
    }
}

impl fmt::Display for BeatPattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s: String = self.0.iter().map(|b| if *b { '-' } else { '_' }).collect();
        write!(f, "{}", s)
    }
}
