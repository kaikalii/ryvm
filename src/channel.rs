use std::collections::{hash_map, HashMap, HashSet};

use crate::{Control, Device, State, Voice};

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
    pub fn pass_thru_of<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        self._pass_thru_of(name, false)
    }
    fn _pass_thru_of<'a>(&'a self, name: &'a str, went_through: bool) -> Option<&'a str> {
        if let Some(device) = self.get(name) {
            if let Some(pass_thru) = device.pass_thru() {
                self._pass_thru_of(pass_thru, true)
            } else if went_through {
                Some(name)
            } else {
                None
            }
        } else {
            None
        }
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

pub struct FrameCache {
    pub voices: HashMap<String, Voice>,
    pub controls: Vec<Control>,
    pub visited: HashSet<String>,
}
