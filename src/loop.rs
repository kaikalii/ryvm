use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{Control, Float, Port};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Recording,
    Playing,
    Disabled,
}

pub type ControlsMap = HashMap<(Port, u8), Vec<Control>>;

#[derive(Debug, Clone)]
pub struct Loop {
    started: bool,
    pub controls: BTreeMap<Float, ControlsMap>,
    note_ids: HashSet<(u64, u8)>,
    length: f32,
    pub loop_state: LoopState,
    i: f32,
    last_i: f32,
}

impl Loop {
    pub fn new(length: f32) -> Self {
        Loop {
            started: false,
            controls: BTreeMap::new(),
            note_ids: HashSet::new(),
            length,
            loop_state: LoopState::Recording,
            i: 0.0,
            last_i: 0.0,
        }
    }
    pub fn record(&mut self, new_controls: ControlsMap) {
        if self.loop_state == LoopState::Recording {
            if !self.started && !new_controls.is_empty() {
                self.started = true;
                println!("Started recording");
            }
            if !self.started {
                return;
            }

            if !new_controls.is_empty() {
                self.controls.insert(Float(self.i), new_controls);
            }
        }
    }
    /// Get the map of controls for the current frame
    pub fn controls(&mut self, state_tempo: f32, period: Option<f32>) -> Option<ControlsMap> {
        let res = if self.loop_state == LoopState::Playing {
            let mut combined_map = HashMap::new();
            if self.last_i <= self.i {
                for (_, controls) in self.controls.range(Float(self.last_i)..Float(self.i)) {
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
                    .range(Float(self.last_i)..)
                    .chain(self.controls.range(..Float(self.i)))
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
            self.last_i = self.i;
            self.i += state_tempo;
            let period = period.map(|p| p * self.length);
            if let Some(period) = period {
                if self.loop_state != LoopState::Recording && self.i >= period.floor() {
                    self.i = 0.0;
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
    pub fn finish(&mut self, period: Option<f32>) {
        if let LoopState::Recording = self.loop_state {
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
            let end_i = self.i;
            let last = self
                .controls
                .entry(Float(end_i))
                .or_insert_with(HashMap::new);
            for (port, ch, id, n) in note_midi_channels {
                last.entry((port, ch))
                    .or_insert_with(Vec::new)
                    .push(Control::NoteEnd(id, n));
            }
            // Only use notes that lie within the period
            if let Some(period) = period {
                let start_i = end_i - period;
                // Ensure the starting map exists
                self.controls
                    .entry(Float(start_i))
                    .or_insert_with(HashMap::new);
                // Split off notes that lie within the period and adjust the key
                let used: BTreeMap<_, _> = self
                    .controls
                    .split_off(&Float(start_i))
                    .into_iter()
                    .map(|(key, map)| (Float(key.0 - start_i), map))
                    .collect();
                self.controls = used;
                dbg!(self.period());
                dbg!(period);
            }
            // Reset i
            self.i = 0.0;
        }
    }
    pub fn note_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.note_ids.iter().map(|(id, _)| *id)
    }
}
