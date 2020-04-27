mod instruments;
mod parts;
pub use {instruments::*, parts::*};

use std::{
    collections::{BTreeMap, HashMap},
    f32::consts::PI,
    iter::{once, repeat},
    path::PathBuf,
    sync::Arc,
};

use crate::{
    adjust_i, mix, Channels, CloneCell, CloneLock, Control, DynInput, Enveloper, Frame, FrameCache,
    InstrId, Letter, SampleType, Voice, ADSR, SAMPLE_RATE,
};

/// An instrument for producing sounds
#[derive(Debug, Clone)]
pub enum Instrument {
    Number(SampleType),
    Wave {
        input: InstrId,
        form: WaveForm,
        voices: u32,
        waves: CloneLock<Channels<Vec<u32>>>,
        octave: Option<i8>,
        adsr: ADSR,
        envelopers: CloneLock<Channels<Enveloper>>,
    },
    Mixer(HashMap<InstrId, Balance>),
    #[cfg(feature = "keyboard")]
    Keyboard,
    Midi {
        port: usize,
    },
    DrumMachine {
        samples: Vec<PathBuf>,
        input: InstrId,
        manual_samples: CloneLock<Channels<Vec<ActiveSampling>>>,
    },
    Loop {
        input: InstrId,
        recording: bool,
        playing: bool,
        tempo: SampleType,
        last_frames: CloneLock<BTreeMap<u32, Vec<Control>>>,
        frames: CloneLock<BTreeMap<u32, Vec<Control>>>,
        start_i: CloneCell<Option<u32>>,
        period: u32,
    },
    InitialLoop {
        input: InstrId,
        frames: Option<CloneLock<BTreeMap<u32, Vec<Control>>>>,
        start_i: CloneCell<Option<u32>>,
    },
    Filter {
        input: InstrId,
        value: DynInput,
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
        octave: Option<i8>,
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
            Instrument::Midi { .. } => true,
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
                                    .states(octave.unwrap_or(0), *adsr)
                                    .zip(waves)
                                    .map(|((freq, amp), i)| build_wave(freq, amp, i))
                                    .zip(repeat(Balance::default()))
                                    .collect()
                            }
                            // For empty frames, use the enveloper to get frequency and aplitude
                            Frame::None => {
                                if let Some(enveloper) = envelopers.get_mut(ch_id) {
                                    enveloper
                                        .states(octave.unwrap_or(0), *adsr)
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
                    Channels::empty_primary()
                }
            }
            Instrument::Midi { port } => {
                if instruments
                    .current_midi
                    .as_ref()
                    .map(|midi_id| midi_id == &my_id)
                    .unwrap_or(false)
                {
                    if let Some(midi) = instruments.midis.get(port) {
                        Frame::controls(midi.controls()).into()
                    } else {
                        Channels::empty_primary()
                    }
                } else {
                    Channels::empty_primary()
                }
            }
            // Drum Machine
            Instrument::DrumMachine {
                samples,
                input,
                manual_samples,
            } => {
                let channels = instruments.next_from(input, cache);
                channels.id_map(|ch_id, frame| {
                    let mut voices = Vec::new();
                    let mut manual_samples = manual_samples.lock();
                    let manual_samples =
                        manual_samples.entry(ch_id.clone()).or_insert_with(Vec::new);
                    // Register controls from input frame
                    if let Frame::Controls(controls) = frame {
                        for &control in controls {
                            if let Control::PadStart(l, o, v) = control {
                                let min_index = Letter::C.to_u8(3);
                                let index = (l.to_u8(o).max(min_index) - min_index) as usize;
                                if index < samples.len() {
                                    manual_samples.push(ActiveSampling {
                                        index,
                                        i: 0,
                                        velocity: v as SampleType / 127.0,
                                    });
                                }
                            }
                        }
                    }
                    // Add manual samples to voies
                    for ms in (0..manual_samples.len()).rev() {
                        let ActiveSampling { index, i, velocity } = &mut manual_samples[ms];
                        if let Some(res) = instruments.sample_bank.get(&samples[*index]).finished()
                        {
                            if let Ok(sample) = &*res {
                                if *i < sample.samples().len() {
                                    let voice = sample.samples()[*i] * *velocity;
                                    voices.push((voice, Balance::default()));
                                    *i += 1;
                                } else {
                                    manual_samples.remove(ms);
                                }
                            }
                        }
                    }

                    mix(&voices).into()
                })
            }
            // Loops
            Instrument::InitialLoop {
                input,
                frames,
                start_i,
            } => {
                let input_frame = instruments
                    .next_from(&*input, cache)
                    .primary()
                    .cloned()
                    .unwrap_or_default();
                if let Frame::Controls(controls) = input_frame {
                    if start_i.load().is_none() {
                        start_i.store(Some(instruments.i()));
                        println!("Started recording {}", my_id)
                    }
                    if let Some(start_i) = start_i.load() {
                        frames
                            .as_ref()
                            .unwrap()
                            .lock()
                            .insert(instruments.i() - start_i, controls);
                    }
                }
                Channels::default()
            }
            Instrument::Loop {
                input,
                recording,
                playing,
                tempo,
                frames,
                last_frames,
                start_i,
                period,
            } => {
                let mut frames = frames.lock();
                let input_channels = instruments.next_from(&*input, cache);
                // Get input frames
                let frame_is_some = input_channels
                    .primary()
                    .map(Frame::is_some)
                    .unwrap_or(false);
                // Set start_i if not set
                if start_i.load().is_none() && frame_is_some {
                    start_i.store(Some(instruments.i()));
                    println!("Started recording {}", my_id);
                }
                let start_i = if let Some(i) = start_i.load() {
                    i
                } else {
                    return Channels::default();
                };

                // The index of the loop's current sample without adjusting for tempo changes
                let raw_loop_i = instruments.i() - start_i;
                // Calculate the index of the loop's current sample adjusting for changes in tempo
                let loop_i = adjust_i(raw_loop_i, *tempo, instruments.tempo);

                if *recording {
                    if raw_loop_i % *period == 0 {
                        last_frames.lock().append(&mut *frames);
                    }
                    // Record if recording and there is input
                    let frame = input_channels.primary().cloned().unwrap_or_default();
                    let frame_i = loop_i % *period;
                    if let Frame::Controls(controls) = frame {
                        frames.insert(frame_i, controls);
                    }
                    // The loop itself is silent while recording
                    Frame::from(Control::EndAllNotes).into()
                } else if *playing {
                    // Play the loop
                    // Find all controls, either for the current loop index or for
                    // ones would be skipped because of tempo changes
                    let controls: Vec<Control> = (loop_i
                        ..adjust_i(raw_loop_i + 1, *tempo, instruments.tempo))
                        .flat_map(|i| {
                            if let Some(controls) = frames.get(&(i % *period)) {
                                controls.clone()
                            } else {
                                Vec::new()
                            }
                        })
                        .collect();
                    if controls.is_empty() {
                        // If there were no controls found, simply output the loop's stored frame
                        frames
                            .get(&(loop_i % *period))
                            .cloned()
                            .map(Frame::controls)
                            .unwrap_or(Frame::None)
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
            Instrument::Wave { input, .. } => vec![input],
            Instrument::Mixer(inputs) => inputs.keys().collect(),
            Instrument::Filter { input, value, .. } => once(input)
                .chain(if let DynInput::Id(id) = value {
                    Some(id)
                } else {
                    None
                })
                .collect(),
            Instrument::DrumMachine { input, .. } => vec![input],
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
