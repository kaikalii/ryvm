use std::collections::HashMap;

use serde_derive::{Deserialize, Serialize};

use crate::{Control, Letter, SampleType, SAMPLE_RATE};

/// A set of values defining an attack-decay-sustain-release envelope
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ADSR {
    pub attack: SampleType,
    pub decay: SampleType,
    pub sustain: SampleType,
    pub release: SampleType,
}

impl Default for ADSR {
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
    Pressed { velocity: u8 },
    Released,
}

/// Keeps track of the key states of an input device
/// and applies an ADSR envelope to them
#[derive(Debug, Clone, Default)]
pub struct Enveloper {
    states: HashMap<(Letter, u8), Vec<(u32, NoteState)>>,
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
                Control::StartNote(l, o, v) => {
                    let states = self.states.entry((l, o)).or_insert_with(Vec::new);
                    if !states
                        .iter()
                        .any(|(_, state)| matches!(state, NoteState::Pressed{..}))
                    {
                        let new_state = (self.i, NoteState::Pressed { velocity: v });
                        states.push(new_state);
                    }
                }
                Control::EndNote(l, o) => {
                    let states = self.states.entry((l, o)).or_insert_with(Vec::new);
                    if let Some(i) = states
                        .iter()
                        .position(|(_, state)| matches!(state, NoteState::Pressed{..}))
                    {
                        states.remove(i);
                        states.push((self.i, NoteState::Released));
                    }
                }
                Control::EndAllNotes => self.states.clear(),
            }
        }
    }
    /// Get an iterator of frequency-amplitude pairs that are currently playing
    pub fn states(
        &self,
        base_octave: u8,
        adsr: ADSR,
    ) -> impl Iterator<Item = (SampleType, SampleType)> + '_ {
        self.states
            .iter()
            .flat_map(|(k, states)| states.iter().map(move |state| (k, state)))
            .filter_map(move |((letter, octave), (start, state))| {
                let t = (self.i - *start) as SampleType / SAMPLE_RATE as SampleType;
                let amplitude = match state {
                    NoteState::Pressed { velocity } => {
                        let velocity = *velocity as SampleType / std::u8::MAX as SampleType;
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
                            adsr.sustain * (1.0 - t / adsr.release)
                        } else {
                            0.0
                        }
                    }
                };
                if amplitude > 0.0 {
                    Some((letter.freq(*octave + base_octave), amplitude))
                } else {
                    None
                }
            })
    }
    /// Progress the enveloper to the next frame
    pub fn progress(&mut self, release: SampleType) {
        let i = self.i;
        for states in self.states.values_mut() {
            states.retain(|(start, state)| match state {
                NoteState::Pressed { .. } => true,
                NoteState::Released => {
                    let t = (i - *start) as SampleType / SAMPLE_RATE as SampleType;
                    t < release
                }
            });
        }
        self.states.retain(|_, states| !states.is_empty());
        self.i += 1;
    }
}
