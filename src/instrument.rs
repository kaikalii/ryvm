use std::{
    collections::HashMap,
    f32::consts::{FRAC_2_PI, PI},
    iter::{once, repeat},
    sync::Arc,
};

use crossbeam::sync::ShardedLock;
use serde_derive::{Deserialize, Serialize};

use crate::{
    Channels, CloneLock, DynInput, Enveloper, Frame, FrameCache, InstrId, InstrIdRef, Instruments,
    Sampling, U32Lock, Voice,
};

pub type SampleType = f32;

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
        #[serde(skip)]
        enveloper: CloneLock<Enveloper>,
    },
    Mixer(HashMap<InstrId, Balance>),
    #[cfg(feature = "keyboard")]
    Keyboard {
        octave: u8,
    },
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
        value: DynInput,
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
            enveloper: CloneLock::new(Enveloper::default()),
        }
    }
    pub fn voices(mut self, v: u32) -> Self {
        if let Instrument::Wave { voices, waves, .. } = &mut self {
            *voices = v;
            waves.resize(v as usize, U32Lock::new(0));
        }
        self
    }
    pub fn is_input_device(&self) -> bool {
        matches!(self, Instrument::Keyboard{..})
    }
    pub(crate) fn next(
        &self,
        cache: &mut FrameCache,
        instruments: &Instruments,
        my_id: InstrId,
    ) -> Channels {
        match self {
            Instrument::Number(n) => Frame::from(Voice::mono(*n)).into(),
            Instrument::Wave {
                input,
                form,
                waves,
                enveloper,
                ..
            } => {
                let mut enveloper = enveloper.lock();
                let res = instruments
                    .next_from(&*input, cache)
                    .filter_map(|input_frame| {
                        macro_rules! build_wave {
                            ($freq:expr, $amp:expr, $i:expr) => {{
                                let ix = $i.load();
                                if $freq == 0.0 {
                                    return Voice::mono(0.0);
                                }
                                // spc = samples per cycle
                                let spc = SAMPLE_RATE as SampleType / $freq;
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
                                    / form.energy()
                                    * $amp;
                                $i.store((ix + 1) % spc as u32);
                                Voice::mono(s)
                            }};
                        }
                        let mix_inputs: Vec<(Voice, Balance)> = match input_frame {
                            Frame::Voice(voice) => once((voice.left, 1.0))
                                .zip(waves.iter())
                                .map(|((freq, amp), i)| build_wave!(freq, amp, i))
                                .zip(repeat(Balance::default()))
                                .collect(),
                            Frame::Controls(controls) => {
                                enveloper.register(controls.iter().copied());
                                enveloper
                                    .states()
                                    .zip(waves.iter())
                                    .map(|((freq, amp), i)| build_wave!(freq, amp, i))
                                    .zip(repeat(Balance::default()))
                                    .collect()
                            }
                        };
                        mix(&mix_inputs)
                    });
                enveloper.progress();
                res
            }
            Instrument::Mixer(list) => {
                let mut voices = Vec::new();
                for (id, bal) in list {
                    for frame in instruments.next_from(id, cache).frames() {
                        voices.push((frame.voice(), *bal));
                    }
                }
                mix(&voices).into()
            }
            #[cfg(feature = "keyboard")]
            Instrument::Keyboard { octave } => {
                if instruments
                    .current_keyboard
                    .as_ref()
                    .map(|kb_id| kb_id == &my_id)
                    .unwrap_or(false)
                {
                    Frame::controls(instruments.keyboard(|kb| {
                        kb.controls()
                            .drain()
                            .map(|con| con.add_octave(*octave))
                            .collect::<Vec<_>>()
                    }))
                    .into()
                } else {
                    Default::default()
                }
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
                if *recording
                    && input_channels
                        .primary()
                        .map(Frame::is_some)
                        .unwrap_or(false)
                {
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
            Instrument::Filter { input, value, avgs } => {
                let avg_factor = match value {
                    DynInput::Num(f) => *f,
                    DynInput::Id(filter_input) => {
                        let filter_input_channels = instruments.next_from(filter_input, cache);
                        filter_input_channels
                            .primary()
                            .map(|frame| frame.left())
                            .unwrap_or(1.0)
                    }
                }
                .powf(2.0);
                let input_channels = instruments.next_from(input, cache);
                let mut avgs = avgs.lock();
                input_channels
                    .iter()
                    .map(|(id, frame)| {
                        let avg = avgs.entry(id.clone()).or_insert_with(|| frame.voice());
                        avg.left = avg.left * (1.0 - avg_factor) + frame.left() * avg_factor;
                        avg.right = avg.right * (1.0 - avg_factor) + frame.right() * avg_factor;
                        (id.clone(), Frame::from(*avg))
                    })
                    .collect()
            }
        }
    }
    pub fn inputs(&self) -> Vec<InstrIdRef> {
        match self {
            Instrument::Wave { input, .. } => vec![input.as_ref()],
            Instrument::Mixer(inputs) => inputs.keys().map(InstrId::as_ref).collect(),
            Instrument::Filter { input, .. } => vec![input.as_ref()],
            _ => Vec::new(),
        }
    }
    pub fn replace_input(&mut self, old: InstrId, new: InstrId) {
        let replace = |id: &mut InstrId| {
            if id == &old {
                *id = new.clone();
            }
        };
        match self {
            Instrument::Wave { input, .. } => replace(input),
            Instrument::Mixer(inputs) => {
                let mixed: Vec<_> = inputs.drain().collect();
                for (mut id, bal) in mixed {
                    replace(&mut id);
                    inputs.insert(id, bal);
                }
            }
            Instrument::Filter { input, .. } => replace(input),
            _ => {}
        }
    }
    pub fn set(&mut self, num: SampleType) {
        if let Instrument::Number(n) = self {
            *n = num;
        }
    }
}

pub fn mix(list: &[(Voice, Balance)]) -> Option<Frame> {
    if list.is_empty() {
        return None;
    }
    let (left_sum, right_sum) = list.iter().fold((0.0, 0.0), |(lacc, racc), (voice, bal)| {
        let (l, r) = bal.stereo_volume();
        (lacc + voice.left * l, racc + voice.right * r)
    });
    let (left_product, right_product) =
        list.iter().fold((0.0, 0.0), |(lacc, racc), (voice, bal)| {
            let (l, r) = bal.stereo_volume();
            (lacc * voice.left * l, racc * voice.right * r)
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
