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

#[derive(Debug, Clone, Copy)]
enum NoteState {
    Pressed,
    Released,
}

pub type LetterOctave = (Letter, u8);
type NoteEnvelope = (u32, NoteState, u8);

/// Keeps track of the key states of an input device
/// and applies an ADSR envelope to them
#[derive(Debug, Clone, Default)]
pub struct Enveloper {
    pitch_bend: f32,
    states: HashMap<LetterOctave, Vec<NoteEnvelope>>,
    i: u32,
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
                    if !states
                        .iter()
                        .any(|(_, state, _)| matches!(state, NoteState::Pressed))
                    {
                        let new_state = (self.i, NoteState::Pressed, v);
                        states.push(new_state);
                    }
                }
                Control::NoteEnd(l, o) => {
                    let states = self.states.entry((l, o)).or_insert_with(Vec::new);
                    if let Some(i) = states
                        .iter()
                        .position(|(_, state, _)| matches!(state, NoteState::Pressed))
                    {
                        let (_, _, velocity) = states.remove(i);
                        states.push((self.i, NoteState::Released, velocity));
                    }
                }
                Control::PitchBend(pb) => self.pitch_bend = pb,
                Control::Control(..) => {}
                Control::PadStart(..) => {}
                Control::PadEnd(..) => {}
            }
        }
    }
    /// Get an iterator of frequency-amplitude pairs that are currently playing
    pub fn states(
        &self,
        sample_rate: u32,
        base_octave: i8,
        bend_range: f32,
        adsr: ADSR<f32>,
    ) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.states
            .iter()
            .flat_map(|(k, states)| states.iter().map(move |state| (k, state)))
            .filter_map(move |((letter, octave), (start, state, velocity))| {
                let t = (self.i - *start) as f32 / sample_rate as f32;
                let velocity = f32::from(*velocity) / 127.0;
                let amplitude = match state {
                    NoteState::Pressed => {
                        if t < adsr.attack {
                            t / adsr.attack * velocity
                        } else {
                            let t_after_attack = t - adsr.attack;
                            if t_after_attack < adsr.decay {
                                (adsr.sustain
                                    + (1.0 - adsr.sustain) * (1.0 - t_after_attack / adsr.decay))
                                    * velocity
                            } else {
                                adsr.sustain * velocity
                            }
                        }
                    }
                    NoteState::Released => {
                        if t < adsr.release {
                            adsr.sustain * velocity * (1.0 - t / adsr.release)
                        } else {
                            0.0
                        }
                    }
                };
                if amplitude > 0.0 {
                    Some((
                        letter.freq((i16::from(*octave) + i16::from(base_octave)).max(0) as u8)
                            * 2_f32.powf(self.pitch_bend * bend_range / 12.0),
                        amplitude,
                    ))
                } else {
                    None
                }
            })
    }
    /// Progress the enveloper to the next frame
    pub fn progress(&mut self, sample_rate: u32, release: f32) {
        let i = self.i;
        for states in self.states.values_mut() {
            states.retain(|(start, state, _)| match state {
                NoteState::Pressed { .. } => true,
                NoteState::Released => {
                    let t = (i - *start) as f32 / sample_rate as f32;
                    t < release
                }
            });
        }
        self.states.retain(|_, states| !states.is_empty());
        self.i += 1;
    }
}
