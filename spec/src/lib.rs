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
#[serde(rename_all = "snake_case")]
pub enum Spec {
    /// Load all spec files with the given names from the `specs` directory
    Load(Vec<String>),
    /// A midi controller
    Controller {
        /// The midi port to use
        port: Optional<usize>,
        /// The channel that drum pads on the controller use
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        pad_channel: Optional<u8>,
        /// The midi note index range that drum pads on the controller fall into
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        pad_range: Optional<(u8, u8)>,
        /// Set this to true if the controller cannot change its own midi channels
        #[serde(default, skip_serializing_if = "Not::not")]
        manual: bool,
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

/// A waveform
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum WaveForm {
    Sine,
    Square,
    Saw,
    Triangle,
    Noise,
}

impl Default for WaveForm {
    fn default() -> Self {
        WaveForm::Sine
    }
}

#[cfg(test)]
#[test]
fn example() {
    use ron::ser::*;

    let specs = vec![
        Spec::Drums(vec!["kick.wav".into(), "clap.wav".into()]),
        Spec::Filter {
            input: "drums".into(),
            value: DynamicValue::Static(1.0),
        },
        Spec::Filter {
            input: "drums".into(),
            value: DynamicValue::Control {
                controller: Omitted,
                number: 28,
                global: false,
                bounds: (0.0, 1.0),
            },
        },
    ];

    let config = PrettyConfig::default();
    std::fs::write(
        "../ryvm_spec.ron",
        to_string_pretty(&specs, config).unwrap().as_bytes(),
    )
    .unwrap();
}

#[cfg(test)]
#[test]
fn deserialize() {
    ron::de::from_reader::<_, Vec<Spec>>(std::fs::File::open("../ryvm_spec.ron").unwrap()).unwrap();
}
