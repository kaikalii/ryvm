use std::collections::HashMap;

use serde_derive::{Deserialize, Serialize};

use crate::{Control, Letter, SampleType, SAMPLE_RATE};

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
            attack: 0.1,
            decay: 0.1,
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

#[derive(Debug, Clone, Default)]
pub struct Enveloper {
    pub adsr: ADSR,
    states: HashMap<(Letter, u8), (u32, NoteState)>,
    i: u32,
}

impl Enveloper {
    pub fn new(adsr: ADSR) -> Self {
        Enveloper {
            adsr,
            states: HashMap::new(),
            i: 0,
        }
    }
    pub fn register<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = Control>,
    {
        for control in iter {
            match control {
                Control::StartNote(l, o, v) => {
                    self.states
                        .entry((l, o))
                        .or_insert((self.i, NoteState::Pressed { velocity: v }));
                }
                Control::EndNote(l, o) => {
                    self.states.remove(&(l, o));
                    self.states.insert((l, o), (self.i, NoteState::Released));
                }
            }
        }
    }
    pub fn states(&self) -> impl Iterator<Item = (SampleType, SampleType)> + '_ {
        self.states
            .iter()
            .filter_map(move |((letter, octave), (start, state))| {
                let t = (self.i - *start) as SampleType / SAMPLE_RATE as SampleType;
                let amplitude = match state {
                    NoteState::Pressed { velocity } => {
                        let velocity = *velocity as SampleType / std::u8::MAX as SampleType;
                        if t < self.adsr.attack {
                            t / self.adsr.attack * velocity
                        } else {
                            let t_after_attack = t - self.adsr.attack;
                            if t_after_attack < self.adsr.decay {
                                (self.adsr.sustain
                                    + (1.0 - self.adsr.sustain) * t_after_attack / self.adsr.decay)
                                    * velocity
                            } else {
                                self.adsr.sustain * velocity
                            }
                        }
                    }
                    NoteState::Released => {
                        if t < self.adsr.release {
                            1.0 - t / self.adsr.release
                        } else {
                            0.0
                        }
                    }
                };
                if amplitude > 0.0 {
                    Some((letter.freq(*octave), amplitude))
                } else {
                    None
                }
            })
    }
    pub fn progress(&mut self) {
        let i = self.i;
        let release = self.adsr.release;
        self.states.retain(|_, (start, state)| match state {
            NoteState::Pressed { .. } => true,
            NoteState::Released => {
                let t = (i - *start) as SampleType / SAMPLE_RATE as SampleType;
                t < release
            }
        });
        self.i += 1;
    }
}