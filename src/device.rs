use std::{
    f32::consts::{FRAC_2_PI, PI},
    path::PathBuf,
};

use rand::random;
use ryvm_spec::{DynamicValue, WaveForm};

use crate::{
    ActiveSampling, Channel, CloneCell, CloneLock, Control, Enveloper, FrameCache, State, Voice,
    ADSR,
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
}

/// A wave synthesizer
#[derive(Debug, Clone)]
pub struct Wave {
    /// The waveform
    pub form: WaveForm,
    pub(crate) waves: CloneLock<Vec<u32>>,
    /// The octave
    pub octave: Option<i8>,
    /// The +- range for pitch bending
    pub pitch_bend_range: DynamicValue,
    /// The attack-decay-sustain-release envelope
    pub adsr: ADSR<DynamicValue>,
    pub(crate) enveloper: CloneLock<Enveloper>,
}

/// A drum machine
#[derive(Debug, Clone, Default)]
pub struct DrumMachine {
    pub(crate) samples: Vec<PathBuf>,
    pub(crate) samplings: CloneLock<Vec<ActiveSampling>>,
}

/// A low-pass filter
#[derive(Debug, Clone)]
pub struct Filter {
    /// The name of the input device
    pub input: String,
    /// The value used to determine filter strength
    pub value: DynamicValue,
    avg: CloneCell<Voice>,
}

/// A volume and pan balancer
#[derive(Debug, Clone)]
pub struct Balance {
    /// The name of the input device
    pub input: String,
    /// The volume
    pub volume: DynamicValue,
    /// The left-right
    pub pan: DynamicValue,
}

impl Device {
    /// Create a new wave
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
    pub fn new_drum_machine() -> Self {
        Device::DrumMachine(DrumMachine {
            samples: Vec::new(),
            samplings: CloneLock::new(Vec::new()),
        })
    }
    /// Create a new filter
    pub fn new_filter(input: String, value: DynamicValue) -> Self {
        Device::Filter(Filter {
            input,
            value,
            avg: CloneCell::new(Voice::SILENT),
        })
    }
    /// Create a new balance
    pub fn new_balance(input: String) -> Self {
        Device::Balance(Balance {
            input,
            volume: DynamicValue::Static(1.0),
            pan: DynamicValue::Static(0.0),
        })
    }
    pub(crate) fn next(
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
                // Ensure that waves is initialized
                let mut waves = wave.waves.lock();

                let mut enveloper = wave.enveloper.lock();
                enveloper.register(cache.channel_controls(channel_num));
                let adsr = wave
                    .adsr
                    .map_or_default(|value| state.resolve_dynamic_value(value, channel_num));
                let pitch_bend_range = state
                    .resolve_dynamic_value(&wave.pitch_bend_range, channel_num)
                    .unwrap_or(12.0);
                let voice = enveloper
                    .states(
                        state.sample_rate,
                        wave.octave.unwrap_or(0),
                        pitch_bend_range,
                        adsr,
                    )
                    .zip(&mut *waves)
                    .map(|((freq, amp), i)| {
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
                        } * amp
                            * MIN_ENERGY
                            / waveform_energy(wave.form);
                        *i = (*i + 1) % spc as u32;
                        Voice::mono(s)
                    })
                    .fold(Voice::SILENT, |acc, v| acc + v);

                enveloper.progress(state.sample_rate, adsr.release);
                voice
            }
            // Drum Machine
            Device::DrumMachine(drums) => {
                let mut samplings = drums.samplings.lock();
                // Process controls
                if channel_num == state.curr_channel() || cache.from_loop {
                    for control in cache.all_controls() {
                        if let Control::PadStart(i, v) = control {
                            let index = i as usize;
                            if index < drums.samples.len() {
                                samplings.push(ActiveSampling {
                                    index,
                                    i: 0,
                                    velocity: v as f32 / 127.0,
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
                // Determine the factor used to maintain the running average
                let avg_factor = state
                    .resolve_dynamic_value(&filter.value, channel_num)
                    .unwrap_or(1.0)
                    .powf(2.0);
                // Get the input channels
                let frame = channel.next_from(channel_num, &filter.input, state, cache);
                let left = filter.avg.load().left * (1.0 - avg_factor) + frame.left * avg_factor;
                let right = filter.avg.load().right * (1.0 - avg_factor) + frame.right * avg_factor;
                filter.avg.store(Voice::stereo(left, right));
                filter.avg.load()
            }
            // Balance
            Device::Balance(bal) => {
                let frame = channel.next_from(channel_num, &bal.input, state, cache);

                let volume = state
                    .resolve_dynamic_value(&bal.volume, channel_num)
                    .unwrap_or(0.0);
                let pan = state
                    .resolve_dynamic_value(&bal.pan, channel_num)
                    .unwrap_or(0.0);

                let pan =
                    Voice::stereo((1.0 + pan).min(1.0).max(0.0), (1.0 - pan).min(1.0).max(0.0));
                let volume = volume.min(1.0).max(-1.0);

                frame * pan * volume
            }
        }
    }
    /// Get a list of this device's input devices
    pub fn inputs(&self) -> Vec<&str> {
        match self {
            Device::Filter(Filter { input, .. }) | Device::Balance(Balance { input, .. }) => {
                vec![input]
            }
            _ => Vec::new(),
        }
    }
}

const MIN_ENERGY: f32 = 0.5;

fn waveform_energy(form: WaveForm) -> f32 {
    match form {
        WaveForm::Sine => FRAC_2_PI,
        WaveForm::Square => 1.0,
        WaveForm::Saw => 0.5,
        WaveForm::Triangle => 0.5,
        WaveForm::Noise => 0.5,
    }
}
