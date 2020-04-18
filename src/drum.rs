use std::{
    convert::Infallible,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    thread,
    time::Duration,
};

use itertools::Itertools;
use lockfree::{map::Map, set::Set};
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

#[derive(Debug)]
pub struct SampleBank {
    inputs: Arc<Set<PathBuf>>,
    samples: Arc<Map<PathBuf, Result<Sample, String>>>,
}

impl Default for SampleBank {
    fn default() -> Self {
        SampleBank::new()
    }
}

impl SampleBank {
    pub fn new() -> Self {
        let samples = Arc::new(Map::new());
        let samples_clone = Arc::clone(&samples);
        let inputs = Arc::new(Set::new());
        let inputs_clone = Arc::clone(&inputs);

        thread::spawn(move || loop {
            if let Some(path) = inputs_clone.iter().next().map(|g| PathBuf::clone(&g)) {
                let res = Sample::open(&path).map_err(|e| e.to_string());
                if let Err(e) = &res {
                    println!("{}", e);
                }
                samples_clone.insert(path.clone(), res);
                inputs_clone.remove(&path);
            }
            thread::sleep(Duration::from_millis(50));
        });
        SampleBank { inputs, samples }
    }
    pub fn load(&self, path: PathBuf, force: bool) {
        if force || (self.inputs.get(&path).is_none() || self.samples.get(&path).is_none()) {
            let _ = self.inputs.insert(path);
        }
    }
    pub fn get<P, F, R>(&self, path: P, f: F) -> Option<R>
    where
        P: AsRef<Path>,
        F: FnOnce(&Sample) -> R,
    {
        let res = self.samples.get(path.as_ref());
        if res.is_none() {
            self.load(path.as_ref().to_path_buf(), false);
        }
        if let Some(guard) = res {
            if let Ok(samples) = guard.val() {
                Some(f(samples))
            } else {
                None
            }
        } else {
            None
        }
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
