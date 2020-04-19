use std::{
    convert::Infallible,
    env::{current_dir, current_exe},
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use find_folder::Search;
use itertools::Itertools;
use rodio::{Decoder, Source};
use serde_derive::{Deserialize, Serialize};

use crate::{SampleType, Voice};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sampling {
    pub path: PathBuf,
    pub beat: BeatPattern,
}

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
    pub fn samples(&self) -> &[Voice] {
        &self.samples
    }
}

#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(into = "BeatPatternRep", from = "BeatPatternRep")]
pub struct BeatPattern(pub Vec<bool>);

impl fmt::Debug for BeatPattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct BeatPatternRep(String);

impl From<BeatPattern> for BeatPatternRep {
    fn from(bp: BeatPattern) -> Self {
        BeatPatternRep(bp.to_string())
    }
}

impl From<BeatPatternRep> for BeatPattern {
    fn from(bpr: BeatPatternRep) -> Self {
        bpr.0.parse().unwrap()
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
