use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
};

use crossbeam::sync::ShardedLock;
use once_cell::sync::Lazy;
use piston_window::*;

use crate::Letter;

pub struct Keyboard {
    pressed: Arc<ShardedLock<HashSet<(Letter, u8)>>>,
    handle: Option<JoinHandle<()>>,
    done: Arc<AtomicBool>,
}

impl Keyboard {
    pub fn new(name: &str, base_octave: u8) -> Keyboard {
        let done = Arc::new(AtomicBool::new(false));
        let done_clone = Arc::clone(&done);
        let name = name.to_string();
        let pressed = Arc::new(ShardedLock::new(HashSet::new()));
        let pressed_clone = Arc::clone(&pressed);
        let handle = thread::spawn(move || {
            let mut window: PistonWindow = WindowSettings::new(name, [400; 2]).build().unwrap();
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
                                pressed_clone.write().unwrap().insert((l, o + base_octave));
                            }
                            ButtonState::Release => {
                                pressed_clone.write().unwrap().remove(&(l, o + base_octave));
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
            pressed,
            handle: Some(handle),
            done,
        }
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
        let _ = self.handle.take().unwrap().join();
    }
}

static KEYBINDS: Lazy<HashMap<Key, (Letter, u8)>> = Lazy::new(|| {
    let mut map = HashMap::new();
    for &(key, letter, octave) in &[
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
        (Key::W, Letter::D, 1),
        (Key::D3, Letter::Dsh, 1),
        (Key::E, Letter::E, 1),
        (Key::R, Letter::F, 1),
        (Key::D5, Letter::Fsh, 1),
        (Key::T, Letter::G, 1),
        (Key::D6, Letter::Gsh, 1),
        (Key::Y, Letter::A, 1),
        (Key::D7, Letter::Ash, 1),
        (Key::U, Letter::B, 1),
        (Key::I, Letter::C, 2),
    ] {
        map.insert(key, (letter, octave));
    }
    map
});
