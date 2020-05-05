use std::thread;

use crossbeam_channel::{unbounded, Receiver, TryIter};
use gilrs::{Error as GilrsError, Event, Gilrs};
use once_cell::sync::Lazy;

static EVENT_RECV: Lazy<Receiver<Event>> = Lazy::new(|| {
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
        while let Some(event) = gil.next_event() {
            let _ = send.send(event);
        }
    });
    recv
});

pub fn gamepad_events() -> TryIter<'static, Event> {
    EVENT_RECV.try_iter()
}
