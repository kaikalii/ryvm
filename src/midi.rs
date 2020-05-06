use std::{collections::HashMap, error::Error, fmt, sync::Arc};

use midir::{
    ConnectErrorKind, Ignore, InitError, MidiInput, MidiInputConnection, MidiOutput,
    MidiOutputConnection, PortInfoError, SendError,
};
use rand::random;
use ryvm_spec::{Action, Button, Buttons, Slider, Sliders, ValuedAction};

use crate::{event_to_midi_message, CloneCell, CloneLock, GAMEPADS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Port {
    Midi(usize),
    Gamepad(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    /// Id, index, velocity
    NoteStart(u64, u8, u8),
    /// Id, index
    NoteEnd(u64, u8),
    /// Bend value
    PitchBend(f32),
    /// Index, value
    Control(u8, u8),
    /// Index, velocity
    PadStart(u8, u8),
    /// Action, velocity
    Action(Action, u8),
    /// Action, value
    ValuedAction(ValuedAction, u8),
}

const NOTE_START: u8 = 0x9;
const NOTE_END: u8 = 0x8;
const PITCH_BEND: u8 = 0xE;
pub(crate) const CONTROL: u8 = 0xB;

const TIMING: u8 = 0x15;

impl Control {
    #[allow(clippy::unnecessary_cast)]
    pub fn decode(
        data: &[u8],
        port: usize,
        monitor: bool,
        buttons: &Buttons,
        sliders: &Sliders,
    ) -> Option<(u8, Control)> {
        if data[0] == TIMING {
            return None;
        }
        let status = data[0] / 0x10;
        let channel = (data[0] % 0x10).overflowing_add(1).0;
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

        let control = match (status, d1, d2) {
            (NOTE_START, n, v) => check_buttons(buttons, sliders, status, channel, d1, d2, || {
                Control::NoteStart(random::<u64>() % 1_000_000, n, v)
            }),
            (NOTE_END, n, _) => check_buttons(buttons, sliders, status, channel, d1, d2, || {
                Control::NoteEnd(0, n)
            }),
            (PITCH_BEND, lsb, msb) => {
                let pb_u16 = u16::from(msb) * 0x80 + u16::from(lsb);
                let pb = f32::from(pb_u16) / 0x3fff as f32 * 2.0 - 1.0;
                Some(Control::PitchBend(pb))
            }
            (CONTROL, n, i) => check_buttons(buttons, sliders, status, channel, d1, d2, || {
                Control::Control(n, i)
            }),
            _ => None,
        };

        if let Some(control) = control {
            if monitor {
                println!(" | ch{:ch_width$} | {:?}", channel, control, ch_width = 3)
            }
        } else if monitor {
            println!();
        }

        control.map(|control| (channel, control))
    }
}

fn check_buttons<F>(
    buttons: &Buttons,
    sliders: &Sliders,
    status: u8,
    channel: u8,
    d1: u8,
    d2: u8,
    otherwise: F,
) -> Option<Control>
where
    F: FnOnce() -> Control,
{
    match (status, d1, d2) {
        (CONTROL, n, v) => {
            if let Some(action) = buttons.get_by_right(&Button::Control(n)) {
                if v == 0 {
                    None
                } else {
                    Some(Control::Action(*action, 0x7f))
                }
            } else if let Some(val_action) = sliders.get_by_right(&Slider::Control(n)) {
                Some(Control::ValuedAction(*val_action, v))
            } else {
                Some(otherwise())
            }
        }
        (NOTE_START, n, v) => {
            if let Some(action) = buttons
                .get_by_right(&Button::Note(n))
                .or_else(|| buttons.get_by_right(&Button::ChannelNote(channel, n)))
            {
                if v == 0 {
                    None
                } else {
                    Some(Control::Action(*action, d2))
                }
            } else {
                Some(otherwise())
            }
        }
        (NOTE_END, n, _) => {
            if buttons.contains_right(&Button::Note(n))
                || buttons.contains_right(&Button::ChannelNote(channel, n))
            {
                None
            } else {
                Some(otherwise())
            }
        }
        _ => Some(otherwise()),
    }
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

/// A queue of channels and corresponding controls
#[derive(Clone)]
enum ControlQueue {
    Midi(Arc<CloneLock<Vec<(u8, Control)>>>),
    Gamepad(usize),
}

#[derive(Clone)]
struct MidiInputState {
    queue: ControlQueue,
    monitor: Arc<CloneCell<bool>>,
    buttons: Buttons,
    sliders: Sliders,
}

enum GenericInput {
    Midi(MidiInputConnection<MidiInputState>),
    Gamepad,
}

#[allow(dead_code)]
pub struct Midi {
    port: Port,
    name: String,
    device: Option<String>,
    input: GenericInput,
    output: Option<MidiOutputConnection>,
    state: MidiInputState,
    manual: bool,
    non_globals: Vec<u8>,
    last_notes: HashMap<u8, u64>,
}

impl Midi {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }
    pub fn control_is_global(&self, control: u8) -> bool {
        !self.non_globals.contains(&control)
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
    pub fn port_matching(name: &str) -> Result<Option<usize>, MidiError> {
        Midi::ports_list().map(|list| list.iter().position(|item| item.contains(name)))
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
        non_globals: Vec<u8>,
        buttons: Buttons,
        sliders: Sliders,
    ) -> Result<Midi, MidiError> {
        let mut midi_in = MidiInput::new(&name)?;
        midi_in.ignore(Ignore::Time);
        let midi_out = MidiOutput::new(&name)?;

        assert_eq!(midi_in.port_name(port)?, midi_out.port_name(port)?);

        let state = MidiInputState {
            queue: ControlQueue::Midi(Arc::new(CloneLock::new(Vec::new()))),
            monitor: Arc::new(CloneCell::new(false)),
            buttons,
            sliders,
        };

        let device = midi_in.port_name(port)?;

        let input = midi_in
            .connect(
                port,
                &name,
                move |_, data, state| {
                    if let Some(control) = Control::decode(
                        data,
                        port,
                        state.monitor.load(),
                        &state.buttons,
                        &state.sliders,
                    ) {
                        if let ControlQueue::Midi(queue) = &state.queue {
                            queue.lock().push(control);
                        }
                    }
                },
                state.clone(),
            )
            .map_err(|e| e.kind())?;

        let output = midi_out.connect(port, &name).map_err(|e| e.kind())?;

        Ok(Midi {
            port: Port::Midi(port),
            name,
            device: Some(device),
            input: GenericInput::Midi(input),
            output: Some(output),
            state,
            manual,
            non_globals,
            last_notes: HashMap::new(),
        })
    }
    pub fn new_gamepad(
        name: String,
        port: usize,
        manual: bool,
        non_globals: Vec<u8>,
        buttons: Buttons,
        sliders: Sliders,
    ) -> Midi {
        let state = MidiInputState {
            queue: ControlQueue::Gamepad(port),
            monitor: Arc::new(CloneCell::new(false)),
            buttons,
            sliders,
        };
        Midi {
            port: Port::Gamepad(port),
            name,
            device: None,
            input: GenericInput::Gamepad,
            output: None,
            state,
            manual,
            non_globals,
            last_notes: HashMap::new(),
        }
    }
    pub fn controls(&mut self) -> Result<Vec<(u8, Control)>, SendError> {
        Ok(match &self.state.queue {
            ControlQueue::Midi(queue) => {
                let last_notes = &mut self.last_notes;
                queue
                    .lock()
                    .drain(..)
                    .filter_map(|(ch, control)| {
                        match control {
                            Control::NoteStart(id, n, _) => {
                                last_notes.insert(n, id);
                            }
                            Control::NoteEnd(_, n) => {
                                return last_notes
                                    .remove(&n)
                                    .map(|id| (ch, Control::NoteEnd(id, n)))
                            }
                            _ => {}
                        }
                        Some((ch, control))
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .collect()
            }
            ControlQueue::Gamepad(id) => GAMEPADS
                .events_for(*id)
                .into_iter()
                .filter_map(event_to_midi_message)
                .filter_map(|data| {
                    Control::decode(
                        &data,
                        *id,
                        self.state.monitor.load(),
                        &self.state.buttons,
                        &self.state.sliders,
                    )
                })
                .collect(),
        })
    }
}

impl fmt::Debug for Midi {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}
