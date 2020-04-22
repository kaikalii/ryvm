use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
};

use once_cell::sync::Lazy;
use piston_window::*;

use crate::{Control, Letter};

#[derive(Clone, Debug)]
pub struct Keyboard {
    name: String,
    controls: Arc<Mutex<HashSet<Control>>>,
    handle: Option<Arc<JoinHandle<()>>>,
    done: Arc<AtomicBool>,
}

impl Keyboard {
    pub fn new(name: &str) -> Keyboard {
        let done = Arc::new(AtomicBool::new(false));
        let done_clone = Arc::clone(&done);
        let name_string = name.to_string();
        let controls = Arc::new(Mutex::new(HashSet::new()));
        let controls_clone = Arc::clone(&controls);
        let handle = thread::spawn(move || {
            let mut window: PistonWindow = WindowSettings::new(name_string, [400; 2])
                .automatic_close(false)
                .build()
                .unwrap();
            while let Some(event) = window.next() {
                // Clear
                window.draw_2d(&event, |_, graphics, _| clear([0.0; 4], graphics));
                // Handle events
                if let Event::Input(
                    Input::Button(ButtonArgs {
                        button: Button::Keyboard(key),
                        state,
                        ..
                    }),
                    _,
                ) = event
                {
                    if let Some(&(l, o)) = KEYBINDS.get(&key) {
                        match state {
                            ButtonState::Press => {
                                controls_clone
                                    .lock()
                                    .unwrap()
                                    .insert(Control::StartNote(l, o, 255));
                            }
                            ButtonState::Release => {
                                controls_clone
                                    .lock()
                                    .unwrap()
                                    .insert(Control::EndNote(l, o));
                            }
                        }
                    }
                }
                // Close if necessary
                if done_clone.load(Ordering::Relaxed) {
                    window.set_should_close(true);
                    return;
                }
            }
            done_clone.store(true, Ordering::Relaxed);
        });
        Keyboard {
            name: name.into(),
            controls,
            handle: Some(Arc::new(handle)),
            done,
        }
    }
    pub fn controls(&self) -> MutexGuard<HashSet<Control>> {
        self.controls.lock().unwrap()
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Ok(handle) = Arc::try_unwrap(self.handle.take().unwrap()) {
            let _ = handle.join();
        }
    }
}

static KEYBINDS: Lazy<HashMap<Key, (Letter, u8)>> = Lazy::new(|| {
    let mut map = HashMap::new();
    #[allow(clippy::useless_vec)]
    for (key, letter, octave) in vec![
        (Key::Z, Letter::C, 0),
        (Key::S, Letter::Csh, 0),
        (Key::X, Letter::D, 0),
        (Key::D, Letter::Dsh, 0),
        (Key::C, Letter::E, 0),
        (Key::V, Letter::F, 0),
        (Key::G, Letter::Fsh, 0),
        (Key::B, Letter::G, 0),
        (Key::H, Letter::Gsh, 0),
        (Key::N, Letter::A, 0),
        (Key::J, Letter::Ash, 0),
        (Key::M, Letter::B, 0),
        (Key::Comma, Letter::C, 1),
        (Key::Q, Letter::C, 1),
        (Key::D2, Letter::Csh, 1),
        (Key::L, Letter::Csh, 1),
        (Key::Period, Letter::D, 1),
        (Key::W, Letter::D, 1),
        (Key::Semicolon, Letter::Dsh, 1),
        (Key::D3, Letter::Dsh, 1),
        (Key::Slash, Letter::E, 1),
        (Key::E, Letter::E, 1),
        (Key::R, Letter::F, 1),
        (Key::D5, Letter::Fsh, 1),
        (Key::T, Letter::G, 1),
        (Key::D6, Letter::Gsh, 1),
        (Key::Y, Letter::A, 1),
        (Key::D7, Letter::Ash, 1),
        (Key::U, Letter::B, 1),
        (Key::I, Letter::C, 2),
        (Key::D9, Letter::Csh, 2),
        (Key::O, Letter::D, 2),
        (Key::D0, Letter::Dsh, 2),
        (Key::P, Letter::E, 2),
    ] {
        map.insert(key, (letter, octave));
    }
    map
});
