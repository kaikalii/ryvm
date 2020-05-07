use std::collections::{hash_map, HashMap, HashSet};

use std::{
    f32, fmt,
    ops::{Add, AddAssign, Mul},
};

use crate::{Control, Device, Port, State};

/// A midi channel that can contain many devices
#[derive(Debug, Default)]
pub struct Channel {
    devices: HashMap<String, Device>,
}

impl Channel {
    /// Get a device by name
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Device> {
        self.devices.get(name)
    }
    /// Get a device map entry
    pub fn entry(&mut self, name: String) -> hash_map::Entry<String, Device> {
        self.devices.entry(name)
    }
    /// Get an iterator over the device names
    #[must_use]
    pub fn device_names(&self) -> hash_map::Keys<String, Device> {
        self.devices.keys()
    }
    /// Get an iterator over the devices
    #[must_use]
    pub fn devices(&self) -> hash_map::Values<String, Device> {
        self.devices.values()
    }
    /// Get an iterator over mutable references to the devices
    #[must_use]
    pub fn devices_mut(&mut self) -> hash_map::ValuesMut<String, Device> {
        self.devices.values_mut()
    }
    /// Get an iterator over names and devices
    #[must_use]
    pub fn names_devices(&self) -> hash_map::Iter<String, Device> {
        self.devices.iter()
    }
    // /// Get an iterator over names and mutable references to the devices
    // #[must_use]
    // pub fn names_devices_mut(&mut self) -> hash_map::IterMut<String, Device> {
    //     self.devices.iter_mut()
    // }
    /// Get an iterator over the names of devices in this channel that should be output
    pub fn outputs(&self) -> impl Iterator<Item = &str> + '_ {
        self.device_names()
            .map(AsRef::as_ref)
            .filter(move |name| !self.devices().any(|device| device.inputs().contains(name)))
    }
    /// Retain devices that satisfy the predicate
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&String, &mut Device) -> bool,
    {
        self.devices.retain(f)
    }
    // /// Clear all devices
    // pub fn clear(&mut self) {
    //     self.devices.clear();
    // }
    /// Remove a device and optionally recursively delete all of its unique inputs
    pub fn remove(&mut self, name: &str, recursive: bool) {
        if let Some(device) = self.get(name) {
            if recursive {
                let inputs: Vec<String> = device.inputs().into_iter().map(Into::into).collect();
                for input in inputs {
                    if !self
                        .devices
                        .iter()
                        .filter(|(i, _)| i != &name)
                        .any(|(_, device)| device.inputs().contains(&&*input))
                    {
                        self.remove(&input, recursive);
                    }
                }
            }
            self.devices.remove(name);
        }
    }
    #[must_use]
    pub(crate) fn next_from(
        &self,
        channel_num: u8,
        name: &str,
        state: &State,
        cache: &mut FrameCache,
    ) -> Voice {
        let full_name = (channel_num, name.to_string());
        if cache.visited.contains(&full_name) {
            // Avoid infinite loops
            Voice::mono(0.0)
        } else {
            cache.visited.insert(full_name.clone());
            if let Some(device) = self.get(name) {
                if let Some(voice) = cache.voices.get(&full_name) {
                    *voice
                } else {
                    let voice = device.next(channel_num, self, state, cache, name);
                    cache.voices.insert(full_name, voice);
                    voice
                }
            } else {
                Voice::SILENT
            }
        }
    }
}

pub(crate) struct FrameCache {
    pub voices: HashMap<(u8, String), Voice>,
    pub controls: HashMap<(Port, u8), Vec<Control>>,
    pub visited: HashSet<(u8, String)>,
    pub from_loop: bool,
}

impl FrameCache {
    pub fn all_controls(&self) -> impl Iterator<Item = Control> + '_ {
        self.controls.values().flat_map(|v| v.iter().copied())
    }
    pub fn channel_controls(&self, channel: u8) -> impl Iterator<Item = Control> + '_ {
        self.controls
            .iter()
            .filter(move |((_, ch), _)| ch == &channel)
            .flat_map(|(_, controls)| controls.iter().copied())
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Voice {
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

impl Mul for Voice {
    type Output = Self;
    fn mul(self, other: Self) -> Self::Output {
        Voice {
            left: self.left * other.left,
            right: self.right * other.right,
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
