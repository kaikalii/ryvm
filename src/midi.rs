use std::{fmt, sync::Arc};

use midir::{Ignore, MidiInput, MidiInputConnection};
use send_wrapper::SendWrapper;

use crate::{CloneLock, Letter};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    NoteStart(Letter, u8, u8),
    NoteEnd(Letter, u8),
    PitchBend(f32),
    Controller(u8, u8),
    PadStart(Letter, u8, u8),
    PadEnd(Letter, u8),
}

const NOTE_START: u8 = 0x9;
const NOTE_END: u8 = 0x8;
const PITCH_BEND: u8 = 0xE;
const CONTROLLER: u8 = 0xB;

impl Control {
    #[allow(clippy::unnecessary_cast)]
    pub fn decode(data: &[u8]) -> Option<(u8, Control)> {
        let status = data[0] / 0x10;
        let channel = data[0] % 0x10;
        let d1 = data[1];
        let d2 = data[2];

        Some((
            channel,
            match (status, d1, d2) {
                (NOTE_START, n, v) => {
                    let (letter, octave) = Letter::from_u8(n);
                    Control::NoteStart(letter, octave, v)
                }
                (NOTE_END, n, _) => {
                    let (letter, octave) = Letter::from_u8(n);
                    Control::NoteEnd(letter, octave)
                }
                (PITCH_BEND, lsb, msb) => {
                    let pb_u16 = msb as u16 * 0x80 + lsb as u16;
                    let pb = pb_u16 as f32 / 0x3fff as f32 * 2.0 - 1.0;
                    Control::PitchBend(pb)
                }
                (CONTROLLER, n, i) => Control::Controller(n, i),
                (PAD_START, n, v) => {
                    let (letter, octave) = Letter::from_u8(n);
                    Control::PadStart(letter, octave, v)
                }
                (PAD_END, n, _) => {
                    let (letter, octave) = Letter::from_u8(n);
                    Control::PadEnd(letter, octave)
                }
                _ => return None,
            },
        ))
    }
}
type ControlQueue = Arc<CloneLock<Vec<(u8, Control)>>>;

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
    pub fn new(name: &str, port: usize, manual: bool) -> Result<Midi, String> {
        let mut midi_in = MidiInput::new(name).map_err(|e| e.to_string())?;
        midi_in.ignore(Ignore::None);

        let queue = Arc::new(CloneLock::new(Vec::new()));
        let queue_clone = Arc::clone(&queue);

        let conn = midi_in
            .connect(
                port,
                name,
                |_, data, queue| {
                    if let Some(control) = Control::decode(data) {
                        queue.lock().push(control);
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
    pub fn controls(&self, current_channel: u8) -> impl Iterator<Item = Control> {
        self.queue.lock().drain(..).collect::<Vec<_>>().into_iter()
    }
}

impl fmt::Debug for Midi {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "midi:{}", self.name)
    }
}
