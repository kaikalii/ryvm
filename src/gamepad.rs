use std::{collections::HashMap, thread};

use crossbeam_channel::{unbounded, Receiver};
use gilrs::{Axis, Button, Error as GilrsError, Event, EventType, Gilrs};
use once_cell::sync::Lazy;

use crate::{midi::CONTROL, spec::GamepadControl, utility::CloneLock};

pub static GAMEPADS: Lazy<Gamepads> = Lazy::new(|| {
    let (send, recv) = unbounded();
    thread::spawn(move || {
        let mut gil = match Gilrs::new() {
            Ok(gil) => gil,
            Err(GilrsError::NotImplemented(gil)) => {
                println!("Gamepad input not supported on this platform");
                gil
            }
            Err(GilrsError::InvalidAxisToBtn) => {
                panic!("Gamepad initialization returned invalid error")
            }
            Err(GilrsError::Other(e)) => panic!("Platform-specific gamepad error: {}", e),
        };
        println!("Gamepads initialized");
        loop {
            if let Some(event) = gil.next_event() {
                let _ = send.send(event);
            }
        }
    });
    Gamepads {
        recv,
        saved: CloneLock::new(HashMap::new()),
    }
});

pub struct Gamepads {
    recv: Receiver<Event>,
    saved: CloneLock<HashMap<usize, Vec<EventType>>>,
}

impl Gamepads {
    pub fn events_for(&self, id: usize) -> Vec<EventType> {
        let mut saved = self.saved.lock();
        let mut events: Vec<_> = saved
            .get_mut(&id)
            .into_iter()
            .flat_map(|queue| queue.drain(..))
            .collect();
        events.extend(self.recv.try_iter().filter_map(|event| {
            if Into::<usize>::into(event.id) == id {
                Some(event.event)
            } else {
                saved
                    .entry(event.id.into())
                    .or_insert_with(Vec::new)
                    .push(event.event);
                None
            }
        }));
        events
    }
}

fn button_to_control(button: Button) -> Option<u8> {
    Some(
        match button {
            Button::Start => GamepadControl::Start,
            Button::Select => GamepadControl::Select,
            Button::South => GamepadControl::South,
            Button::East => GamepadControl::East,
            Button::West => GamepadControl::West,
            Button::North => GamepadControl::North,
            Button::LeftTrigger => GamepadControl::L1,
            Button::LeftTrigger2 => GamepadControl::L2,
            Button::RightTrigger => GamepadControl::R1,
            Button::RightTrigger2 => GamepadControl::R2,
            Button::DPadUp => GamepadControl::DPadUp,
            Button::DPadDown => GamepadControl::DPadDown,
            Button::DPadLeft => GamepadControl::DPadLeft,
            Button::DPadRight => GamepadControl::DPadRight,
            Button::LeftThumb => GamepadControl::L3,
            Button::RightThumb => GamepadControl::R3,
            _ => return None,
        }
        .into(),
    )
}

fn stick_val_to_u8(val: f32) -> u8 {
    ((val + 1.0) * 0x3f as f32) as u8
}

fn trigger_val_to_u8(val: f32) -> u8 {
    (val * 0x7f as f32) as u8
}

fn axis_to_control(axis: Axis, val: f32) -> Option<(u8, u8)> {
    Some(match axis {
        Axis::LeftZ => (8, trigger_val_to_u8(val)),
        Axis::RightZ => (9, trigger_val_to_u8(val)),
        Axis::LeftStickX => (10, stick_val_to_u8(val)),
        Axis::LeftStickY => (11, stick_val_to_u8(val)),
        Axis::RightStickX => (12, stick_val_to_u8(val)),
        Axis::RightStickY => (13, stick_val_to_u8(val)),
        _ => return None,
    })
}

pub fn event_to_midi_message(event: EventType) -> Option<[u8; 3]> {
    const STATUS: u8 = CONTROL * 0x10;
    let (d1, d2) = match event {
        EventType::ButtonPressed(button, _) => (button_to_control(button)?, 0x7f),
        EventType::ButtonReleased(button, _) => (button_to_control(button)?, 0),
        EventType::AxisChanged(axis, val, _) => axis_to_control(axis, val)?,
        _ => return None,
    };
    Some([STATUS, d1, d2])
}
