use std::collections::HashMap;

use crate::{adjust_i, CloneCell, CloneLock, Control, Frame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Recording,
    Playing,
    Disabled,
}

pub type ControlsMap = HashMap<(usize, u8), Vec<Control>>;

#[derive(Debug, Clone)]
pub struct Loop {
    started: CloneCell<bool>,
    pub controls: CloneLock<Vec<Option<ControlsMap>>>,
    tempo: f32,
    length: f32,
    pub loop_state: LoopState,
    i: Frame,
}

impl Loop {
    pub fn new(tempo: f32, length: f32) -> Self {
        Loop {
            started: CloneCell::new(false),
            controls: CloneLock::new(Vec::new()),
            tempo,
            length,
            loop_state: LoopState::Recording,
            i: 0,
        }
    }
    fn loop_i(&self, state_tempo: f32) -> Frame {
        adjust_i(self.i, self.tempo, state_tempo)
    }
    pub fn record(&mut self, new_controls: ControlsMap, state_tempo: f32, period: Option<Frame>) {
        if self.loop_state == LoopState::Recording {
            if !self.started.load() && !new_controls.is_empty() {
                self.started.store(true);
                println!("Started recording");
            }
            if !self.started.load() {
                return;
            }
            let period = period.map(|p| (p as f32 * self.length).round() as Frame);

            let new_controls = Some(new_controls).filter(|map| !map.is_empty());
            let mut controls = self.controls.lock();
            if let Some(period) = period {
                let loop_i = self.loop_i(state_tempo);
                controls.resize(period as usize, None);
                controls[(loop_i % period) as usize] = new_controls;
            } else {
                controls.push(new_controls);
            }
        }
    }
    pub fn controls(&mut self, state_tempo: f32, period: Option<Frame>) -> Option<ControlsMap> {
        let period = period.map(|p| (p as f32 * self.length).round() as Frame);
        if let Some(period) = period {
            if self.started.load() {
                self.i += 1;
                if self.i >= period {
                    self.i = 0;
                }
            }
        }
        if self.loop_state != LoopState::Playing {
            return None;
        }
        let loop_i = self.loop_i(state_tempo);
        if let Some(period) = period {
            self.controls.lock()[(loop_i % period) as usize].clone()
        } else {
            None
        }
    }
}
