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
}

/// A button to map a control to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case")]
pub enum Button {
    /// A button triggered by a control midi message
    Control(u8),
    /// A button triggered by a note start midi message
    Note(u8),
}

/// A mapping of controls to buttons
pub type Buttons = BiMap<Action, Button>;
