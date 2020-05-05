use std::collections::HashMap;

use ryvm_spec::DynamicValue;

use crate::Control;

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
            attack: f(&self.attack).unwrap_or(ADSR::const_default().attack),
            decay: f(&self.decay).unwrap_or(ADSR::const_default().decay),
            sustain: f(&self.sustain).unwrap_or(ADSR::const_default().sustain),
            release: f(&self.release).unwrap_or(ADSR::const_default().release),
        }
    }
}

impl ADSR<f32> {
    pub const fn const_default() -> Self {
        ADSR {
            attack: 0.05,
            decay: 0.05,
            sustain: 0.7,
            release: 0.1,
        }
    }
}

impl Default for ADSR<f32> {
    fn default() -> Self {
        ADSR::const_default()
    }
}

impl ADSR<DynamicValue> {
    pub fn inputs(&self) -> impl Iterator<Item = &str> {
        self.attack
            .input()
            .into_iter()
            .chain(self.decay.input())
            .chain(self.sustain.input())
            .chain(self.release.input())
    }
}

#[derive(Debug, Clone, Copy)]
enum EnvelopeState {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

#[derive(Debug, Clone, Copy)]
struct NoteEnvelope {
    note: u8,
    state: EnvelopeState,
    velocity: u8,
    amplitude: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct EnvelopeFrame {
    pub note: u8,
    pub amplitude: f32,
    pub pitch_bend: f32,
}

/// Keeps track of the key states of an input device
/// and applies an ADSR envelope to them
#[derive(Debug, Clone, Default)]
pub struct Enveloper {
    pitch_bend: f32,
    envelopes: HashMap<u64, NoteEnvelope>,
}

impl Enveloper {
    /// Register some controls
    pub fn register<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = Control>,
    {
        for control in iter {
            match control {
                Control::NoteStart(id, n, v) => {
                    self.envelopes.insert(
                        id,
                        NoteEnvelope {
                            note: n,
                            state: EnvelopeState::Attack,
                            velocity: v,
                            amplitude: 0.0,
                        },
                    );
                }
                Control::NoteEnd(id, _) => {
                    if let Some(ne) = self.envelopes.get_mut(&id) {
                        ne.state = EnvelopeState::Release;
                    }
                }
                Control::PitchBend(pb) => self.pitch_bend = pb,
                _ => {}
            }
        }
    }
    /// Get an iterator of frequency-amplitude pairs that are currently playing
    pub fn envelopes(&self) -> impl Iterator<Item = EnvelopeFrame> + '_ {
        self.envelopes.iter().filter_map(move |(_, ne)| {
            if ne.amplitude > 0.0 {
                Some(EnvelopeFrame {
                    note: ne.note,
                    pitch_bend: self.pitch_bend,
                    amplitude: ne.amplitude,
                })
            } else {
                None
            }
        })
    }
    /// Progress the enveloper to the next frame
    pub fn progress(&mut self, sample_rate: u32, adsr: ADSR<f32>) {
        const MIN_VAL: f32 = 0.001;
        for ne in self.envelopes.values_mut() {
            let velocity = f32::from(ne.velocity) / 127.0;
            match ne.state {
                EnvelopeState::Attack => {
                    let slope = velocity / adsr.attack.max(MIN_VAL);
                    assert!(slope >= 0.0);
                    ne.amplitude += slope / sample_rate as f32;
                    if ne.amplitude >= velocity {
                        ne.state = EnvelopeState::Decay;
                    }
                }
                EnvelopeState::Decay => {
                    let slope = (adsr.sustain * velocity - velocity) / adsr.decay.max(MIN_VAL);
                    assert!(slope <= 0.0);
                    ne.amplitude += slope / sample_rate as f32;
                    if ne.amplitude <= adsr.sustain * velocity {
                        ne.state = EnvelopeState::Sustain;
                    }
                }
                EnvelopeState::Sustain => {}
                EnvelopeState::Release => {
                    let slope = (0.0 - adsr.sustain * velocity) / adsr.release.max(MIN_VAL);
                    assert!(slope <= 0.0);
                    ne.amplitude += slope / sample_rate as f32;
                    if ne.amplitude <= 0.0 {
                        ne.state = EnvelopeState::Done;
                    }
                }
                EnvelopeState::Done => panic!("EnvelopeState::Done not purged"),
            }
        }
        self.envelopes
            .retain(|_, ne| !matches!(ne.state, EnvelopeState::Done));
    }
    pub fn end_notes(&mut self, id: u64) {
        if let Some(ne) = self.envelopes.get_mut(&id) {
            ne.state = EnvelopeState::Release;
        }
    }
}
