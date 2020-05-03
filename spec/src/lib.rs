#![warn(missing_docs)]

/*!
This crate defines the Ryvm specification format. RON files satisfying the `Spec` structure are used to program a Ryvm state.
*/

mod parts;
pub use parts::*;

use std::{ops::Not, path::PathBuf};

use serde_derive::{Deserialize, Serialize};

/// A specification for a Ryvm item
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Spec {
    /// Load into the given channel the spec file with the given path
    Load(u8, PathBuf),
    /// A midi controller
    Controller {
        /// The name of the midi device
        device: Optional<String>,
        /// The channel that drum pads on the controller use
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        pad_channel: Optional<u8>,
        /// The midi note index range that drum pads on the controller fall into
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        pad_range: Optional<(u8, u8)>,
        /// Set this to true if the controller cannot change its own midi channels
        #[serde(default, skip_serializing_if = "Not::not")]
        manual: bool,
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
        /// The midi control number used to start and end loops
        record: Optional<u8>,
        /// The midi control number used to stop recording loops
        stop_record: Optional<u8>,
    },
    /// A wave synthesizer
    Wave {
        /// The waveform
        form: WaveForm,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        /// The base octave
        octave: Optional<i8>,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        /// The envelope attack
        attack: Optional<DynamicValue>,
        /// The envelope decay
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        decay: Optional<DynamicValue>,
        /// The envelope sustain
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        sustain: Optional<DynamicValue>,
        /// The envelope release
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
