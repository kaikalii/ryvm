use std::{
    collections::{HashMap, HashSet},
    f32::consts::{FRAC_2_PI, PI},
    iter::{once, repeat},
    sync::Arc,
};

use crossbeam::sync::ShardedLock;
use serde_derive::{Deserialize, Serialize};

#[cfg(feature = "keyboard")]
use crate::{freq, Keyboard};
use crate::{CloneLock, Instruments, Sampling, U32Lock, MAX_BEATS};

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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Voice {
    pub left: SampleType,
    pub right: SampleType,
    pub velocity: SampleType,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Frame {
    pub first: Voice,
    pub extra: Vec<Voice>,
}

impl Voice {
    pub fn stereo(left: SampleType, right: SampleType) -> Self {
        Voice {
            left,
            right,
            velocity: 1.0,
        }
    }
    pub fn mono(both: SampleType) -> Self {
        Voice::stereo(both, both)
    }
    pub fn velocity(self, velocity: SampleType) -> Self {
        Voice { velocity, ..self }
    }
}

impl Frame {
    pub fn multi<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Voice>,
    {
        let mut iter = iter.into_iter();
        let mut frame = Frame {
            first: Voice::default(),
            extra: Vec::new(),
        };
        if let Some(first) = iter.next() {
            frame.first = first;
        }
        frame.extra.extend(iter);
        frame
    }
    pub fn iter(&self) -> impl Iterator<Item = Voice> + '_ {
        once(self.first).chain(self.extra.iter().copied())
    }
}

impl IntoIterator for Frame {
    type Item = Voice;
    type IntoIter = Box<dyn Iterator<Item = Voice>>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(once(self.first).chain(self.extra.into_iter()))
    }
}

impl From<Voice> for Frame {
    fn from(voice: Voice) -> Self {
        Frame {
            first: voice,
            extra: Vec::new(),
        }
    }
}

pub(crate) struct FrameCache {
    pub map: HashMap<InstrId, Frame>,
    pub visited: HashSet<InstrId>,
}

fn default_voices() -> u32 {
    1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_voices(v: &u32) -> bool {
    v == &default_voices()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
        frames: CloneLock<Vec<LoopFrame>>,
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
    pub(crate) fn next(&self, cache: &mut FrameCache, instruments: &Instruments) -> Option<Frame> {
        match self {
            Instrument::Number(n) => Some(Voice::mono(*n).into()),
            Instrument::Wave {
                input, form, waves, ..
            } => {
                let input_frame = instruments.next_from(&*input, cache);
                let mix_inputs: Vec<(Voice, Balance)> = input_frame
                    .into_iter()
                    .flat_map(|f| f.into_iter())
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
            }
            Instrument::Mixer(list) => {
                let next_frame: Vec<(Voice, Balance)> = list
                    .iter()
                    .map(|(id, bal)| {
                        if let Some(frame) = instruments.next_from(id, cache) {
                            (frame.first, *bal)
                        } else {
                            (Voice::default(), Balance::default())
                        }
                    })
                    .collect();
                mix(&next_frame)
            }
            #[cfg(feature = "keyboard")]
            Instrument::Keyboard(keyboard) => {
                let freqs: Vec<SampleType> = keyboard
                    .pressed(|set| set.iter().map(|&(letter, oct)| freq(letter, oct)).collect());
                let voices: Vec<Voice> = freqs.into_iter().map(Voice::mono).collect();
                if voices.is_empty() {
                    None
                } else {
                    Some(Frame::multi(voices))
                }
            }
            Instrument::DrumMachine(samplings) => {
                if samplings.is_empty() {
                    return None;
                }
                let mut voices = Vec::new();
                let frames_per_sub =
                    instruments.frames_per_measure() as SampleType / MAX_BEATS as SampleType;
                let ix = instruments.measure_i();
                for sampling in samplings {
                    let samples = &sampling.sample.samples();
                    for b in 0..MAX_BEATS {
                        let start = (frames_per_sub * b as f32) as u32;
                        if sampling.beat.get(b) && ix >= start {
                            let si = (ix - start) as usize;
                            if si < samples.len() {
                                voices.push((samples[si], Balance::default()));
                            }
                        }
                    }
                }
                mix(&voices)
            }
            Instrument::Loop {
                input,
                measures,
                recording,
                frames,
            } => {
                let mut frames = frames.lock();
                let frames_per_loop = instruments.frames_per_measure() * *measures as u32;
                let loop_i = instruments.i() % frames_per_loop;
                if loop_i == 0 {
                    for frame in frames.iter_mut() {
                        frame.new = false;
                    }
                }
                let input_frame = instruments.next_from(&*input, cache);
                if *recording && input_frame.is_some() {
                    frames[loop_i as usize] = LoopFrame {
                        frame: input_frame.clone(),
                        new: true,
                    };
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
                    input_frame
                } else {
                    frames[loop_i as usize].frame.clone()
                }
            }
        }
    }
    pub fn set(&mut self, num: SampleType) {
        if let Instrument::Number(n) = self {
            *n = num;
        }
    }
}

fn mix(list: &[(Voice, Balance)]) -> Option<Frame> {
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
