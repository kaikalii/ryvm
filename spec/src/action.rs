use bimap::BiMap;
use serde_derive::{Deserialize, Serialize};

use crate::Name;

/// An action that can be mapped to a button
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case")]
pub enum Action {
    /// Start recording a new loop. If you are already recording, this
    /// finishes the current loop and starts it playing first.
    Record,
    /// Stop recording and discard anything not already in a loop
    StopRecording,
    /// Start recording a specific loop. This discard any loop
    /// currently being recorded as well as any previous content
    /// of this loop
    RecordLoop(u8),
    /// Stop a loop that is playing
    StopLoop(u8),
    /// Play a loop that was stopped
    PlayLoop(u8),
    /// Stop a loop if it is playing, play a loop if it is stopped
    ToggleLoop(u8),
    /// Play a drum pad sample on a given channel (channel, sample_index)
    Drum(u8, u8),
    /// Set the output channel of a controller
    SetOutputChannel(Name, u8),
}

/// A button to map an action to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case")]
pub enum Button {
    /// A button triggered by a control midi message
    Control(u8),
    /// A button triggered by a note start midi message
    Note(u8),
    /// A button triggered by a note start midi message on a particular channel (channel, note_index)
    ChannelNote(u8, u8),
}

/// A mapping of actions to buttons
pub type Buttons = BiMap<Action, Button>;

/// An action that takes a value that can be mapped to a slider
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case")]
pub enum ValuedAction {
    /// Sets the relative tempo
    Tempo,
    /// Sets the master volume
    MasterVolume,
}

/// A slider or knob to map a valued action to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case")]
pub enum Slider {
    /// A slider triggered by a control midi message
    Control(u8),
}

/// A mapping of valued actions
pub type Sliders = BiMap<ValuedAction, Slider>;

fn range_next(pair: &mut (u8, u8)) -> Option<u8> {
    let mut range = pair.0..pair.1;
    let res = range.next();
    pair.0 = range.start;
    res
}

/// A range of actions that can be mapped to a range of buttons
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case")]
pub enum ActionRange {
    /// Start recording a specific loop. This discard any loop
    /// currently being recorded as well as any previous content
    /// of this loop
    RecordLoop((u8, u8)),
    /// Stop a loop if it is playing, play a loop if it is stopped
    ToggleLoop((u8, u8)),
    /// Play a drum pad sample on a given channel (channel, (sample_index_start, sample_index_end))
    Drum(u8, (u8, u8)),
    /// Set the output channel of a controller (name, (channel_start, channel_end))
    SetOutputChannel(Name, (u8, u8)),
}

impl Iterator for ActionRange {
    type Item = Action;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            ActionRange::RecordLoop(range) => range_next(range).map(Action::RecordLoop),
            ActionRange::ToggleLoop(range) => range_next(range).map(Action::ToggleLoop),
            ActionRange::Drum(ch, range) => range_next(range).map(|i| Action::Drum(*ch, i)),
            ActionRange::SetOutputChannel(name, range) => {
                range_next(range).map(|ch| Action::SetOutputChannel(*name, ch))
            }
        }
    }
}

/// A range of buttons to map a ranged action to
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case")]
pub enum ButtonRange {
    /// A button triggered by a control midi message
    Control((u8, u8)),
    /// A button triggered by a note start midi message
    Note((u8, u8)),
    /// A button triggered by a note start midi message on a
    /// particular channel (channel, (note_index_start, note_index_end))
    ChannelNote(u8, (u8, u8)),
}

impl Iterator for ButtonRange {
    type Item = Button;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            ButtonRange::Control(range) => range_next(range).map(Button::Control),
            ButtonRange::Note(range) => range_next(range).map(Button::Note),
            ButtonRange::ChannelNote(ch, range) => {
                range_next(range).map(|i| Button::ChannelNote(*ch, i))
            }
        }
    }
}

/// A mapping of action ranges
pub type ButtonRanges = BiMap<ActionRange, ButtonRange>;
