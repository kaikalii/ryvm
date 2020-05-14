use std::collections::HashMap;

use crate::{spec::ADSR, ty::Control};

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
    t: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct EnvelopeFrame {
    pub note: u8,
    pub amplitude: f32,
    pub pitch_bend: f32,
    pub t: f32,
}

/// Keeps track of the key states of an input node
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
                            t: 0.0,
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
                    t: ne.t,
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
            ne.t += 1.0 / sample_rate as f32;
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
