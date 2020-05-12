use std::{
    collections::{BTreeMap, HashMap, HashSet},
    mem::swap,
};

use serde_derive::{Deserialize, Serialize};

use crate::{colorprintln, Control, Float, Name, Port, Voice};

#[derive(Debug, Clone, Copy)]
pub struct LoopMaster {
    pub period: f32,
    pub num: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopDef {
    pub controls: BTreeMap<Float, ControlsMap>,
    pub length: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Recording,
    Playing,
    Disabled,
}

pub type ControlsMap = HashMap<(Port, u8), Vec<Control>>;
pub type SamplesMap = HashMap<Name, Voice>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "LoopDef", into = "LoopDef")]
pub struct Loop {
    started: bool,
    pub controls: BTreeMap<Float, ControlsMap>,
    note_ids: HashSet<(u64, u8)>,
    length: f32,
    pub loop_state: LoopState,
    t: f32,
    last_t: f32,
    speed: f32,
    speed_queue: Option<f32>,
}

impl From<LoopDef> for Loop {
    fn from(ld: LoopDef) -> Self {
        Loop {
            started: true,
            controls: ld.controls,
            note_ids: HashSet::new(),
            length: ld.length,
            loop_state: LoopState::Disabled,
            t: 0.0,
            last_t: 0.0,
            speed: 1.0,
            speed_queue: None,
        }
    }
}

impl From<Loop> for LoopDef {
    fn from(lup: Loop) -> LoopDef {
        LoopDef {
            controls: lup.controls,
            length: lup.length,
        }
    }
}

impl Loop {
    pub fn new(length: f32) -> Self {
        Loop {
            started: false,
            loop_state: LoopState::Recording,
            ..Loop::from(LoopDef {
                controls: BTreeMap::new(),
                length,
            })
        }
    }
    pub fn t(&self) -> f32 {
        self.t
    }
    pub fn set_t(&mut self, t: f32) {
        self.t = t;
        self.last_t = t;
    }
    pub fn set_speed(&mut self, speed: f32) {
        self.speed_queue = Some(speed);
    }
    pub fn record<F>(&mut self, new_controls: ControlsMap, get_advance: F)
    where
        F: Fn(Port) -> Option<f32>,
    {
        if self.loop_state == LoopState::Recording {
            if !self.started && !new_controls.is_empty() {
                self.started = true;
                colorprintln!("Started recording", bright_magenta);
            }
            if !self.started {
                return;
            }

            if !new_controls.is_empty() {
                for ((port, ch), controls) in new_controls {
                    let advance = get_advance(port).unwrap_or(0.0);
                    let t = self.t - advance;
                    self.controls
                        .entry(Float(t))
                        .or_insert_with(HashMap::new)
                        .insert((port, ch), controls);
                }
            }
        }
    }
    /// Get the map of controls for the current frame
    pub fn controls(&mut self, state_tempo: f32, period: Option<f32>) -> Option<ControlsMap> {
        let res = if self.loop_state == LoopState::Playing {
            let period = period.expect("Playing loop was not supplied a period");
            if self.last_t > self.t {
                if let Some(new_speed) = self.speed_queue.take() {
                    self.speed = new_speed;
                }
            }
            let t = (self.t * self.speed) % period;
            let mut combined_map = HashMap::new();
            if self.last_t <= t {
                for (_, controls) in self.controls.range(Float(self.last_t)..Float(t)) {
                    for (key, list) in controls {
                        combined_map
                            .entry(*key)
                            .or_insert_with(Vec::new)
                            .extend(list.iter().copied());
                    }
                }
            } else {
                for (_, controls) in self
                    .controls
                    .range(Float(self.last_t)..)
                    .chain(self.controls.range(..Float(t)))
                {
                    for (key, list) in controls {
                        combined_map
                            .entry(*key)
                            .or_insert_with(Vec::new)
                            .extend(list.iter().copied());
                    }
                }
            }
            Some(combined_map).filter(|map| !map.is_empty())
        } else {
            None
        };
        if self.started {
            self.last_t = if let Some(period) = period {
                (self.t * self.speed) % period
            } else {
                self.t
            };
            self.t += state_tempo;
            let period = period.map(|p| p * self.length);
            if let Some(period) = period {
                if self.loop_state != LoopState::Recording && self.t >= period.floor() {
                    self.t = 0.0;
                }
            }
        }
        res
    }
    pub fn period(&self) -> f32 {
        if let (Some(start), Some(end)) = (self.controls.keys().next(), self.controls.keys().last())
        {
            end.0 - start.0
        } else {
            0.0
        }
    }
    pub fn base_period(&self) -> f32 {
        self.period() / self.length
    }
    pub fn finish(&mut self, period: Option<f32>) {
        if let LoopState::Recording = self.loop_state {
            if self.controls.is_empty() {
                return;
            }
            self.loop_state = LoopState::Playing;
            // Collect a set of all port-channel-id-note quartets
            let mut note_midi_channels = HashSet::new();
            for control_map in self.controls.values() {
                for ((port, ch), controls) in control_map.iter() {
                    for control in controls {
                        if let Control::NoteStart(id, n, _) = control {
                            self.note_ids.insert((*id, *n));
                            note_midi_channels.insert((*port, *ch, *id, *n));
                        }
                    }
                }
            }
            // Insert a note end for each note at the end of the loop
            let end_t = self.t;
            let last = self
                .controls
                .entry(Float(end_t))
                .or_insert_with(HashMap::new);
            for (port, ch, id, n) in note_midi_channels {
                last.entry((port, ch))
                    .or_insert_with(Vec::new)
                    .push(Control::NoteEnd(id, n));
            }
            // Only use notes that lie within the period
            if let Some(period) = period {
                let start_t = end_t - period;
                // Ensure the starting map exists
                self.controls
                    .entry(Float(start_t))
                    .or_insert_with(HashMap::new);
                // Split off notes that lie within the period and adjust the key
                let used: BTreeMap<_, _> = self
                    .controls
                    .split_off(&Float(start_t))
                    .into_iter()
                    .map(|(key, map)| (Float(key.0 - start_t), map))
                    .collect();
                self.controls = used;
            }
            // Reset i
            self.t = 0.0;
        }
    }
    pub fn note_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.note_ids.iter().map(|(id, _)| *id)
    }
    pub fn set_period(&mut self, new_period: f32) {
        let base_period = self.base_period();
        let mut new_controls = BTreeMap::new();
        swap(&mut new_controls, &mut self.controls);
        self.controls = new_controls
            .into_iter()
            .map(|(t, controls)| (Float(t.0 * new_period / base_period), controls))
            .collect()
    }
}
