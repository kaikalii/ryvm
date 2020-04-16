use std::{
    convert::Infallible,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use itertools::Itertools;
use rodio::{Decoder, Source};
use serde_derive::{Deserialize, Serialize};

use crate::{SampleType, Voice};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleBeat {
    pub sample: Sample,
    pub beat: BeatPattern,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub path: PathBuf,
    pub sample_rate: u32,
    #[serde(skip)]
    pub samples: Vec<Voice>,
}

impl Sample {
    pub fn open<P>(path: P) -> Result<Self, Box<dyn Error>>
    where
        P: AsRef<Path>,
    {
        let mut sample = Sample {
            path: path.as_ref().into(),
            sample_rate: 1,
            samples: Vec::new(),
        };
        sample.init()?;
        Ok(sample)
    }
    pub fn init(&mut self) -> Result<(), Box<dyn Error>> {
        if let Ok(decoder) = Decoder::new(fs::File::open(&self.path)?) {
            self.sample_rate = decoder.sample_rate();
            let channels = decoder.channels();
            self.samples = if channels == 1 {
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
        }
        Ok(())
    }
}

pub const MAX_BEATS: u8 = 32;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "BeatPatternRep", from = "BeatPatternRep")]
pub struct BeatPattern(pub u32);

impl BeatPattern {
    pub fn get(self, beat: u8) -> bool {
        bit_at(self.0, beat)
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
        BeatPattern(bpr.0.parse().unwrap())
    }
}

impl FromStr for BeatPattern {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut v = s.as_bytes().to_vec();
        for i in &mut v {
            *i = (*i == b'-' || *i == 1) as u8;
        }
        while v.len() < MAX_BEATS as usize {
            v = v.into_iter().flat_map(|i| vec![i, 0]).collect();
        }
        let u = v
            .into_iter()
            .take(32)
            .enumerate()
            .fold(0u32, |acc, (i, n)| {
                acc + if n > 0 { 2 ^ i } else { 0 } as u32
            });
        Ok(BeatPattern(u))
    }
}

fn bit_at(input: u32, n: u8) -> bool {
    if n < 32 {
        input & (1 << n) != 0
    } else {
        false
    }
}

impl fmt::Display for BeatPattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s: String = (0u8..MAX_BEATS)
            .map(|i| if bit_at(self.0, i) { '-' } else { '_' })
            .collect();
        write!(f, "{}", s)
    }
}
