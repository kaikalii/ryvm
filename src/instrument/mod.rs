mod instruments;
mod parts;
pub use {instruments::*, parts::*};

use std::{
    collections::HashMap,
    f32::consts::PI,
    iter::{once, repeat},
    sync::Arc,
};

use serde_derive::{Deserialize, Serialize};

use crate::{
    default_voices, is_default_voices, mix, Channels, CloneLock, Control, DynInput, Enveloper,
    Frame, FrameCache, InstrId, SampleType, Sampling, Voice, ADSR, SAMPLE_RATE,
};

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
        waves: CloneLock<Channels<Vec<u32>>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        octave: Option<u8>,
        #[serde(default)]
        adsr: ADSR,
        #[serde(skip)]
        envelopers: CloneLock<Channels<Enveloper>>,
    },
    Mixer(HashMap<InstrId, Balance>),
    #[cfg(feature = "keyboard")]
    Keyboard,
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
    Script {
        args: Vec<String>,
        commands: Vec<(bool, Vec<String>)>,
    },
}

impl Instrument {
    /// Create a new wave instrument
    pub fn wave(
        input: InstrId,
        form: WaveForm,
        octave: Option<u8>,
        adsr: ADSR,
        voices: u32,
    ) -> Self {
        Instrument::Wave {
            input: input.into(),
            form,
            voices,
            octave,
            adsr,
            waves: CloneLock::new(Channels::new()),
            envelopers: CloneLock::new(Channels::new()),
        }
    }
    /// Check if the instrument is an input device
    pub fn is_input_device(&self) -> bool {
        match self {
            #[cfg(feature = "keyboard")]
            Instrument::Keyboard { .. } => true,
            _ => false,
        }
    }
    /// Get the next frame(s) from the instrument
    pub fn next(
        &self,
        cache: &mut FrameCache,
        instruments: &Instruments,
        #[cfg_attr(not(feature = "keyboard"), allow(unused_variables))] my_id: InstrId,
    ) -> Channels {
        match self {
            // Numbers
            Instrument::Number(n) => Frame::from(Voice::mono(*n)).into(),
            // Waves
            Instrument::Wave {
                input,
                form,
                voices,
                octave,
                adsr,
                waves,
                envelopers,
                ..
            } => {
                let mut envelopers = envelopers.lock();
                let res = instruments
                    .next_from(&*input, cache)
                    .id_map(|ch_id, input_frame| {
                        // Closure for building the wave
                        let build_wave = |freq: SampleType, amp: SampleType, i: &mut u32| {
                            if freq == 0.0 {
                                return Voice::mono(0.0);
                            }
                            // spc = samples per cycle
                            let spc = SAMPLE_RATE as SampleType / freq;
                            let t = *i as SampleType / spc;
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
                                * amp;
                            *i = (*i + 1) % spc as u32;
                            Voice::mono(s)
                        };
                        // Ensure that waves is initialized
                        let mut waves = waves.lock();
                        let waves = waves
                            .entry(ch_id.clone())
                            .or_insert_with(|| vec![0; *voices as usize]);
                        waves.resize(*voices as usize, 0);
                        // Match on input frame type to get a list of voice/balance pairs to mix
                        let mix_inputs: Vec<(Voice, Balance)> = match input_frame {
                            // For voices build the wave based on freqency and amplitude
                            Frame::Voice(voice) => once((voice.left, 1.0))
                                .zip(waves)
                                .map(|((freq, amp), i)| build_wave(freq, amp, i))
                                .zip(repeat(Balance::default()))
                                .collect(),
                            // For controls, regitser with the enveloper to get frequency and aplitude
                            Frame::Controls(controls) => {
                                let enveloper = envelopers
                                    .entry(ch_id.clone())
                                    .or_insert_with(Enveloper::default);
                                enveloper.register(controls.iter().copied());
                                enveloper
                                    .states(octave.unwrap_or(3), *adsr)
                                    .zip(waves)
                                    .map(|((freq, amp), i)| build_wave(freq, amp, i))
                                    .zip(repeat(Balance::default()))
                                    .collect()
                            }
                            // For empty frames, use the enveloper to get frequency and aplitude
                            Frame::None => {
                                if let Some(enveloper) = envelopers.get_mut(ch_id) {
                                    enveloper
                                        .states(octave.unwrap_or(3), *adsr)
                                        .zip(waves)
                                        .map(|((freq, amp), i)| build_wave(freq, amp, i))
                                        .zip(repeat(Balance::default()))
                                        .collect()
                                } else {
                                    Vec::new()
                                }
                            }
                        };
                        mix(&mix_inputs)
                    });
                for enveloper in envelopers.values_mut() {
                    enveloper.progress(adsr.release);
                }
                res
            }
            // Mixers
            Instrument::Mixer(list) => {
                // Simply mix all inputs
                let mut voices = Vec::new();
                for (id, bal) in list {
                    for frame in instruments.next_from(id, cache).frames() {
                        voices.push((frame.voice(), *bal));
                    }
                }
                mix(&voices).into()
            }
            // Keyboards
            #[cfg(feature = "keyboard")]
            Instrument::Keyboard => {
                if instruments
                    .current_keyboard
                    .as_ref()
                    .map(|kb_id| kb_id == &my_id)
                    .unwrap_or(false)
                {
                    // Turn controls from keyboard into control frames
                    Frame::controls(
                        instruments.keyboard(|kb| kb.controls().drain().collect::<Vec<_>>()),
                    )
                    .into()
                } else {
                    Default::default()
                }
            }
            // Drum Machine
            Instrument::DrumMachine(samplings) => {
                if samplings.is_empty() {
                    return Channels::default();
                }
                let mut voices = Vec::new();
                // Get the sample index for the current measure
                let ix = instruments.measure_i();
                // For all samplings...
                for sampling in samplings {
                    let frames_per_sub = instruments.frames_per_measure() as SampleType
                        / sampling.beat.0.len() as SampleType;
                    // If the sample data is loaded...
                    if let Some(res) = instruments.sample_bank.get(&sampling.path).finished() {
                        if let Ok(sample) = &*res {
                            let samples = &sample.samples();
                            // Check each beat in the sampling to see if it should be playing
                            for b in 0..sampling.beat.0.len() {
                                // Determine the beat's start index
                                let start = (frames_per_sub * b as f32) as u32;
                                // If the sampling is playing and the current index is after its start...
                                if sampling.beat.0[b as usize] && ix >= start {
                                    // And before its end
                                    let si = (ix - start) as usize;
                                    if si < samples.len() {
                                        // Add the voice from the sample for this measure index to the list to be mixed
                                        voices.push((samples[si], Balance::default()));
                                    }
                                }
                            }
                        }
                    }
                }
                mix(&voices).into()
            }
            // Loops
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

                let adjust_i = |i: u32| (i as u64 * actual_fpl as u64 / ideal_fpl as u64) as u32;
                // The index of the loop's current sample with adjusting for tempo changes
                let raw_loop_i = instruments.i() % ideal_fpl;
                // Calculate the index of the loop's current sample adjusting for changes in tempo
                let loop_i = adjust_i(raw_loop_i);
                if loop_i == 0 {
                    for frame in frames.iter_mut() {
                        frame.new = false;
                    }
                }
                // Get input frame
                let input_channels = instruments.next_from(&*input, cache);
                let frame_is_some = || {
                    input_channels
                        .primary()
                        .map(Frame::is_some)
                        .unwrap_or(false)
                };
                if *recording && frame_is_some() {
                    // Record if recording and there is input
                    frames[loop_i as usize] = LoopFrame {
                        frame: input_channels.primary().cloned().unwrap_or_default(),
                        new: true,
                    };
                    // Go backward and set all old frames to new and empty
                    for i in (0..(loop_i as usize)).rev() {
                        if frames[i].new {
                            break;
                        } else {
                            frames[i] = LoopFrame {
                                frame: Frame::None,
                                new: true,
                            }
                        }
                    }
                    // The loop itself is silent while recording
                    Frame::from(Control::EndAllNotes).into()
                } else if *playing {
                    // Play the loop
                    // Find all controls, either for the current loop index or for
                    // ones would be skipped because of tempo changes
                    let controls: Vec<Control> = (loop_i..adjust_i(raw_loop_i + 1))
                        .flat_map(|i| {
                            if let Frame::Controls(controls) = &frames[i as usize].frame {
                                controls.clone()
                            } else {
                                Vec::new()
                            }
                        })
                        .collect();
                    if controls.is_empty() {
                        // If there were no controls found, simply output the loop's stored frame
                        frames[loop_i as usize].frame.clone()
                    } else {
                        Frame::Controls(controls)
                    }
                    .into()
                } else {
                    // The loop is silent if it is not playing
                    Frame::from(Control::EndAllNotes).into()
                }
            }
            // Filters
            Instrument::Filter { input, value, avgs } => {
                // Determine the factor used to maintain the running average
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
                // Get the input channels
                let input_channels = instruments.next_from(input, cache);
                let mut avgs = avgs.lock();
                // Apply the filter for each input channel
                input_channels.id_map(|id, frame| {
                    let avg = avgs.entry(id.clone()).or_insert_with(|| frame.voice());
                    avg.left = avg.left * (1.0 - avg_factor) + frame.left() * avg_factor;
                    avg.right = avg.right * (1.0 - avg_factor) + frame.right() * avg_factor;
                    Frame::from(*avg)
                })
            }
            // Script are not actual instruments, so they do not output frames
            Instrument::Script { .. } => Channels::new(),
        }
    }
    /// Get a list of this instrument's inputs
    pub fn inputs(&self) -> Vec<&InstrId> {
        match self {
            Instrument::Wave { input, .. } => vec![&input],
            Instrument::Mixer(inputs) => inputs.keys().collect(),
            Instrument::Filter { input, value, .. } => once(input)
                .chain(if let DynInput::Id(id) = value {
                    Some(id)
                } else {
                    None
                })
                .collect(),
            _ => Vec::new(),
        }
    }
    /// Replace any of this instrument's inputs that match the old id with the new one
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
}
