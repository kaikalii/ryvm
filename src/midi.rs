use std::{error::Error, fmt, sync::Arc};

use midir::{
    ConnectErrorKind, Ignore, InitError, MidiInput, MidiInputConnection, MidiOutput,
    MidiOutputConnection, PortInfoError, SendError,
};

use crate::{CloneCell, CloneLock, Letter};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    NoteStart(Letter, u8, u8),
    NoteEnd(Letter, u8),
    PitchBend(f32),
    Control(u8, u8),
    PadStart(u8, u8),
    PadEnd(u8),
    Record,
    StopRecord,
}

const NOTE_START: u8 = 0x9;
const NOTE_END: u8 = 0x8;
const PITCH_BEND: u8 = 0xE;
const CONTROL: u8 = 0xB;

const TIMING: u8 = 0x15;

impl Control {
    #[allow(clippy::unnecessary_cast)]
    pub fn decode(
        data: &[u8],
        port: usize,
        monitor: bool,
        pad: Option<PadBounds>,
        record: Option<u8>,
        stop_record: Option<u8>,
    ) -> Option<(u8, Control)> {
        if data[0] == TIMING {
            return None;
        }
        let status = data[0] / 0x10;
        let channel = data[0] % 0x10;
        let d1 = data.get(1).copied().unwrap_or(0);
        let d2 = data.get(2).copied().unwrap_or(0);

        if monitor {
            print!(
                "port {:port_width$} | {:data_width$}",
                port,
                format!("{:?}", data),
                port_width = 3,
                data_width = 15
            );
        }

        #[rustfmt::skip]
        macro_rules! return_none { () => {{ println!(); return None; }} };

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
                let pb_u16 = u16::from(msb) * 0x80 + u16::from(lsb);
                let pb = f32::from(pb_u16) / 0x3fff as f32 * 2.0 - 1.0;
                Control::PitchBend(pb)
            }
            (CONTROL, n, i) => {
                if record == Some(n) {
                    if i != 0x7f {
                        return_none!();
                    };
                    Control::Record
                } else if stop_record == Some(n) {
                    if i != 0x7f {
                        return_none!();
                    };
                    Control::StopRecord
                } else {
                    Control::Control(n, i)
                }
            }
            _ => return_none!(),
        };

        if monitor {
            println!(" | ch{:ch_width$} | {:?}", channel, control, ch_width = 3)
        }

        Some((channel, control))
    }
}

#[derive(Clone, Copy)]
pub struct PadBounds {
    pub channel: u8,
    pub start: u8,
    pub end: u8,
}

#[derive(Debug)]
pub enum MidiError {
    Init(InitError),
    Connect(ConnectErrorKind),
    Send(SendError),
    PortInfo(PortInfoError),
}

impl fmt::Display for MidiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MidiError::Init(e) => write!(f, "{}", e),
            MidiError::Connect(e) => write!(f, "{}", e),
            MidiError::Send(e) => write!(f, "{}", e),
            MidiError::PortInfo(e) => write!(f, "{}", e),
        }
    }
}

macro_rules! midi_error_from {
    ($variant:ident, $type:ty) => {
        impl From<$type> for MidiError {
            fn from(e: $type) -> Self {
                MidiError::$variant(e)
            }
        }
    };
}

midi_error_from!(Init, InitError);
midi_error_from!(Connect, ConnectErrorKind);
midi_error_from!(Send, SendError);
midi_error_from!(PortInfo, PortInfoError);

impl Error for MidiError {}

type ControlQueue = Arc<CloneLock<Vec<(u8, Control)>>>;

#[derive(Clone)]
struct MidiInputState {
    queue: ControlQueue,
    monitor: Arc<CloneCell<bool>>,
}

#[allow(dead_code)]
pub struct Midi {
    port: usize,
    name: String,
    input: MidiInputConnection<MidiInputState>,
    output: MidiOutputConnection,
    state: MidiInputState,
    manual: bool,
    pad: Option<PadBounds>,
}

impl Midi {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn ports_list() -> Result<Vec<String>, MidiError> {
        let midi_in = MidiInput::new("")?;
        Ok((0..midi_in.port_count())
            .map(|i| {
                midi_in
                    .port_name(i)
                    .unwrap_or_else(|_| "<unknown>".to_string())
            })
            .collect())
    }
    pub fn monitoring(&self) -> bool {
        self.state.monitor.load()
    }
    pub fn set_monitoring(&self, monitoring: bool) {
        self.state.monitor.store(monitoring)
    }
    pub fn first_device() -> Result<Option<usize>, MidiError> {
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
        name: String,
        port: usize,
        manual: bool,
        pad: Option<PadBounds>,
        record: Option<u8>,
        stop_record: Option<u8>,
    ) -> Result<Midi, MidiError> {
        let mut midi_in = MidiInput::new(&name)?;
        midi_in.ignore(Ignore::Time);
        let midi_out = MidiOutput::new(&name)?;

        assert_eq!(midi_in.port_name(port)?, midi_out.port_name(port)?);

        let state = MidiInputState {
            queue: Arc::new(CloneLock::new(Vec::new())),
            monitor: Arc::new(CloneCell::new(false)),
        };

        let input = midi_in
            .connect(
                port,
                &name,
                move |_, data, state| {
                    if let Some(control) =
                        Control::decode(data, port, state.monitor.load(), pad, record, stop_record)
                    {
                        state.queue.lock().push(control);
                    }
                },
                state.clone(),
            )
            .map_err(|e| e.kind())?;

        let output = midi_out.connect(port, &name).map_err(|e| e.kind())?;

        Ok(Midi {
            port,
            name,
            input,
            output,
            state,
            manual,
            pad,
        })
    }
    pub fn controls(&mut self) -> Result<impl Iterator<Item = (u8, Control)>, SendError> {
        Ok(self
            .state
            .queue
            .lock()
            .drain(..)
            .collect::<Vec<_>>()
            .into_iter())
    }
}

impl fmt::Debug for Midi {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}
