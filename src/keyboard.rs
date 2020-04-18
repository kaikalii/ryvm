use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
};

use crossbeam::sync::ShardedLock;
use once_cell::sync::Lazy;
use piston_window::*;
use serde_derive::{Deserialize, Serialize};

use crate::Letter;

/// Struct used as a definitation for serializing a keyboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardDef {
    pub name: String,
    pub base_octave: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(into = "KeyboardDef", from = "KeyboardDef")]
pub struct Keyboard {
    name: String,
    base_octave: Arc<AtomicU8>,
    pressed: Arc<ShardedLock<HashSet<(Letter, u8)>>>,
    handle: Option<Arc<JoinHandle<()>>>,
    done: Arc<AtomicBool>,
}

impl From<Keyboard> for KeyboardDef {
    fn from(keyboard: Keyboard) -> Self {
        KeyboardDef {
            name: keyboard.name.clone(),
            base_octave: keyboard.base_octave.load(Ordering::Relaxed),
        }
    }
}

impl From<KeyboardDef> for Keyboard {
    fn from(def: KeyboardDef) -> Self {
        Keyboard::new(&def.name, def.base_octave)
    }
}

impl Keyboard {
    pub fn new(name: &str, base_octave: u8) -> Keyboard {
        let done = Arc::new(AtomicBool::new(false));
        let done_clone = Arc::clone(&done);
        let name_string = name.to_string();
        let pressed = Arc::new(ShardedLock::new(HashSet::new()));
        let pressed_clone = Arc::clone(&pressed);
        let base_octave = Arc::new(AtomicU8::new(base_octave));
        let base_octave_clone = Arc::clone(&base_octave);
        let handle = thread::spawn(move || {
            let mut window: PistonWindow =
                WindowSettings::new(name_string, [400; 2]).build().unwrap();
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
                                pressed_clone
                                    .write()
                                    .unwrap()
                                    .insert((l, o + base_octave_clone.load(Ordering::Relaxed)));
                            }
                            ButtonState::Release => {
                                pressed_clone
                                    .write()
                                    .unwrap()
                                    .remove(&(l, o + base_octave_clone.load(Ordering::Relaxed)));
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
            base_octave,
            pressed,
            handle: Some(Arc::new(handle)),
            done,
        }
    }
    pub fn set_base_octave(&self, base_octave: u8) {
        self.base_octave.store(base_octave, Ordering::Relaxed);
    }
    pub fn pressed<F, R>(&self, f: F) -> R
    where
        F: Fn(&HashSet<(Letter, u8)>) -> R,
    {
        f(&*self.pressed.read().unwrap())
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
