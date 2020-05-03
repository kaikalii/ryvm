use std::ops::Not;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_derive::{Deserialize as Deser, Serialize as Ser};

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

/// A value that can be either a static number or mapped to a midi control
#[derive(Debug, Clone, Ser, Deser)]
#[serde(rename_all = "snake_case")]
pub enum DynamicValue {
    /// A static number
    Static(f32),
    /// A midi control mapping
    Control {
        /// The name of the midi controller
        #[serde(default, skip_serializing_if = "Optional::is_omitted")]
        controller: Optional<String>,
        /// The midi control number
        number: u8,
        /// Whether this is a global control
        ///
        /// Global controls are active even if the midi controller is set to output a different channel
        #[serde(default, skip_serializing_if = "Not::not")]
        global: bool,
        /// The minimum and maxinum values this control should map to
        #[serde(default = "default_bounds", skip_serializing_if = "is_default_bounds")]
        bounds: (f32, f32),
    },
}

fn default_bounds() -> (f32, f32) {
    (0.0, 1.0)
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_default_bounds(bounds: &(f32, f32)) -> bool {
    bounds == &default_bounds()
}

impl From<f32> for DynamicValue {
    fn from(f: f32) -> Self {
        DynamicValue::Static(f)
    }
}

/// An optional that can be omitted
///
/// Optionals that are not given a value typically choose some sensible default
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
    pub(crate) fn is_omitted(&self) -> bool {
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
