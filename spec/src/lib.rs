mod parts;
pub use parts::*;

use std::path::PathBuf;

use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Spec {
    Wave {
        waveform: WaveForm,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        octave: Optional<i8>,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        attack: Optional<f32>,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        decay: Optional<f32>,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        sustain: Optional<f32>,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        release: Optional<f32>,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        bend: Optional<f32>,
    },
    Drums(Vec<PathBuf>),
    Filter {
        input: String,
        value: DynamicValue,
    },
    Balance {
        input: String,
        value: DynamicValue,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        volume: Optional<f32>,
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        pan: Optional<f32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
                channel: Supplied(0),
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
