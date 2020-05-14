use std::{
    hash::{Hash, Hasher},
    path::PathBuf,
};

use arrayvec::ArrayString;
use serde_derive::{Deserialize, Serialize};

/// The total number of bytes that can be in a name
pub const NAME_CAPACITY: usize = 20;

/// The name of a controller or node
pub type Name = ArrayString<[u8; NAME_CAPACITY]>;

use super::default;

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
/// A type of filter
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamepadControl {
    South,
    East,
    West,
    North,
    Start,
    Select,
    L1,
    R1,
    L2,
    R2,
    L3,
    R3,
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl From<GamepadControl> for u8 {
    fn from(gc: GamepadControl) -> u8 {
        gc as u8
    }
}

#[derive(Debug, Clone, Copy, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", untagged)]
pub enum GenericControl {
    Midi(u8),
    Gamepad(GamepadControl),
}

impl From<GenericControl> for u8 {
    fn from(gc: GenericControl) -> Self {
        match gc {
            GenericControl::Midi(i) => i,
            GenericControl::Gamepad(gc) => gc.into(),
        }
    }
}

impl From<u8> for GenericControl {
    fn from(u: u8) -> Self {
        GenericControl::Midi(u)
    }
}

impl PartialEq for GenericControl {
    fn eq(&self, other: &Self) -> bool {
        u8::from(*self) == u8::from(*other)
    }
}

impl Hash for GenericControl {
    fn hash<H>(&self, hasher: &mut H)
    where
        H: Hasher,
    {
        hasher.write_u8(u8::from(*self));
    }
}

/// A value that can be either a static number, mapped to a midi control,
/// or mapped to a node output
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields, untagged)]
pub enum DynamicValue {
    /// A static number
    Static(f32),
    /// A midi control mapping
    Control {
        /// The midi control
        index: GenericControl,
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
    /// The value output by another node
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

/// A set of values defining an attack-decay-sustain-release envelope
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename = "snake_case", deny_unknown_fields, default)]
pub struct ADSR<T> {
    pub attack: T,
    pub decay: T,
    pub sustain: T,
    pub release: T,
}

impl<T> ADSR<T> {
    pub fn map<F, U>(&self, mut f: F) -> ADSR<U>
    where
        F: FnMut(&T) -> U,
    {
        ADSR {
            attack: f(&self.attack),
            decay: f(&self.decay),
            sustain: f(&self.sustain),
            release: f(&self.release),
        }
    }
    pub fn map_or_default<F>(&self, mut f: F) -> ADSR<f32>
    where
        F: FnMut(&T) -> Option<f32>,
    {
        ADSR {
            attack: f(&self.attack).unwrap_or_else(|| default::ATTACK.unwrap_static()),
            decay: f(&self.decay).unwrap_or_else(|| default::DECAY.unwrap_static()),
            sustain: f(&self.sustain).unwrap_or_else(|| default::SUSTAIN.unwrap_static()),
            release: f(&self.release).unwrap_or_else(|| default::RELEASE.unwrap_static()),
        }
    }
}

impl Default for ADSR<f32> {
    fn default() -> Self {
        ADSR {
            attack: default::ATTACK.unwrap_static(),
            decay: default::DECAY.unwrap_static(),
            sustain: default::SUSTAIN.unwrap_static(),
            release: default::RELEASE.unwrap_static(),
        }
    }
}

impl Default for ADSR<DynamicValue> {
    fn default() -> Self {
        ADSR {
            attack: default::ATTACK,
            decay: default::DECAY,
            sustain: default::SUSTAIN,
            release: default::RELEASE,
        }
    }
}

impl ADSR<DynamicValue> {
    pub fn inputs(&self) -> impl Iterator<Item = &str> {
        self.attack
            .input()
            .into_iter()
            .chain(self.decay.input())
            .chain(self.sustain.input())
            .chain(self.release.input())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleDef {
    pub path: PathBuf,
    pub loop_start: f32,
    pub pitch: f32,
}
