use bimap::BiMap;
use serde_derive::{Deserialize, Serialize};

use crate::Name;

/// A mapping from an action to a control type
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Mapping<A, B> {
    /// The action value
    pub action: A,
    /// The control value
    pub control: B,
}

/// An action that can be mapped to a button
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case", tag = "type")]
pub enum Action {
    /// Start recording a new loop. If you are already recording, this
    /// finishes the current loop and starts it playing first.
    Record,
    /// Stop recording and discard anything not already in a loop
    StopRecording,
    /// Start recording a specific loop. This discard any loop
    /// currently being recorded as well as any previous content
    /// of this loop
    RecordLoop {
        /// The loop number to start recording
        num: u8,
    },
    /// Stop a loop that is playing
    StopLoop {
        /// The loop number to stop playing
        num: u8,
    },
    /// Play a loop that was stopped
    PlayLoop {
        /// The loop number to start playing
        num: u8,
    },
    /// Stop a loop if it is playing, play a loop if it is stopped
    ToggleLoop {
        /// The loop number to toggle
        num: u8,
    },
    /// Play a drum pad sample on a given channel
    Drum {
        /// The channel
        channel: u8,
        /// The sample index
        index: u8,
    },
    /// Set the output channel of a controller
    SetOutputChannel {
        /// The controller name
        name: Name,
        /// The channel
        channel: u8,
    },
}

/// A button to map an action to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case", tag = "type")]
pub enum Button {
    /// A button triggered by a control midi message
    Control {
        /// The index of the control
        index: u8,
    },
    /// A button triggered by a note start midi message
    Note {
        /// The index of the note
        index: u8,
    },
    /// A button triggered by a note start midi message on a particular channel (channel, note_index)
    ChannelNote {
        /// The channel
        channel: u8,
        /// The index of the note
        index: u8,
    },
}

/// A mapping of actions to buttons
pub type Buttons = Vec<Mapping<Action, Button>>;
#[doc(hidden)]
pub type ButtonsMap = BiMap<Action, Button>;

/// An action that takes a value that can be mapped to a slider
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case", tag = "type")]
pub enum ValuedAction {
    /// Sets the relative tempo
    Tempo,
    /// Sets the master volume
    MasterVolume,
    /// Set the playback speed of a loop.
    ///
    /// Only uses multiples of 2.
    /// 0-1 maps to 1-8
    LoopSpeed {
        /// The number of the loop
        num: u8,
    },
}

/// A slider or knob to map a valued action to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case", tag = "type")]
pub enum Slider {
    /// A slider triggered by a control midi message
    Control {
        /// The index of the control
        index: u8,
    },
}

/// A mapping of valued actions
pub type Sliders = Vec<Mapping<ValuedAction, Slider>>;
#[doc(hidden)]
pub type SlidersMap = BiMap<ValuedAction, Slider>;

fn range_next(bounds: &mut (u8, u8)) -> Option<u8> {
    let mut range = bounds.0..bounds.0;
    let res = range.next();
    bounds.0 = range.start;
    res
}

/// A range of actions that can be mapped to a range of buttons
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case", tag = "type")]
pub enum ActionRange {
    /// Start recording a specific loop. This discard any loop
    /// currently being recorded as well as any previous content
    /// of this loop
    RecordLoop {
        /// The loop number range bounds
        bounds: (u8, u8),
    },
    /// Stop a loop number if it is playing, play a loop if it is stopped
    ToggleLoop {
        /// The loop number range bounds
        bounds: (u8, u8),
    },
    /// Play a drum pad sample on a given channel
    Drum {
        /// The channel
        channel: u8,
        /// The drum pad note index range bounds
        bounds: (u8, u8),
    },
    /// Set the output channel of a controller
    SetOutputChannel {
        /// The controller name
        name: Name,
        /// The channel range bounds
        bounds: (u8, u8),
    },
}

impl Iterator for ActionRange {
    type Item = Action;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            ActionRange::RecordLoop { bounds } => {
                range_next(bounds).map(|num| Action::RecordLoop { num })
            }
            ActionRange::ToggleLoop { bounds } => {
                range_next(bounds).map(|num| Action::ToggleLoop { num })
            }
            ActionRange::Drum { channel, bounds } => range_next(bounds).map(|i| Action::Drum {
                channel: *channel,
                index: i,
            }),
            ActionRange::SetOutputChannel { name, bounds } => {
                range_next(bounds).map(|ch| Action::SetOutputChannel {
                    name: *name,
                    channel: ch,
                })
            }
        }
    }
}

/// A range of buttons to map a ranged action to
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case", rename_all = "snake_case", tag = "type")]
pub enum ButtonRange {
    /// A button triggered by a control midi message
    Control {
        /// The range bounds
        bounds: (u8, u8),
    },
    /// A button triggered by a note start midi message
    Note {
        /// The range bounds
        bounds: (u8, u8),
    },
    /// A button triggered by a note start midi message on a particular channel
    ChannelNote {
        /// The channel
        channel: u8,
        /// The range bounds
        bounds: (u8, u8),
    },
}

impl Iterator for ButtonRange {
    type Item = Button;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            ButtonRange::Control { bounds } => {
                range_next(bounds).map(|i| Button::Control { index: i })
            }
            ButtonRange::Note { bounds } => range_next(bounds).map(|i| Button::Note { index: i }),
            ButtonRange::ChannelNote { channel, bounds } => {
                range_next(bounds).map(|i| Button::ChannelNote {
                    channel: *channel,
                    index: i,
                })
            }
        }
    }
}

/// A mapping of action ranges
pub type ButtonRanges = Vec<Mapping<ActionRange, ButtonRange>>;
