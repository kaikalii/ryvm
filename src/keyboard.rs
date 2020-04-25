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
                                    .insert(Control::StartNote(l, o, 127));
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
        (Key::Z, Letter::C, 3),
        (Key::S, Letter::Csh, 3),
        (Key::X, Letter::D, 3),
        (Key::D, Letter::Dsh, 3),
        (Key::C, Letter::E, 3),
        (Key::V, Letter::F, 3),
        (Key::G, Letter::Fsh, 3),
        (Key::B, Letter::G, 3),
        (Key::H, Letter::Gsh, 3),
        (Key::N, Letter::A, 3),
        (Key::J, Letter::Ash, 3),
        (Key::M, Letter::B, 3),
        (Key::Comma, Letter::C, 4),
        (Key::Q, Letter::C, 4),
        (Key::D2, Letter::Csh, 4),
        (Key::L, Letter::Csh, 4),
        (Key::Period, Letter::D, 4),
        (Key::W, Letter::D, 4),
        (Key::Semicolon, Letter::Dsh, 4),
        (Key::D3, Letter::Dsh, 4),
        (Key::Slash, Letter::E, 4),
        (Key::E, Letter::E, 4),
        (Key::R, Letter::F, 4),
        (Key::D5, Letter::Fsh, 4),
        (Key::T, Letter::G, 4),
        (Key::D6, Letter::Gsh, 4),
        (Key::Y, Letter::A, 4),
        (Key::D7, Letter::Ash, 4),
        (Key::U, Letter::B, 4),
        (Key::I, Letter::C, 5),
        (Key::D9, Letter::Csh, 5),
        (Key::O, Letter::D, 5),
        (Key::D0, Letter::Dsh, 5),
        (Key::P, Letter::E, 5),
    ] {
        map.insert(key, (letter, octave));
    }
    map
});
