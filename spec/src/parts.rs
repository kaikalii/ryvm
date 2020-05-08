use arrayvec::ArrayString;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DynamicValue {
    /// A static number
    Static(f32),
    /// A midi control mapping
    Control {
        /// The name of the midi controller
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        controller: Optional<Name>,
        /// The midi control number
        number: Required<u8>,
        /// The minimum and maxinum values this control should map to
        #[serde(
            default = "default::bounds",
            skip_serializing_if = "default::is_bounds"
        )]
        bounds: (f32, f32),
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

/// A value that is required
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Required<T>(pub T);

impl<T> From<T> for Required<T> {
    fn from(val: T) -> Self {
        Required(val)
    }
}

/// An setting that may have no value
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Optional<T> {
    /// The option was supplied by the user
    Supplied(T),
    /// The option was omitted by the user
    Omitted,
}

pub use Optional::*;

impl<T> Default for Optional<T> {
    fn default() -> Self {
        Omitted
    }
}

impl<T> From<Option<T>> for Optional<T> {
    fn from(op: Option<T>) -> Self {
        match op {
            Some(val) => Supplied(val),
            None => Omitted,
        }
    }
}

impl<T> From<Optional<T>> for Option<T> {
    fn from(op: Optional<T>) -> Self {
        match op {
            Supplied(val) => Some(val),
            Omitted => None,
        }
    }
}

impl<T> Optional<T> {
    #[doc(hidden)]
    pub fn or<U>(self, default: U) -> T
    where
        U: Into<T>,
    {
        match self {
            Supplied(val) => val,
            Omitted => default.into(),
        }
    }
    #[doc(hidden)]
    pub fn or_else<F>(self, default: F) -> T
    where
        F: FnOnce() -> T,
    {
        match self {
            Supplied(val) => val,
            Omitted => default(),
        }
    }
    #[doc(hidden)]
    pub fn is_omitted(&self) -> bool {
        matches!(self, Omitted)
    }
}

impl<T> Serialize for Optional<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Supplied(val) => val.serialize(serializer),
            Omitted => serializer.serialize_none(),
        }
    }
}

impl<'de, T> Deserialize<'de> for Optional<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(T::deserialize(deserializer)
            .map(Supplied)
            .unwrap_or(Omitted))
    }
}
