use std::{f32::consts::PI, iter::once, path::PathBuf};

use rand::random;

use crate::{
    adjust_i, ActiveSampling, Channel, CloneCell, CloneLock, Control, DynInput, Enveloper,
    FrameCache, State, Voice, WaveForm, ADSR,
};

#[derive(Debug)]
pub enum Device {
    Wave(Box<Wave>),
    DrumMachine(Box<DrumMachine>),
    Filter {
        input: String,
        value: DynInput<f32, String>,
        avg: CloneCell<Voice>,
    },
    Loop {
        input: String,
        start_i: CloneCell<Option<u32>>,
        frames: CloneLock<Vec<Voice>>,
        tempo: f32,
        length: f32,
        loop_state: LoopState,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Recording,
    Playing,
    Disabled,
}

impl Device {
    pub fn pass_thru(&self) -> Option<&str> {
        if let Device::Loop { input, .. } = self {
            Some(input.as_str())
        } else {
            None
        }
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
                enveloper.register(cache.controls_for_channel(channel_num));
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
                if channel_num == state.curr_channel() {
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
                let avg_factor = match value {
                    DynInput::First(f) => *f,
                    DynInput::Second(id) => channel.next_from(channel_num, id, state, cache).left,
                }
                .powf(2.0);
                // Get the input channels
                let frame = channel.next_from(channel_num, input, state, cache);
                let left = avg.load().left * (1.0 - avg_factor) + frame.left * avg_factor;
                let right = avg.load().right * (1.0 - avg_factor) + frame.right * avg_factor;
                avg.store(Voice::stereo(left, right));
                avg.load()
            }
            // Loops
            Device::Loop {
                input,
                start_i,
                frames,
                tempo,
                loop_state,
                length,
            } => {
                let input = channel.next_from(channel_num, input, state, cache);
                if start_i.load().is_none() {
                    if input.is_silent() {
                        return Voice::SILENT;
                    } else {
                        start_i.store(Some(state.i));
                        println!("Started recording");
                    }
                }
                let start_i = start_i.load().unwrap();
                let period = state
                    .loop_period
                    .map(|p| (p as f32 * *length).round() as u32);

                let raw_loop_i = state.i - start_i;
                let loop_i = adjust_i(raw_loop_i, *tempo, state.tempo);

                match loop_state {
                    LoopState::Recording => {
                        if let Some(period) = period {
                            let mut frames = frames.lock();
                            frames.resize(period as usize, Voice::SILENT);
                            frames[(loop_i % period) as usize] = input;
                        } else {
                            frames.lock().push(input);
                        }
                        Voice::SILENT
                    }
                    LoopState::Playing => frames
                        .lock()
                        .get(
                            (loop_i % period.expect("Loop is playing with no period set")) as usize,
                        )
                        .copied()
                        .unwrap_or(Voice::SILENT),
                    LoopState::Disabled => Voice::SILENT,
                }
            }
        }
    }
    /// Get a list of this instrument's inputs
    pub fn inputs(&self) -> Vec<&str> {
        match self {
            Device::Filter { input, value, .. } => once(input)
                .chain(if let DynInput::Second(id) = value {
                    Some(id)
                } else {
                    None
                })
                .map(AsRef::as_ref)
                .collect(),
            Device::Loop { input, .. } => vec![input.as_str()],
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
