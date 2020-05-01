use std::{f32::consts::PI, path::PathBuf};

use rand::random;

use crate::{
    ActiveSampling, Channel, CloneCell, CloneLock, Control, Enveloper, FrameCache, State, Voice,
    WaveForm, ADSR,
};

#[derive(Debug)]
pub enum Device {
    Wave(Box<Wave>),
    DrumMachine(Box<DrumMachine>),
    Filter {
        input: String,
        value: f32,
        avg: CloneCell<Voice>,
    },
}

#[derive(Debug, Clone)]
pub struct Wave {
    pub form: WaveForm,
    pub voices: u32,
    pub(crate) waves: CloneLock<Vec<u32>>,
    pub octave: Option<i8>,
    pub pitch_bend_range: f32,
    pub adsr: ADSR,
    pub(crate) enveloper: CloneLock<Enveloper>,
}

#[derive(Debug, Clone, Default)]
pub struct DrumMachine {
    pub(crate) samples: Vec<PathBuf>,
    pub(crate) samplings: CloneLock<Vec<ActiveSampling>>,
}

impl DrumMachine {
    pub fn samples_len(&self) -> usize {
        self.samples.len()
    }
    pub fn set_path(&mut self, index: usize, path: PathBuf) {
        self.samples.resize(index + 1, PathBuf::new());
        self.samples[index] = path;
    }
}

impl Device {
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
                let Wave {
                    form,
                    voices,
                    octave,
                    pitch_bend_range,
                    adsr,
                    waves,
                    enveloper,
                    ..
                } = &**wave;
                // Ensure that waves is initialized
                let mut waves = waves.lock();
                waves.resize(*voices as usize, 0);

                let mut enveloper = enveloper.lock();
                enveloper.register(cache.channel_controls(channel_num));
                let voice = enveloper
                    .states(
                        state.sample_rate,
                        octave.unwrap_or(0),
                        *pitch_bend_range,
                        *adsr,
                    )
                    .zip(&mut *waves)
                    .map(|((freq, amp), i)| {
                        if freq == 0.0 {
                            return Voice::SILENT;
                        }
                        // spc = samples per cycle
                        let spc = state.sample_rate as f32 / freq;
                        let t = *i as f32 / spc;
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
                            WaveForm::Noise => random::<f32>() % 2.0 - 1.0,
                        } * WaveForm::MIN_ENERGY
                            / form.energy()
                            * amp;
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
            Device::Filter { input, value, avg } => {
                // Determine the factor used to maintain the running average
                let avg_factor = value.powf(2.0);
                // Get the input channels
                let frame = channel.next_from(channel_num, input, state, cache);
                let left = avg.load().left * (1.0 - avg_factor) + frame.left * avg_factor;
                let right = avg.load().right * (1.0 - avg_factor) + frame.right * avg_factor;
                avg.store(Voice::stereo(left, right));
                avg.load()
            }
        }
    }
    /// Get a list of this instrument's inputs
    pub fn inputs(&self) -> Vec<&str> {
        match self {
            Device::Filter { input, .. } => vec![input],
            _ => Vec::new(),
        }
    }
    /// Replace any of this instrument's inputs that match the old id with the new one
    #[allow(clippy::single_match)]
    pub fn replace_input(&mut self, old: String, new: String) {
        let replace = |id: &mut String| {
            if id == &old {
                *id = new.clone();
            }
        };
        match self {
            Device::Filter { input, .. } => replace(input),
            _ => {}
        }
    }
}
