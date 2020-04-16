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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sampling {
    pub sample: Sample,
    pub beat: BeatPattern,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sample {
    path: Option<PathBuf>,
    sample_rate: u32,
    #[serde(skip)]
    samples: Vec<Voice>,
}

impl fmt::Debug for Sample {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Sample")
            .field("path", &self.path)
            .field("sample_rate", &self.sample_rate)
            .field("samples", &self.samples.len())
            .finish()
    }
}

impl Default for Sample {
    fn default() -> Self {
        Sample {
            path: None,
            sample_rate: 1,
            samples: Vec::new(),
        }
    }
}

impl Sample {
    pub fn path(&self) -> Option<&Path> {
        self.path.as_ref().map(|p| p.as_ref())
    }
    pub fn set_path<P>(&mut self, path: P) -> Result<(), Box<dyn Error>>
    where
        P: AsRef<Path>,
    {
        self.path = Some(path.as_ref().into());
        self.init()?;
        Ok(())
    }
    pub fn open<P>(path: P) -> Result<Self, Box<dyn Error>>
    where
        P: AsRef<Path>,
    {
        let mut sample = Sample {
            path: Some(path.as_ref().into()),
            ..Sample::default()
        };
        sample.init()?;
        Ok(sample)
    }
    pub fn init(&mut self) -> Result<(), Box<dyn Error>> {
        let path = if let Some(path) = &self.path {
            path
        } else {
            return Ok(());
        };
        if let Ok(decoder) = Decoder::new(fs::File::open(path)?) {
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
    pub fn samples(&self) -> &[Voice] {
        &self.samples
    }
}

pub const MAX_BEATS: u8 = 32;

#[derive(Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(into = "BeatPatternRep", from = "BeatPatternRep")]
pub struct BeatPattern(pub u32);

impl BeatPattern {
    pub fn get(self, beat: u8) -> bool {
        bit_at(self.0, beat)
    }
}

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
            .take(MAX_BEATS as usize)
            .enumerate()
            .fold(0u32, |acc, (i, n)| {
                acc + if n > 0 { 2u32.pow(i as u32) } else { 0 }
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
