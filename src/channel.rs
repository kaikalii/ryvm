use std::collections::{hash_map, HashMap, HashSet};

use crate::{Control, Device, State, Voice};

#[derive(Debug, Default)]
pub struct Channel {
    devices: HashMap<String, Device>,
}

impl Channel {
    pub fn get(&self, name: &str) -> Option<&Device> {
        self.devices.get(name)
    }
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Device> {
        self.devices.get_mut(name)
    }
    pub fn insert(&mut self, name: String, device: Device) {
        self.devices.insert(name, device);
    }
    pub fn insert_wrapper<F>(&mut self, input: String, name: String, build_device: F)
    where
        F: FnOnce(String) -> Device,
    {
        if self.get(&input).is_none() {
            return;
        }
        for device in self.devices.values_mut() {
            device.replace_input(input.clone(), name.clone());
        }
        let new_device = build_device(input);
        self.insert(name, new_device);
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
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&String, &mut Device) -> bool,
    {
        self.devices.retain(f)
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
    pub fn next_from(
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

pub struct FrameCache {
    pub voices: HashMap<(u8, String), Voice>,
    pub controls: HashMap<(usize, u8), Vec<Control>>,
    pub visited: HashSet<(u8, String)>,
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
    pub fn controls(&self, port: usize, channel: u8) -> impl Iterator<Item = Control> + '_ {
        self.controls
            .get(&(port, channel))
            .into_iter()
            .flat_map(|controls| controls.iter().copied())
    }
}
