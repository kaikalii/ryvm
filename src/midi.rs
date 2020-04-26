use std::{fmt, sync::Arc};

use midir::{Ignore, MidiInput, MidiInputConnection};
use send_wrapper::SendWrapper;

use crate::{CloneLock, Control, Letter, SampleType};

const START_NOTE: u8 = 0x90;
const END_NOTE: u8 = 0x80;
const PITCH_BEND: u8 = 0xE0;

pub fn decode_control(data: &[u8]) -> Option<Control> {
    let mut padded = [0; 3];
    for (i, &b) in data.iter().enumerate() {
        padded[i] = b;
    }
    match padded {
        [END_NOTE, n, _] => {
            let (letter, octave) = Letter::from_u8(n);
            Some(Control::EndNote(letter, octave))
        }
        [START_NOTE, n, v] => {
            let (letter, octave) = Letter::from_u8(n);
            Some(Control::StartNote(letter, octave, v))
        }
        [PITCH_BEND, lsb, msb] => {
            let pb_u16 = msb as u16 * 0x80 + lsb as u16;
            let pb = pb_u16 as SampleType / 0x3fff as SampleType * 2.0 - 1.0;
            Some(Control::PitchBend(pb))
        }
        _ => None,
    }
}

type ControlQueue = Arc<CloneLock<Vec<Control>>>;

#[derive(Clone)]
pub struct Midi {
    name: String,
    conn: Arc<SendWrapper<MidiInputConnection<ControlQueue>>>,
    queue: ControlQueue,
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
    pub fn new(name: &str, port: usize) -> Result<Midi, String> {
        let mut midi_in = MidiInput::new(name).map_err(|e| e.to_string())?;
        midi_in.ignore(Ignore::None);

        let queue = Arc::new(CloneLock::new(Vec::new()));
        let queue_clone = Arc::clone(&queue);

        let conn = midi_in
            .connect(
                port,
                name,
                |_, data, queue| {
                    if let Some(control) = decode_control(data) {
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
        })
    }
    pub fn controls(&self) -> impl Iterator<Item = Control> {
        self.queue.lock().drain(..).collect::<Vec<_>>().into_iter()
    }
}

impl fmt::Debug for Midi {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "midi:{}", self.name)
    }
}
