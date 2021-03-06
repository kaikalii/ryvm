/*!
This crate defines the Ryvm specification format. TOML files satisfying the `Spec` structure are used to program a Ryvm state.
*/

mod action;
pub mod default;
mod parts;
pub use action::*;
pub use parts::*;

use std::path::PathBuf;

use indexmap::IndexMap;
use serde_derive::{Deserialize, Serialize};

/// A specification for a Ryvm item
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields, tag = "type")]
#[allow(clippy::large_enum_variant)]
pub enum Spec {
    /// Load spec file with the given paths (relative to the specs directory) into the given channels
    Load {
        /// The paths and the channels into which to load them
        paths: IndexMap<PathBuf, u8>,
    },
    /// A midi controller
    Controller {
        /// The name of the midi device
        device: Option<String>,
        /// The midi channel to which this device should output.
        ///
        /// Only speficy this for devices which cannot select their own channels.
        /// This is evaluated before all other other channel mappings.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_channel: Option<u8>,
        /// Set this to a gamepad index if this controller is a gamepad
        ///
        /// Controls on the gamepad are mapped to midi controls.
        /// Buttons work like normal midi controller buttons.
        /// Stick and trigger axis work like midi controller knobs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gamepad: Option<usize>,
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
        button: Buttons,
        /// A mapping of sliders or knobs on this controller to Ryvm  valued actions
        #[serde(default, skip_serializing_if = "Sliders::is_empty")]
        slider: Sliders,
        /// A mapping of button ranges on this controller to ranges of Ryvm actions
        #[serde(default, skip_serializing_if = "ButtonRanges::is_empty")]
        range: ButtonRanges,
    },
    /// An audio input device
    Input {
        /// The system-supplied name of the input device (devices can be listed with the `inputs` command)
        ///
        /// If this field is not specified, the default input device will be chosen
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<String>,
    },
    /// A channel-bound interface for an audio input device
    InputPass {
        /// The user-supplied name of the input device
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<Name>,
    },
    /// A wave synthesizer
    Wave {
        /// The waveform
        form: WaveForm,
        /// The base octave
        #[serde(
            default = "default::octave",
            skip_serializing_if = "default::is_octave"
        )]
        octave: i8,
        /// The volume envelope attack
        #[serde(
            default = "default::attack",
            skip_serializing_if = "default::is_attack"
        )]
        attack: DynamicValue,
        /// The volume envelope decay
        #[serde(default = "default::decay", skip_serializing_if = "default::is_decay")]
        decay: DynamicValue,
        /// The volume envelope sustain
        #[serde(
            default = "default::sustain",
            skip_serializing_if = "default::is_sustain"
        )]
        sustain: DynamicValue,
        /// The volume envelope release
        #[serde(
            default = "default::release",
            skip_serializing_if = "default::is_release"
        )]
        release: DynamicValue,
        /// The +- pitch bend range in semitones
        #[serde(
            default = "default::bend_range",
            skip_serializing_if = "default::is_bend_range"
        )]
        bend: DynamicValue,
        /// The sustain pedal control
        #[serde(
            default = "default::sustain_pedal",
            skip_serializing_if = "default::is_sustain_pedal"
        )]
        sustain_pedal: DynamicValue,
    },
    /// A drum machine with a list of paths to sample files
    Drums {
        /// The paths to the sample audio files (relative to the ryvm samples directory)
        paths: Option<Vec<PathBuf>>,
        /// The path to a folder containing audio files (relative to the ryvm samples directory)
        folder: Option<PathBuf>,
    },
    /// A low-pass filter
    Filter {
        /// The name of the input device
        input: Option<Name>,
        /// The value that determines the filter's shape
        value: DynamicValue,
        /// The type of filter
        #[serde(
            default = "default::filter_type",
            skip_serializing_if = "default::is_filter_type"
        )]
        filter: FilterType,
        /// The optional ADSR envelope
        #[serde(default, skip_serializing_if = "Option::is_none")]
        adsr: Option<ADSR<DynamicValue>>,
        /// The sustain pedal control
        #[serde(
            default = "default::sustain_pedal",
            skip_serializing_if = "default::is_sustain_pedal"
        )]
        sustain_pedal: DynamicValue,
    },
    /// A volume and pan balancer
    Balance {
        /// The name of the input device
        input: Option<Name>,
        /// The volume
        #[serde(
            default = "default::volume",
            skip_serializing_if = "default::is_volume"
        )]
        volume: DynamicValue,
        /// The left-right pan
        #[serde(default = "default::pan", skip_serializing_if = "default::is_pan")]
        pan: DynamicValue,
    },
    /// A reverb simulator
    Reverb {
        /// The name of the input device
        input: Option<Name>,
        /// The simulated room size
        #[serde(
            default = "default::room_size",
            skip_serializing_if = "default::is_room_size"
        )]
        size: DynamicValue,
        /// The simulated energy multiplier
        #[serde(
            default = "default::energy_mul",
            skip_serializing_if = "default::is_energy_mul"
        )]
        energy_mul: DynamicValue,
    },
    /// A pitch-changing sampler
    Sampler {
        /// The sample definition
        def: SampleDef,
        /// The optional ADSR envelope
        #[serde(
            default = "default::adsr_env",
            skip_serializing_if = "default::is_adsr_env"
        )]
        adsr: ADSR<DynamicValue>,
        /// The sustain pedal control
        #[serde(
            default = "default::sustain_pedal",
            skip_serializing_if = "default::is_sustain_pedal"
        )]
        sustain_pedal: DynamicValue,
    },
}
