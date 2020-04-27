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
    adjust_i, mix, Channel, ChannelId, Channels, CloneCell, CloneLock, Control, DynInput,
    Enveloper, Frame, FrameCache, InstrId, Letter, Voice, ADSR,
};

#[derive(Debug, Clone)]
pub struct Wave {
    input: InstrId,
    form: WaveForm,
    voices: u32,
    waves: CloneLock<HashMap<ChannelId, Vec<u32>>>,
    octave: Option<i8>,
    adsr: ADSR,
    envelopers: CloneLock<HashMap<ChannelId, Enveloper>>,
}

#[derive(Debug, Clone)]
pub struct DrumMachine {
    samples: Vec<PathBuf>,
    input: InstrId,
    samplings: CloneLock<HashMap<ChannelId, Vec<ActiveSampling>>>,
}

/// An instrument for producing sounds
#[derive(Debug, Clone)]
pub enum Instrument {
    Wave(Box<Wave>),
    Mixer(HashMap<InstrId, Balance>),
    #[cfg(feature = "keyboard")]
    Keyboard,
    Midi {
        port: usize,
    },
    DrumMachine(Box<DrumMachine>),
    Loop {
        input: InstrId,
        recording: bool,
        playing: bool,
        tempo: f32,
        last_frames: CloneLock<BTreeMap<u32, Vec<Control>>>,
        frames: CloneLock<BTreeMap<u32, Vec<Control>>>,
        size: f32,
    },
    InitialLoop {
        input: InstrId,
        frames: Option<CloneLock<BTreeMap<u32, Vec<Control>>>>,
        start_i: CloneCell<Option<u32>>,
    },
    Filter {
        input: InstrId,
        value: DynInput,
        avgs: Arc<CloneLock<HashMap<ChannelId, Voice>>>,
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
        Instrument::Wave(Box::new(Wave {
            input,
            form,
            voices,
            octave,
            adsr,
            waves: CloneLock::new(HashMap::new()),
            envelopers: CloneLock::new(HashMap::new()),
        }))
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
            // Waves
            Instrument::Wave(wave) => {
                let Wave {
                    input,
                    form,
                    voices,
                    octave,
                    adsr,
                    waves,
                    envelopers,
                    ..
                } = &**wave;
                let mut envelopers = envelopers.lock();
                let res = instruments
                    .next_from(&*input, cache)
                    .id_map(|ch_id, input_channel| {
                        // Closure for building the wave
                        let build_wave = |freq: f32, amp: f32, i: &mut u32| {
                            if freq == 0.0 {
                                return Voice::mono(0.0);
                            }
                            // spc = samples per cycle
                            let spc = instruments.sample_rate as f32 / freq;
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
                        let mix_inputs: Vec<(Voice, Balance)> = match &input_channel.frame {
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
                                    .states(instruments.sample_rate, octave.unwrap_or(0), *adsr)
                                    .zip(waves)
                                    .map(|((freq, amp), i)| build_wave(freq, amp, i))
                                    .zip(repeat(Balance::default()))
                                    .collect()
                            }
                            // For empty frames, use the enveloper to get frequency and aplitude
                            Frame::None => {
                                if let Some(enveloper) = envelopers.get_mut(ch_id) {
                                    enveloper
                                        .states(instruments.sample_rate, octave.unwrap_or(0), *adsr)
                                        .zip(waves)
                                        .map(|((freq, amp), i)| build_wave(freq, amp, i))
                                        .zip(repeat(Balance::default()))
                                        .collect()
                                } else {
                                    Vec::new()
                                }
                            }
                        };
                        input_channel.with_frame(mix(&mix_inputs))
                    });
                for enveloper in envelopers.values_mut() {
                    enveloper.progress(instruments.sample_rate, adsr.release);
                }
                res
            }
            // Mixers
            Instrument::Mixer(list) => {
                // Simply mix all inputs
                let mut input_channels = HashMap::new();
                for (id, bal) in list {
                    for (ch_id, channel) in instruments.next_from(id, cache).iter() {
                        input_channels
                            .entry(ch_id.clone())
                            .or_insert_with(Vec::new)
                            .push((channel.clone(), *bal));
                    }
                }
                input_channels
                    .into_iter()
                    .map(|(id, channels)| {
                        let mut validated = false;
                        let mut voices = Vec::new();
                        for (ch, bal) in channels {
                            voices.push((ch.frame.voice(), bal));
                            validated = validated || ch.validated;
                        }
                        (
                            id,
                            Channel {
                                frame: mix(&voices),
                                validated,
                            },
                        )
                    })
                    .collect()
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
            // Midi controller
            Instrument::Midi { port } => {
                if instruments
                    .default_midi
                    .as_ref()
                    .map(|midi_id| midi_id == &my_id)
                    .unwrap_or(false)
                {
                    if let Some(midi) = instruments.midis.get(port) {
                        let channels = Channels::split_controls(midi.controls());
                        if instruments.debug_live {
                            println!("{:?}", channels);
                        }
                        channels
                    } else {
                        Channels::default()
                    }
                } else {
                    Channels::default()
                }
            }
            // Drum Machine
            Instrument::DrumMachine(drums) => {
                let channels = instruments.next_from(&drums.input, cache);
                // if instruments.debug_live {
                //     println!("{:?}", channels);
                // }
                channels.id_map(|ch_id, channel| {
                    let mut voices = Vec::new();
                    let mut samplings = drums.samplings.lock();
                    let samplings = samplings.entry(ch_id.clone()).or_insert_with(Vec::new);
                    // Register controls from input frame
                    if let Frame::Controls(controls) = &channel.frame {
                        for &control in controls {
                            if let Control::PadStart(l, o, v) = control {
                                let min_index = Letter::C.to_u8(3);
                                let index = (l.to_u8(o).max(min_index) - min_index) as usize;
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
                    // Add manual samples to voies
                    for ms in (0..samplings.len()).rev() {
                        let ActiveSampling { index, i, velocity } = &mut samplings[ms];
                        if let Some(res) = instruments
                            .sample_bank
                            .get(&drums.samples[*index])
                            .finished()
                        {
                            if let Ok(sample) = &*res {
                                if *i < sample.len(instruments.sample_rate) {
                                    let voice =
                                        *sample.voice(*i, instruments.sample_rate) * *velocity;
                                    voices.push((voice, Balance::default()));
                                    *i += 1;
                                } else {
                                    samplings.remove(ms);
                                }
                            }
                        }
                    }
                    channel.with_frame(mix(&voices))
                })
            }
            // Loops
            Instrument::InitialLoop {
                input,
                frames,
                start_i,
            } => {
                for input_channel in instruments.next_from(&*input, cache).values() {
                    if let Frame::Controls(controls) = &input_channel.frame {
                        if start_i.load().is_none() {
                            start_i.store(Some(instruments.i()));
                            println!("Started recording {}", my_id)
                        }
                        if let Some(start_i) = start_i.load() {
                            frames
                                .as_ref()
                                .unwrap()
                                .lock()
                                .insert(instruments.i() - start_i, controls.clone());
                        }
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
                size,
            } => {
                let mut frames = frames.lock();
                let input_channels = instruments.next_from(&*input, cache);
                let LoopMaster {
                    start_i, period, ..
                } = instruments
                    .loop_master
                    .expect("logic error: Loop is running with no master set");
                let period = (period as f32 * *size) as u32;

                // The index of the loop's current sample without adjusting for tempo changes
                let raw_loop_i = instruments.i() - start_i;
                // Calculate the index of the loop's current sample adjusting for changes in tempo
                let loop_i = adjust_i(raw_loop_i, *tempo, instruments.tempo);

                if *recording {
                    if raw_loop_i % period == 0 {
                        let mut last_frames = last_frames.lock();
                        last_frames.clear();
                        last_frames.append(&mut *frames);
                    }
                    // Record if recording and there is input
                    let frame_i = loop_i % period;
                    for channel in input_channels.values() {
                        if let Frame::Controls(controls) = &channel.frame {
                            frames.insert(frame_i, controls.clone());
                        }
                    }
                    // The loop itself is silent while recording
                    Channels::end_all_notes()
                } else if *playing {
                    // Play the loop
                    // Find all controls, either for the current loop index or for
                    // ones would be skipped because of tempo changes
                    let controls: Vec<Control> = (loop_i
                        ..adjust_i(raw_loop_i + 1, *tempo, instruments.tempo))
                        .flat_map(|i| {
                            if let Some(controls) = frames.get(&(i % period)) {
                                controls.clone()
                            } else {
                                Vec::new()
                            }
                        })
                        .collect();
                    let frame = if controls.is_empty() {
                        // If there were no controls found, simply output the loop's stored frame
                        frames
                            .get(&(loop_i % period))
                            .cloned()
                            .map(Frame::controls)
                            .unwrap_or(Frame::None)
                    } else {
                        Frame::Controls(controls)
                    };
                    (ChannelId::Dummy, frame.unvalidated()).into()
                } else {
                    // The loop is silent if it is not playing
                    Channels::end_all_notes()
                }
            }
            // Filters
            Instrument::Filter { input, value, avgs } => {
                // Determine the factor used to maintain the running average
                let avg_factor = match value {
                    DynInput::Num(f) => *f,
                    DynInput::Id(_) => unimplemented!(),
                }
                .powf(2.0);
                // Get the input channels
                let input_channels = instruments.next_from(input, cache);
                let mut avgs = avgs.lock();
                // Apply the filter for each input channel
                input_channels.id_map(|id, channel| {
                    let avg = avgs
                        .entry(id.clone())
                        .or_insert_with(|| channel.frame.voice());
                    avg.left = avg.left * (1.0 - avg_factor) + channel.frame.left() * avg_factor;
                    avg.right = avg.right * (1.0 - avg_factor) + channel.frame.right() * avg_factor;
                    channel.with_frame(Frame::from(*avg))
                })
            }
            // Script are not actual instruments, so they do not output frames
            Instrument::Script { .. } => Channels::new(),
        }
        .validate(&my_id, instruments)
    }
    /// Get a list of this instrument's inputs
    pub fn inputs(&self) -> Vec<&InstrId> {
        match self {
            Instrument::Wave(wave) => vec![&wave.input],
            Instrument::Mixer(inputs) => inputs.keys().collect(),
            Instrument::Filter { input, value, .. } => once(input)
                .chain(if let DynInput::Id(id) = value {
                    Some(id)
                } else {
                    None
                })
                .collect(),
            Instrument::DrumMachine(drums) => vec![&drums.input],
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
            Instrument::Wave(wave) => replace(&mut wave.input),
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
