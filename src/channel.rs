use std::{
    collections::{hash_map, HashMap, HashSet},
    f32, fmt,
    ops::{Add, AddAssign, Mul},
};

use crate::{Device, Letter, State};

#[derive(Debug, Default)]
pub struct Channel {
    devices: HashMap<String, Device>,
}

impl Channel {
    pub fn get(&self, id: &str) -> Option<&Device> {
        self.devices.get(id)
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Device> {
        self.devices.get_mut(id)
    }
    pub fn insert(&mut self, id: String, device: Device) {
        self.devices.insert(id, device);
    }
    pub fn insert_wrapper<F>(&mut self, input: String, id: String, build_device: F)
    where
        F: FnOnce(String) -> Device,
    {
        if self.get(&input).is_none() {
            return;
        }
        for device in self.devices.values_mut() {
            device.replace_input(input.clone(), id.clone());
        }
        let new_device = build_device(input);
        self.insert(id, new_device);
    }
    pub fn device_names(&self) -> hash_map::Keys<String, Device> {
        self.devices.keys()
    }
    pub fn devices(&self) -> hash_map::Values<String, Device> {
        self.devices.values()
    }
    // pub fn devices_mut(&mut self) -> hash_map::ValuesMut<String, Device> {
    //     self.devices.values_mut()
    // }
    pub fn names_devices(&self) -> hash_map::Iter<String, Device> {
        self.devices.iter()
    }
    pub fn names_devices_mut(&mut self) -> hash_map::IterMut<String, Device> {
        self.devices.iter_mut()
    }
    pub fn outputs(&self) -> impl Iterator<Item = &str> + '_ {
        self.device_names()
            .map(AsRef::as_ref)
            .filter(move |name| !self.devices().any(|device| device.inputs().contains(name)))
    }
    pub fn remove(&mut self, id: &str, recursive: bool) {
        if let Some(device) = self.get(id) {
            if recursive {
                let inputs: Vec<String> = device.inputs().into_iter().map(Into::into).collect();
                for input in inputs {
                    if !self
                        .devices
                        .iter()
                        .filter(|(i, _)| i != &id)
                        .any(|(_, device)| device.inputs().contains(&&*input))
                    {
                        self.remove(&input, recursive);
                    }
                }
            }
            self.devices.remove(id);
        }
    }
    pub fn next_from(&self, name: &str, state: &State, cache: &mut FrameCache) -> Voice {
        if cache.visited.contains(name) {
            // Avoid infinite loops
            Voice::mono(0.0)
        } else {
            cache.visited.insert(name.into());
            if let Some(device) = self.get(name) {
                if let Some(voice) = cache.voices.get(name) {
                    *voice
                } else {
                    let voice = device.next(self, state, cache, name);
                    cache.voices.insert(name.into(), voice);
                    voice
                }
            } else {
                Voice::SILENT
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Voice {
    pub left: f32,
    pub right: f32,
}

impl Voice {
    pub const SILENT: Self = Voice {
        left: 0.0,
        right: 0.0,
    };
    pub fn stereo(left: f32, right: f32) -> Self {
        Voice { left, right }
    }
    pub fn mono(both: f32) -> Self {
        Voice::stereo(both, both)
    }
    pub fn is_silent(self) -> bool {
        self.left.abs() < f32::EPSILON && self.right.abs() < f32::EPSILON
    }
}

impl Mul<f32> for Voice {
    type Output = Self;
    fn mul(self, m: f32) -> Self::Output {
        Voice {
            left: self.left * m,
            right: self.right * m,
        }
    }
}

impl AddAssign for Voice {
    fn add_assign(&mut self, other: Self) {
        self.left += other.left;
        self.right += other.right;
    }
}

impl Add for Voice {
    type Output = Self;
    fn add(mut self, other: Self) -> Self::Output {
        self += other;
        self
    }
}

impl fmt::Debug for Voice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.is_silent() {
            write!(f, "silent")
        } else if (self.left - self.right).abs() < std::f32::EPSILON {
            write!(f, "{}", self.left)
        } else {
            write!(f, "({}, {})", self.left, self.right)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    NoteStart(Letter, u8, u8),
    NoteEnd(Letter, u8),
    PitchBend(f32),
    Controller(u8, u8),
    PadStart(Letter, u8, u8),
    PadEnd(Letter, u8),
}

pub struct FrameCache {
    pub voices: HashMap<String, Voice>,
    pub controls: Vec<Control>,
    pub visited: HashSet<String>,
}
