use std::{
    convert::Infallible,
    env::{current_dir, current_exe},
    error::Error,
    fmt, fs,
    path::Path,
    str::FromStr,
};

use find_folder::Search;
use itertools::Itertools;
use rodio::{Decoder, Source};

use crate::{SampleType, Voice, SAMPLE_RATE};

/// Data for an audio sample
#[derive(Clone)]
pub struct Sample {
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
    pub fn open<P>(path: P) -> Result<Self, Box<dyn Error>>
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
                .map(|i| Voice::mono(i as SampleType / std::i16::MAX as SampleType))
                .collect()
        } else {
            decoder
                .chunks(channels as usize)
                .into_iter()
                .map(|mut lr| {
                    Voice::stereo(
                        lr.next().unwrap_or(0) as SampleType / std::i16::MAX as SampleType,
                        lr.next().unwrap_or(0) as SampleType / std::i16::MAX as SampleType,
                    )
                })
                .collect()
        };
        Ok(Sample {
            sample_rate,
            samples,
        })
    }
    pub fn voice(&self, index: u32) -> &Voice {
        let adjusted = index as usize * self.sample_rate as usize / SAMPLE_RATE as usize;
        &self.samples[adjusted as usize]
    }
    pub fn len(&self) -> u32 {
        (self.samples.len() as u64 * SAMPLE_RATE as u64 / self.sample_rate as u64) as u32
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
