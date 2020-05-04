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
    start_i: CloneCell<Option<Frame>>,
    pub controls: CloneLock<Vec<Option<ControlsMap>>>,
    tempo: f32,
    length: f32,
    pub loop_state: LoopState,
}

impl Loop {
    pub fn new(tempo: f32, length: f32) -> Self {
        Loop {
            start_i: CloneCell::new(None),
            controls: CloneLock::new(Vec::new()),
            tempo,
            length,
            loop_state: LoopState::Recording,
        }
    }
    fn loop_i(&self, state_i: Frame, state_tempo: f32) -> Option<Frame> {
        let start_i = self.start_i.load()?;

        let raw_loop_i = state_i - start_i;
        Some(adjust_i(raw_loop_i, self.tempo, state_tempo))
    }
    pub fn record(
        &mut self,
        new_controls: ControlsMap,
        state_i: Frame,
        state_tempo: f32,
        period: Option<Frame>,
    ) {
        if self.loop_state == LoopState::Recording {
            if self.start_i.load().is_none() && !new_controls.is_empty() {
                self.start_i.store(Some(state_i));
                println!("Started recording");
            }
            let loop_i = if let Some(i) = self.loop_i(state_i, state_tempo) {
                i
            } else {
                return;
            };
            let period = period.map(|p| (p as f32 * self.length).round() as Frame);

            let new_controls = Some(new_controls).filter(|map| !map.is_empty());
            let mut controls = self.controls.lock();
            if let Some(period) = period {
                controls.resize(period as usize, None);
                controls[(loop_i % period) as usize] = new_controls;
            } else {
                controls.push(new_controls);
            }
        }
    }
    pub fn controls(
        &self,
        state_i: Frame,
        state_tempo: f32,
        period: Option<Frame>,
    ) -> Option<ControlsMap> {
        if self.loop_state != LoopState::Playing {
            return None;
        }
        let period = period.map(|p| (p as f32 * self.length).round() as Frame);
        let loop_i = self.loop_i(state_i, state_tempo)?;
        if let Some(period) = period {
            self.controls.lock()[(loop_i % period) as usize].clone()
        } else {
            None
        }
    }
}
