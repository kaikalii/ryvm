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
    PadStart(u8, u8),
    PadEnd(u8),
}

const NOTE_START: u8 = 0x9;
const NOTE_END: u8 = 0x8;
const PITCH_BEND: u8 = 0xE;
const CONTROLLER: u8 = 0xB;

impl Control {
    #[allow(clippy::unnecessary_cast)]
    pub fn decode(data: &[u8], pad: Option<PadBounds>) -> Option<(u8, Control)> {
        let status = data[0] / 0x10;
        let channel = data[0] % 0x10;
        let d1 = data.get(1).copied().unwrap_or(0);
        let d2 = data.get(2).copied().unwrap_or(0);

        let control = match (status, d1, d2) {
            (NOTE_START, n, v) => {
                let (letter, octave) = Letter::from_u8(n);
                match pad {
                    Some(pad) if pad.channel == channel && pad.start <= n => {
                        Control::PadStart(n - pad.start, v)
                    }
                    _ => Control::NoteStart(letter, octave, v),
                }
            }
            (NOTE_END, n, _) => {
                let (letter, octave) = Letter::from_u8(n);
                match pad {
                    Some(pad) if pad.channel == channel && pad.start <= n => {
                        Control::PadEnd(n - pad.start)
                    }
                    _ => Control::NoteEnd(letter, octave),
                }
            }
            (PITCH_BEND, lsb, msb) => {
                let pb_u16 = msb as u16 * 0x80 + lsb as u16;
                let pb = pb_u16 as f32 / 0x3fff as f32 * 2.0 - 1.0;
                Control::PitchBend(pb)
            }
            (CONTROLLER, n, i) => Control::Controller(n, i),
            _ => return None,
        };

        Some((channel, control))
    }
}
type ControlQueue = Arc<CloneLock<Vec<(u8, Control)>>>;

#[derive(Clone, Copy)]
pub struct PadBounds {
    pub channel: u8,
    pub start: u8,
}

#[derive(Clone)]
pub struct Midi {
    port: usize,
    name: Option<String>,
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
    pub fn new(
        port: usize,
        name: Option<String>,
        manual: bool,
        pad: Option<PadBounds>,
    ) -> Result<Midi, String> {
        let client_name = name.clone().unwrap_or_else(|| format!("midi{}", port));
        let mut midi_in = MidiInput::new(&client_name).map_err(|e| e.to_string())?;
        midi_in.ignore(Ignore::None);

        let queue = Arc::new(CloneLock::new(Vec::new()));
        let queue_clone = Arc::clone(&queue);

        let conn = midi_in
            .connect(
                port,
                &client_name,
                move |_, data, queue| {
                    if let Some(control) = Control::decode(data, pad) {
                        queue.lock().push(control);
                    }
                },
                queue_clone,
            )
            .map_err(|e| e.to_string())?;

        Ok(Midi {
            port,
            name,
            conn: Arc::new(SendWrapper::new(conn)),
            queue,
            manual,
        })
    }
    pub fn controls(&self) -> impl Iterator<Item = (u8, Control)> {
        self.queue.lock().drain(..).collect::<Vec<_>>().into_iter()
    }
}

impl fmt::Debug for Midi {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            self.name.as_ref().unwrap_or(&format!("midi{}", self.port))
        )
    }
}
