use std::collections::HashMap;

use crate::{Control, Letter};

/// A set of values defining an attack-decay-sustain-release envelope
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ADSR<T> {
    pub attack: T,
    pub decay: T,
    pub sustain: T,
    pub release: T,
}

impl<T> ADSR<T> {
    pub fn map<F, U>(&self, mut f: F) -> ADSR<U>
    where
        F: FnMut(&T) -> U,
    {
        ADSR {
            attack: f(&self.attack),
            decay: f(&self.decay),
            sustain: f(&self.sustain),
            release: f(&self.release),
        }
    }
    pub fn map_or_default<F>(&self, mut f: F) -> ADSR<f32>
    where
        F: FnMut(&T) -> Option<f32>,
    {
        ADSR {
            attack: f(&self.attack).unwrap_or_else(|| ADSR::default().attack),
            decay: f(&self.decay).unwrap_or_else(|| ADSR::default().decay),
            sustain: f(&self.sustain).unwrap_or_else(|| ADSR::default().sustain),
            release: f(&self.release).unwrap_or_else(|| ADSR::default().release),
        }
    }
}

impl Default for ADSR<f32> {
    fn default() -> Self {
        ADSR {
            attack: 0.05,
            decay: 0.05,
            sustain: 0.7,
            release: 0.1,
        }
    }
}

pub type LetterOctave = (Letter, u8);

#[derive(Debug, Clone, Copy)]
enum EnvelopeState {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

impl EnvelopeState {
    fn is_pressed(self) -> bool {
        matches!(self, Self::Attack | Self::Decay | Self::Sustain)
    }
}

#[derive(Debug, Clone, Copy)]
struct NoteEnvelope {
    state: EnvelopeState,
    velocity: u8,
    amplitude: f32,
}

/// Keeps track of the key states of an input device
/// and applies an ADSR envelope to them
#[derive(Debug, Clone, Default)]
pub struct Enveloper {
    pitch_bend: f32,
    states: HashMap<LetterOctave, Vec<NoteEnvelope>>,
}

impl Enveloper {
    /// Register some controls
    pub fn register<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = Control>,
    {
        for control in iter {
            match control {
                Control::NoteStart(l, o, v) => {
                    let states = self.states.entry((l, o)).or_insert_with(Vec::new);
                    if !states.iter().any(|ne| ne.state.is_pressed()) {
                        let new_ne = NoteEnvelope {
                            state: EnvelopeState::Attack,
                            velocity: v,
                            amplitude: 0.0,
                        };
                        states.push(new_ne);
                    }
                }
                Control::NoteEnd(l, o) => {
                    let states = self.states.entry((l, o)).or_insert_with(Vec::new);
                    if let Some(ne) = states.iter_mut().find(|ne| ne.state.is_pressed()) {
                        ne.state = EnvelopeState::Release;
                    }
                }
                Control::PitchBend(pb) => self.pitch_bend = pb,
                _ => {}
            }
        }
    }
    /// Get an iterator of frequency-amplitude pairs that are currently playing
    pub fn states(
        &self,
        base_octave: i8,
        bend_range: f32,
    ) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.states
            .iter()
            .flat_map(|(k, states)| states.iter().map(move |state| (k, state)))
            .filter_map(move |((letter, octave), ne)| {
                if ne.amplitude > 0.0 {
                    Some((
                        letter.freq((i16::from(*octave) + i16::from(base_octave)).max(0) as u8)
                            * 2_f32.powf(self.pitch_bend * bend_range / 12.0),
                        ne.amplitude,
                    ))
                } else {
                    None
                }
            })
    }
    /// Progress the enveloper to the next frame
    pub fn progress(&mut self, sample_rate: u32, adsr: ADSR<f32>) {
        for states in self.states.values_mut() {
            for ne in states.iter_mut() {
                let velocity = f32::from(ne.velocity) / 127.0;
                match ne.state {
                    EnvelopeState::Attack => {
                        let slope = velocity / adsr.attack;
                        assert!(slope > 0.0);
                        ne.amplitude += slope / sample_rate as f32;
                        if ne.amplitude >= velocity {
                            ne.state = EnvelopeState::Decay;
                        }
                    }
                    EnvelopeState::Decay => {
                        let slope = (adsr.sustain * velocity - velocity) / adsr.decay;
                        assert!(slope < 0.0);
                        ne.amplitude += slope / sample_rate as f32;
                        if ne.amplitude <= adsr.sustain * velocity {
                            ne.state = EnvelopeState::Sustain;
                        }
                    }
                    EnvelopeState::Sustain => {}
                    EnvelopeState::Release => {
                        let slope = (0.0 - adsr.sustain * velocity) / adsr.release;
                        assert!(slope < 0.0);
                        ne.amplitude += slope / sample_rate as f32;
                        if ne.amplitude <= 0.0 {
                            ne.state = EnvelopeState::Done;
                        }
                    }
                    EnvelopeState::Done => panic!("EnvelopeState::Done not purged"),
                }
            }
            states.retain(|ne| !matches!(ne.state, EnvelopeState::Done));
        }
        self.states.retain(|_, states| !states.is_empty());
    }
}
