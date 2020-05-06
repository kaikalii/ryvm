use bimap::BiMap;
use serde_derive::{Deserialize, Serialize};

/// A control that can be mapped to a button
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case")]
pub enum Action {
    /// Start recording a new loop. If you are already recording, this
    /// finishes the current loop and starts it playing first.
    Record,
    /// Stop recording and discard anything not already in a loop
    StopRecording,
    /// Stop a loop for playing
    StopLoop(u8),
    /// Play a loop that was stopped
    PlayLoop(u8),
    /// Stop a loop if it is playing, play a loop if it is stopped
    ToggleLoop(u8),
    /// Play a drum pad sample on a given channel (channel, sample_index)
    Drum(u8, u8),
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
