use std::{collections::HashMap, fmt, iter::once, sync::Arc};

use midir::{Ignore, MidiInput, MidiInputConnection};
use send_wrapper::SendWrapper;

use crate::{CloneLock, Letter};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    NoteStart(Letter, u8, u8),
    NoteEnd(Letter, u8),
    PitchBend(f32),
    Controller(u8, u8),
    PadStart(u8, u8),
    PadEnd(u8),
}

const NOTE_START: u8 = 0x9;
const NOTE_END: u8 = 0x8;
const PITCH_BEND: u8 = 0xE;
const CONTROLLER: u8 = 0xB;

impl Control {
    #[allow(clippy::unnecessary_cast)]
    pub fn decode(data: &[u8], pad: Option<Pad>) -> Option<(u8, Vec<Control>)> {
        let status = data[0] / 0x10;
        let channel = data[0] % 0x10;
        let d1 = data.get(1).copied().unwrap_or(0);
        let d2 = data.get(2).copied().unwrap_or(0);

        Some((
            channel,
            match (status, d1, d2) {
                (NOTE_START, n, v) => {
                    let (letter, octave) = Letter::from_u8(n);
                    once(Control::NoteStart(letter, octave, v))
                        .chain(if let Some(pad) = pad {
                            if pad.channel == channel && pad.start <= n {
                                Some(Control::PadStart(n - pad.start, v))
                            } else {
                                None
                            }
                        } else {
                            None
                        })
                        .collect()
                }
                (NOTE_END, n, _) => {
                    let (letter, octave) = Letter::from_u8(n);
                    once(Control::NoteEnd(letter, octave))
                        .chain(if let Some(pad) = pad {
                            if pad.channel == channel && pad.start <= n {
                                Some(Control::PadEnd(n - pad.start))
                            } else {
                                None
                            }
                        } else {
                            None
                        })
                        .collect()
                }
                (PITCH_BEND, lsb, msb) => {
                    let pb_u16 = msb as u16 * 0x80 + lsb as u16;
                    let pb = pb_u16 as f32 / 0x3fff as f32 * 2.0 - 1.0;
                    vec![Control::PitchBend(pb)]
                }
                (CONTROLLER, n, i) => vec![Control::Controller(n, i)],
                _ => return None,
            },
        ))
    }
}
type ControlQueue = Arc<CloneLock<Vec<(u8, Control)>>>;

#[derive(Clone, Copy)]
pub struct Pad {
    pub channel: u8,
    pub start: u8,
}

#[derive(Clone)]
pub struct Midi {
    name: String,
    conn: Arc<SendWrapper<MidiInputConnection<ControlQueue>>>,
    queue: ControlQueue,
    manual: bool,
}

impl Midi {
    pub fn ports_list() -> Result<Vec<String>, String> {
        let midi_in = MidiInput::new("").map_err(|e| e.to_string())?;
        Ok((0..midi_in.port_count())
            .map(|i| {
                midi_in
                    .port_name(i)
                    .unwrap_or_else(|_| "<unknown>".to_string())
            })
            .collect())
    }
    pub fn first_device() -> Result<Option<usize>, String> {
        for (i, name) in Midi::ports_list()?.into_iter().enumerate() {
            if !["thru", "through"]
                .iter()
                .any(|pat| name.to_lowercase().contains(pat))
            {
                return Ok(Some(i));
            }
        }
        Ok(None)
    }
    pub fn new(name: &str, port: usize, manual: bool, pad: Option<Pad>) -> Result<Midi, String> {
        let mut midi_in = MidiInput::new(name).map_err(|e| e.to_string())?;
        midi_in.ignore(Ignore::None);

        let queue = Arc::new(CloneLock::new(Vec::new()));
        let queue_clone = Arc::clone(&queue);

        let conn = midi_in
            .connect(
                port,
                name,
                move |_, data, queue| {
                    if let Some((channel, controls)) = Control::decode(data, pad) {
                        for control in controls {
                            queue.lock().push((channel, control));
                        }
                    }
                },
                queue_clone,
            )
            .map_err(|e| e.to_string())?;

        Ok(Midi {
            name: name.into(),
            conn: Arc::new(SendWrapper::new(conn)),
            queue,
            manual,
        })
    }
    pub fn controls(&self) -> HashMap<u8, Control> {
        self.queue.lock().drain(..).collect()
    }
}

impl fmt::Debug for Midi {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "midi:{}", self.name)
    }
}
