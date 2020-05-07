#![warn(missing_docs)]

/*!
This crate defines the Ryvm specification format. RON files satisfying the `Spec` structure are used to program a Ryvm state.
*/

mod action;
mod parts;
pub use action::*;
pub use parts::*;

use std::{ops::Not, path::PathBuf};

use serde_derive::{Deserialize, Serialize};

/// A specification for a Ryvm item
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[allow(clippy::large_enum_variant)]
pub enum Spec {
    /// Load into the given channel the spec file with the given path
    Load(u8, PathBuf),
    /// A midi controller
    Controller {
        /// The name of the midi device
        device: Optional<String>,
        /// Set this to true if the controller cannot change its own midi channels
        #[serde(default, skip_serializing_if = "Not::not")]
        manual: bool,
        /// Set this to true if the controller is actually a gamepad
        ///
        /// Controls on the gamepad are mapped to midi controls.
        /// Buttons work like normal midi controller buttons.
        /// Stick and trigger axis work like midi controller knobs.
        ///
        /// The mappings are as follows:
        ///
        /// - Start: 0
        /// - Select: 1
        /// - South: 2
        /// - East: 3
        /// - West: 4
        /// - North: 5
        /// - L1: 6
        /// - R1: 7
        /// - L2: 8
        /// - R2: 9
        /// - LeftStickX: 10
        /// - LeftStickY: 11
        /// - RightStickX: 12
        /// - RightStickY: 13
        /// - DPadUp: 14
        /// - DPadDown: 15
        /// - DPadLeft: 16
        /// - DPadRight: 17
        /// - L3: 18
        /// - R3: 19
        #[serde(default, skip_serializing_if = "Not::not")]
        gamepad: bool,
        /// A list of the controls that are not global
        ///
        /// By default, every control on a midi controller is set to global.
        /// This means that it is always in effect no matter on what channel the
        /// controller is outputting.
        ///
        /// This makes sense for controls like knobs and some buttons. Physical knobs
        /// in particular, which remain in their position when you change the channel
        /// should not be added to this list.
        ///
        /// However, you may want the controls for say, a mod wheel, to be specific
        /// to a certain channel. Controls like this should be added to this list
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        non_globals: Vec<u8>,
        /// A mapping of buttons on this controller to Ryvm actions
        #[serde(default, skip_serializing_if = "Buttons::is_empty")]
        buttons: Buttons,
        /// A mapping of sliders or knobs on this controller to Ryvm  valued actions
        #[serde(default, skip_serializing_if = "Sliders::is_empty")]
        sliders: Sliders,
        /// A mapping of button ranges on this controller to ranges of Ryvm actions
        #[serde(default, skip_serializing_if = "ButtonRanges::is_empty")]
        ranges: ButtonRanges,
    },
    /// A wave synthesizer
    Wave {
        /// The waveform
        form: WaveForm,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        /// The base octave
        octave: Optional<i8>,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        /// The volume envelope attack
        attack: Optional<DynamicValue>,
        /// The volume envelope decay
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        decay: Optional<DynamicValue>,
        /// The volume envelope sustain
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        sustain: Optional<DynamicValue>,
        /// The volume envelope release
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        release: Optional<DynamicValue>,
        /// The +- pitch bend range in semitones
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        bend: Optional<DynamicValue>,
    },
    /// A drum machine with a list of paths to sample files
    Drums(Vec<PathBuf>),
    /// A low-pass filter
    Filter {
        /// The name of the input device
        input: String,
        /// The value that determines the filter's shape
        value: DynamicValue,
    },
    /// A volume and pan balancer
    Balance {
        /// The name of the input device
        input: String,
        /// The volume
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        volume: Optional<DynamicValue>,
        /// The left-right pan
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        pan: Optional<DynamicValue>,
    },
}
