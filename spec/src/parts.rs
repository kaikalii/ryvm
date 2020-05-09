use arrayvec::ArrayString;
use serde::{Deserialize, Serialize};
use serde_derive::{Deserialize as Deser, Serialize as Ser};

use crate::default;

/// The total number of bytes that can be in a name
pub const NAME_CAPACITY: usize = 20;

/// The name of a controller or device
pub type Name = ArrayString<[u8; NAME_CAPACITY]>;

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

/// A type of filter
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterType {
    /// A basic low-pass filter
    ///
    /// This is the default filter type
    LowPass,
    /// A comb filter
    Comb,
    /// A resolution-reducing filter
    Crush,
    /// A distortion filter
    Distortion,
}

impl Default for FilterType {
    fn default() -> Self {
        FilterType::LowPass
    }
}

/// A value that can be either a static number, mapped to a midi control,
/// or mapped to a device output
#[derive(Debug, Clone, Copy, PartialEq, Ser, Deser)]
#[serde(rename_all = "snake_case", deny_unknown_fields, untagged)]
pub enum DynamicValue {
    /// A static number
    Static(f32),
    /// A midi control mapping
    Control {
        /// The midi control index
        index: u8,
        /// The name of the midi controller
        #[serde(default, skip_serializing_if = "Option::is_none")]
        controller: Option<Name>,
        /// The minimum and maxinum values this control should map to
        #[serde(
            default = "default::bounds",
            skip_serializing_if = "default::is_bounds"
        )]
        bounds: (f32, f32),
        /// The default value that will be used before the control is touched
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<f32>,
    },
    /// The value output by another device
    Output(Name),
}

impl From<f32> for DynamicValue {
    fn from(f: f32) -> Self {
        DynamicValue::Static(f)
    }
}

impl DynamicValue {
    #[doc(hidden)]
    pub fn input(&self) -> Option<&str> {
        if let DynamicValue::Output(s) = self {
            Some(s)
        } else {
            None
        }
    }
    #[doc(hidden)]
    pub fn unwrap_static(self) -> f32 {
        if let DynamicValue::Static(f) = self {
            f
        } else {
            panic!("Called DynamicValue::unwrap_static on a non-static value")
        }
    }
}
