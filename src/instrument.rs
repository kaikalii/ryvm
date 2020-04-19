use std::{
    collections::HashMap,
    f32::consts::{FRAC_2_PI, PI},
    iter::repeat,
    sync::Arc,
};

use crossbeam::sync::ShardedLock;
use serde_derive::{Deserialize, Serialize};

#[cfg(feature = "keyboard")]
use crate::{freq, Keyboard};
use crate::{Channels, CloneLock, Frame, FrameCache, Instruments, Sampling, U32Lock, Voice};

pub type SampleType = f32;
pub type InstrId = String;
pub type InstrIdRef<'a> = &'a str;

/// The global sample rate
pub const SAMPLE_RATE: u32 = 44100;

/// An error bound for the sample type
pub const SAMPLE_EPSILON: SampleType = std::f32::EPSILON;

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

fn default_voices() -> u32 {
    1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_voices(v: &u32) -> bool {
    v == &default_voices()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WaveForm {
    Sine,
    Square,
    Saw,
    Triangle,
}

impl WaveForm {
    const MIN_ENERGY: SampleType = 0.5;
    pub fn energy(self) -> SampleType {
        match self {
            WaveForm::Sine => FRAC_2_PI,
            WaveForm::Square => 1.0,
            WaveForm::Saw => 0.5,
            WaveForm::Triangle => 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopFrame {
    pub(crate) frame: Option<Frame>,
    pub(crate) new: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterSetting {
    Id(InstrId),
    Static(SampleType),
}

/// An instrument for producing sounds
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Instrument {
    Number(SampleType),
    Wave {
        input: InstrId,
        form: WaveForm,
        #[serde(default = "default_voices", skip_serializing_if = "is_default_voices")]
        voices: u32,
        #[serde(skip)]
        waves: Vec<U32Lock>,
    },
    Mixer(HashMap<InstrId, Balance>),
    #[cfg(feature = "keyboard")]
    Keyboard(Keyboard),
    DrumMachine(Vec<Sampling>),
    Loop {
        input: InstrId,
        measures: u8,
        #[serde(skip)]
        recording: bool,
        playing: bool,
        frames: CloneLock<Vec<LoopFrame>>,
    },
    Filter {
        input: InstrId,
        setting: FilterSetting,
        #[serde(skip)]
        avgs: Arc<CloneLock<Channels<Voice>>>,
    },
}

impl Instrument {
    #[allow(clippy::single_match)]
    pub fn init(&mut self) {
        match self {
            Instrument::Wave { voices, waves, .. } => {
                waves.resize(*voices as usize, U32Lock::new(0));
            }
            _ => {}
        }
    }
    pub fn wave<I>(id: I, form: WaveForm) -> Self
    where
        I: Into<InstrId>,
    {
        Instrument::Wave {
            input: id.into(),
            form,
            voices: default_voices(),
            waves: vec![U32Lock::new(0)],
        }
    }
    pub fn voices(mut self, v: u32) -> Self {
        if let Instrument::Wave { voices, waves, .. } = &mut self {
            *voices = v;
            waves.resize(v as usize, U32Lock::new(0));
        }
        self
    }
    pub(crate) fn next(&self, cache: &mut FrameCache, instruments: &Instruments) -> Channels {
        match self {
            Instrument::Number(n) => Frame::from(Voice::mono(*n)).into(),
            Instrument::Wave {
                input, form, waves, ..
            } => {
                instruments
                    .next_from(&*input, cache)
                    .filter_map(|input_frame| {
                        let mix_inputs: Vec<(Voice, Balance)> = input_frame
                            .iter()
                            .zip(waves.iter())
                            .map(|(voice, i)| {
                                let ix = i.load();
                                if voice.left == 0.0 {
                                    return Voice::mono(0.0);
                                }
                                // spc = samples per cycle
                                let spc = SAMPLE_RATE as SampleType / voice.left;
                                let t = ix as SampleType / spc;
                                let s = match form {
                                    WaveForm::Sine => (t * 2.0 * PI).sin(),
                                    WaveForm::Square => {
                                        if t < 0.5 {
                                            1.0
                                        } else {
                                            -1.0
                                        }
                                    }
                                    WaveForm::Saw => 2.0 * (t % 1.0) - 1.0,
                                    WaveForm::Triangle => 2.0 * (2.0 * (t % 1.0) - 1.0).abs() - 1.0,
                                } * WaveForm::MIN_ENERGY
                                    / form.energy();
                                i.store((ix + 1) % spc as u32);
                                Voice::mono(s)
                            })
                            .zip(repeat(Balance::default()))
                            .collect();
                        mix(&mix_inputs)
                    })
            }
            Instrument::Mixer(list) => {
                let mut voices = Vec::new();
                for (id, bal) in list {
                    for frame in instruments.next_from(id, cache).frames() {
                        voices.push((frame.first, *bal));
                    }
                }
                mix(&voices).into()
            }
            #[cfg(feature = "keyboard")]
            Instrument::Keyboard(keyboard) => {
                let freqs: Vec<SampleType> = keyboard
                    .pressed(|set| set.iter().map(|&(letter, oct)| freq(letter, oct)).collect());
                let voices: Vec<Voice> = freqs.into_iter().map(Voice::mono).collect();
                Frame::multi(voices).into()
            }
            Instrument::DrumMachine(samplings) => {
                if samplings.is_empty() {
                    return Channels::new();
                }
                let mut voices = Vec::new();
                let ix = instruments.measure_i();
                for sampling in samplings {
                    let frames_per_sub = instruments.frames_per_measure() as SampleType
                        / sampling.beat.0.len() as SampleType;
                    if let Some(res) = instruments.sample_bank.get(&sampling.path).finished() {
                        if let Ok(sample) = &*res {
                            let samples = &sample.samples();
                            for b in 0..sampling.beat.0.len() {
                                let start = (frames_per_sub * b as f32) as u32;
                                if sampling.beat.0[b as usize] && ix >= start {
                                    let si = (ix - start) as usize;
                                    if si < samples.len() {
                                        voices.push((samples[si], Balance::default()));
                                    }
                                }
                            }
                        }
                    }
                }
                mix(&voices).into()
            }
            Instrument::Loop {
                input,
                measures,
                recording,
                playing,
                frames,
            } => {
                let mut frames = frames.lock();
                // The correct number of frames per loop at the current tempo
                let ideal_fpl = instruments.frames_per_measure() * *measures as u32;
                // The actual number of frames per loop
                let actual_fpl = frames.len() as u32;
                // Calculate the index of the loop's current sample adjusting for changes in tempo
                let loop_i = ((instruments.i() % ideal_fpl) as u64 * actual_fpl as u64
                    / ideal_fpl as u64) as u32;
                if loop_i == 0 {
                    for frame in frames.iter_mut() {
                        frame.new = false;
                    }
                }
                // Get input frame
                let input_channels = instruments.next_from(&*input, cache);
                if *recording && input_channels.primary().is_some() {
                    // Record if recording and there is input
                    frames[loop_i as usize] = LoopFrame {
                        frame: input_channels.primary().cloned(),
                        new: true,
                    };
                    // Go backward and set all old frames to new and empty
                    for i in (0..(loop_i as usize)).rev() {
                        if frames[i].new {
                            break;
                        } else {
                            frames[i] = LoopFrame {
                                frame: None,
                                new: true,
                            }
                        }
                    }
                    Channels::new()
                } else if *playing {
                    // Play the loop
                    frames[loop_i as usize].frame.clone().into()
                } else {
                    Channels::new()
                }
            }
            Instrument::Filter {
                input,
                setting: _s,
                avgs,
            } => {
                let input_channels = instruments.next_from(&*input, cache);
                let mut avgs = avgs.lock();
                input_channels
                    .iter()
                    .map(|(id, frame)| {
                        let avg_factor = 0.5;
                        let avg = avgs.entry(id.clone()).or_insert(frame.first);
                        avg.left = avg.left * (1.0 - avg_factor) + frame.first.left * avg_factor;
                        avg.right = avg.right * (1.0 - avg_factor) + frame.first.right * avg_factor;
                        (id.clone(), Frame::from(*avg))
                    })
                    .collect()
            }
        }
    }
    pub fn set(&mut self, num: SampleType) {
        if let Instrument::Number(n) = self {
            *n = num;
        }
    }
}

pub fn mix(list: &[(Voice, Balance)]) -> Option<Frame> {
    // let (left_vol_sum, right_vol_sum) = list.iter().fold((0.0, 0.0), |(lacc, racc), (_, bal)| {
    //     let (l, r) = bal.stereo_volume();
    //     (lacc + l, racc + r)
    // });
    if list.is_empty() {
        return None;
    }
    let (left_sum, right_sum) = list.iter().fold((0.0, 0.0), |(lacc, racc), (voice, bal)| {
        let (l, r) = bal.stereo_volume();
        (
            lacc + voice.left * l * voice.velocity,
            racc + voice.right * r * voice.velocity,
        )
    });
    let (left_product, right_product) =
        list.iter().fold((0.0, 0.0), |(lacc, racc), (voice, bal)| {
            let (l, r) = bal.stereo_volume();
            (
                lacc * voice.left * l * voice.velocity,
                racc * voice.right * r * voice.velocity,
            )
        });
    Some(
        Voice::stereo(
            left_sum - left_product.abs() * left_sum.signum(),
            right_sum - right_product.abs() * right_sum.signum(),
        )
        .into(),
    )
}

fn default_volume() -> SampleType {
    1.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_volume(v: &SampleType) -> bool {
    (v - default_volume()).abs() < SAMPLE_EPSILON
}

fn default_pan() -> SampleType {
    0.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_pan(v: &SampleType) -> bool {
    (v - default_pan()).abs() < SAMPLE_EPSILON
}

/// A balance wrapper for an `Instrument`
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Balance {
    #[serde(default = "default_volume", skip_serializing_if = "is_default_volume")]
    pub volume: SampleType,
    #[serde(default = "default_pan", skip_serializing_if = "is_default_pan")]
    pub pan: SampleType,
}

impl Default for Balance {
    fn default() -> Self {
        Balance {
            volume: default_volume(),
            pan: default_pan(),
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
    pub fn volume(self, volume: SampleType) -> Self {
        Balance { volume, ..self }
    }
    pub fn pan(self, pan: SampleType) -> Self {
        Balance { pan, ..self }
    }
}
