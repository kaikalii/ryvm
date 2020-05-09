use std::{
    collections::VecDeque,
    f32::consts::{FRAC_2_PI, PI},
    iter::once,
    path::PathBuf,
};

use rand::random;
use ryvm_spec::{DynamicValue, FilterType, Name, WaveForm};

use crate::{
    ActiveSampling, Channel, CloneCell, CloneLock, Control, Enveloper, Frame, FrameCache, Letter,
    State, Voice, ADSR,
};

/// A virtual audio processing device
#[derive(Debug)]
pub enum Device {
    /// A wave synthesizer
    Wave(Box<Wave>),
    /// A drum machine
    DrumMachine(DrumMachine),
    /// A low-pass filter
    Filter(Filter),
    /// A volume and pan balancer
    Balance(Balance),
    /// A reverb simulator
    Reverb(Reverb),
}

/// A wave synthesizer
#[derive(Debug, Clone)]
pub struct Wave {
    /// The waveform
    pub form: WaveForm,
    pub waves: CloneLock<Vec<Frame>>,
    /// The octave
    pub octave: Option<i8>,
    /// The +- range for pitch bending
    pub pitch_bend_range: DynamicValue,
    /// The attack-decay-sustain-release envelope
    pub adsr: ADSR<DynamicValue>,
    enveloper: CloneLock<Enveloper>,
}

/// A drum machine
#[derive(Debug, Clone, Default)]
pub struct DrumMachine {
    pub samples: Vec<PathBuf>,
    pub samplings: CloneLock<Vec<ActiveSampling>>,
}

#[derive(Debug, Clone)]
enum FilterState {
    LowPass(CloneCell<Voice>),
    Comb(CloneLock<VecDeque<Voice>>),
    Crush {
        counter: CloneCell<usize>,
        voice: CloneCell<Voice>,
    },
    Distortion,
}

impl From<FilterType> for FilterState {
    fn from(ty: FilterType) -> Self {
        match ty {
            FilterType::LowPass => FilterState::LowPass(CloneCell::new(Voice::SILENT)),
            FilterType::Comb => FilterState::Comb(CloneLock::new(VecDeque::new())),
            FilterType::Crush => FilterState::Crush {
                counter: CloneCell::new(0),
                voice: CloneCell::new(Voice::SILENT),
            },
            FilterType::Distortion => FilterState::Distortion,
        }
    }
}

impl PartialEq<FilterType> for FilterState {
    fn eq(&self, ty: &FilterType) -> bool {
        match (self, ty) {
            (FilterState::LowPass(_), FilterType::LowPass)
            | (FilterState::Comb(_), FilterType::Comb) => true,
            _ => false,
        }
    }
}

/// A low-pass filter
#[derive(Debug, Clone)]
pub struct Filter {
    /// The name of the input device
    pub input: Name,
    /// The value used to determine filter strength
    pub value: DynamicValue,
    state: FilterState,
}

impl Filter {
    pub fn set_type(&mut self, ty: FilterType) {
        if self.state != ty {
            self.state = ty.into();
        }
    }
}

/// A volume and pan balancer
#[derive(Debug, Clone)]
pub struct Balance {
    /// The name of the input device
    pub input: Name,
    /// The volume
    pub volume: DynamicValue,
    /// The left-right
    pub pan: DynamicValue,
}

const SPEED_OF_SOUND: f32 = 340.27;

/// A reverb simulator
#[derive(Debug, Clone)]
pub struct Reverb {
    pub input: Name,
    pub size: DynamicValue,
    pub energy_mul: DynamicValue,
    frames: CloneLock<VecDeque<Voice>>,
}

impl Device {
    /// Create a new wave
    #[must_use]
    pub fn new_wave(form: WaveForm) -> Self {
        Device::Wave(Box::new(Wave {
            form,
            octave: None,
            pitch_bend_range: DynamicValue::Static(12.0),
            adsr: ADSR::default().map(|f| DynamicValue::Static(*f)),
            enveloper: CloneLock::new(Enveloper::default()),
            waves: CloneLock::new(vec![0; 10]),
        }))
    }
    /// Create a new drum machine
    #[must_use]
    pub fn new_drum_machine() -> Self {
        Device::DrumMachine(DrumMachine {
            samples: Vec::new(),
            samplings: CloneLock::new(Vec::new()),
        })
    }
    /// Create a new filter
    #[must_use]
    pub fn new_filter(input: Name, value: DynamicValue, ty: FilterType) -> Self {
        Device::Filter(Filter {
            input,
            value,
            state: ty.into(),
        })
    }
    /// Create a new balance
    #[must_use]
    pub fn new_balance(input: Name) -> Self {
        Device::Balance(Balance {
            input,
            volume: DynamicValue::Static(1.0),
            pan: DynamicValue::Static(0.0),
        })
    }
    /// Create a new reverb
    #[must_use]
    pub fn new_reverb(input: Name) -> Self {
        Device::Reverb(Reverb {
            input,
            size: DynamicValue::Static(1.0),
            energy_mul: DynamicValue::Static(0.5),
            frames: CloneLock::new(VecDeque::new()),
        })
    }
    pub fn next(
        &self,
        channel_num: u8,
        channel: &Channel,
        state: &State,
        cache: &mut FrameCache,
        #[allow(unused_variables)] my_name: &str,
    ) -> Voice {
        match self {
            // Waves
            Device::Wave(wave) => {
                let mut waves = wave.waves.lock();

                let mut enveloper = wave.enveloper.lock();
                enveloper.register(cache.channel_controls(channel_num));
                let adsr = wave
                    .adsr
                    .map_or_default(|value| state.resolve_dynamic_value(value, channel_num, cache));
                let pitch_bend_range = state
                    .resolve_dynamic_value(&wave.pitch_bend_range, channel_num, cache)
                    .unwrap_or(12.0);
                let voice = enveloper
                    .envelopes()
                    .zip(&mut *waves)
                    .map(|(env_frame, i)| {
                        let (letter, octave) = Letter::from_u8(env_frame.note);
                        let freq = letter.freq(
                            (i16::from(octave) + i16::from(wave.octave.unwrap_or(0))).max(0) as u8,
                        ) * 2_f32.powf(env_frame.pitch_bend * pitch_bend_range / 12.0);
                        if freq == 0.0 {
                            return Voice::SILENT;
                        }
                        // spc = samples per cycle
                        let spc = state.sample_rate as f32 / freq;
                        let t = *i as f32 / spc;
                        let s = match wave.form {
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
                            WaveForm::Noise => random::<f32>() % 2.0 - 1.0,
                        } * env_frame.amplitude
                            * MIN_ENERGY
                            / waveform_energy(wave.form);
                        *i = (*i + 1) % spc as Frame;
                        Voice::mono(s)
                    })
                    .fold(Voice::SILENT, |acc, v| acc + v);

                enveloper.progress(state.sample_rate, adsr);
                voice
            }
            // Drum Machine
            Device::DrumMachine(drums) => {
                let mut samplings = drums.samplings.lock();
                // Process controls
                if channel_num == state.curr_channel() || cache.from_loop {
                    for control in cache.all_controls() {
                        if let Control::Pad(i, v) = control {
                            let index = i as usize;
                            if index < drums.samples.len() {
                                samplings.push(ActiveSampling {
                                    index,
                                    i: 0,
                                    velocity: f32::from(v) / 127.0,
                                });
                            }
                        }
                    }
                }
                // Mix currently playing samples
                let mut mixed = Voice::SILENT;
                for ms in (0..samplings.len()).rev() {
                    let ActiveSampling { index, i, velocity } = &mut samplings[ms];
                    if let Some(res) = state.sample_bank.get(&drums.samples[*index]).finished() {
                        if let Ok(sample) = &*res {
                            if *i < sample.len(state.sample_rate) {
                                mixed += *sample.voice(*i, state.sample_rate) * *velocity;
                                *i += 1;
                            } else {
                                samplings.remove(ms);
                            }
                        }
                    }
                }
                mixed
            }
            // Filters
            Device::Filter(filter) => {
                // Get the input channels
                let frame = channel.next_from(channel_num, &filter.input, state, cache);
                // Determine the factor used to maintain the running average
                let value = state.resolve_dynamic_value(&filter.value, channel_num, cache);
                match &filter.state {
                    FilterState::LowPass(avg) => {
                        let avg_factor = value.unwrap_or(1.0).powf(2.0);
                        let left = avg.load().left * (1.0 - avg_factor) + frame.left * avg_factor;
                        let right =
                            avg.load().right * (1.0 - avg_factor) + frame.right * avg_factor;
                        avg.store(Voice::stereo(left, right));
                        avg.load()
                    }
                    FilterState::Comb(prevs) => {
                        let mut prevs = prevs.lock();
                        let delay_frames = value.map_or(0, |val| (val * 0x7f as f32) as usize);
                        prevs.push_back(frame);
                        let mut output = None;
                        loop {
                            if prevs.len() <= delay_frames {
                                break;
                            }
                            output = prevs.pop_front();
                        }
                        (output.unwrap_or(frame) + frame) * 0.5
                    }
                    FilterState::Crush { counter, voice } => {
                        let delay_frames = value.map_or(0, |val| (val * 0x7f as f32) as usize);
                        if counter.load() == 0 {
                            counter.store(delay_frames);
                            voice.store(frame);
                            frame
                        } else {
                            counter.store(counter.load() - 1);
                            voice.load()
                        }
                    }
                    FilterState::Distortion => {
                        let threshold = value.unwrap_or(1.0).max(0.01).powf(2.0);
                        Voice::stereo(
                            frame.left.max(-threshold).min(threshold),
                            frame.right.max(-threshold).min(threshold),
                        ) * (1.0 / threshold)
                    }
                }
            }
            // Balance
            Device::Balance(bal) => {
                let frame = channel.next_from(channel_num, &bal.input, state, cache);

                let volume = state
                    .resolve_dynamic_value(&bal.volume, channel_num, cache)
                    .unwrap_or(0.5);
                let pan = state
                    .resolve_dynamic_value(&bal.pan, channel_num, cache)
                    .unwrap_or(0.0);

                let pan =
                    Voice::stereo((1.0 + pan).min(1.0).max(0.0), (1.0 - pan).min(1.0).max(0.0));
                let volume = volume.min(1.0).max(-1.0);

                frame * pan * volume
            }
            // Reverb
            Device::Reverb(reverb) => {
                let input_frame = channel.next_from(channel_num, &reverb.input, state, cache);
                let size = state
                    .resolve_dynamic_value(&reverb.size, channel_num, cache)
                    .unwrap_or(1.0)
                    * 10.0;
                let energy_mul = state
                    .resolve_dynamic_value(&reverb.energy_mul, channel_num, cache)
                    .unwrap_or(0.5);
                let return_time = size * 2.0 / SPEED_OF_SOUND;
                let return_frame_count = (return_time * state.sample_rate as f32) as usize;
                let mut frames = reverb.frames.lock();
                let mut reverbed = Voice::SILENT;
                while frames.len() > return_frame_count {
                    if let Some(voice) = frames.pop_front() {
                        reverbed = voice;
                    }
                }
                let output = input_frame + reverbed;
                frames.push_back(output * energy_mul);
                output
            }
        }
    }
    pub fn end_envelopes(&mut self, id: u64) {
        if let Device::Wave(wave) = self {
            wave.enveloper.lock().end_notes(id);
        }
    }
    /// Get a list of this device's input devices
    pub fn inputs(&self) -> Vec<&str> {
        match self {
            Device::Wave(wave) => wave
                .pitch_bend_range
                .input()
                .into_iter()
                .chain(wave.adsr.inputs())
                .collect(),
            Device::Balance(bal) => once(bal.input.as_str())
                .chain(bal.volume.input())
                .chain(bal.pan.input())
                .collect(),
            Device::Filter(filter) => once(filter.input.as_str())
                .chain(filter.value.input())
                .collect(),
            Device::Reverb(reverb) => once(reverb.input.as_str())
                .chain(reverb.size.input())
                .chain(reverb.energy_mul.input())
                .collect(),
            _ => Vec::new(),
        }
    }
}

const MIN_ENERGY: f32 = 0.5;

fn waveform_energy(form: WaveForm) -> f32 {
    match form {
        WaveForm::Sine => 0.5,
        WaveForm::Square => 1.0,
        WaveForm::Saw => 0.6,
        WaveForm::Triangle => 0.5,
        WaveForm::Noise => 0.5,
    }
}
