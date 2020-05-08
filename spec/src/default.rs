//! All the Ryvm spec default values
use crate::{DynamicValue, FilterType};

macro_rules! default {
    (#[$attr:meta] const $constant:ident: $type:ty = $val:expr; $def_fn_name:ident; $is_def_fn_name:ident;) => {
        #[$attr]
        pub const $constant: $type = $val;
        pub(crate) fn $def_fn_name() -> $type {
            $constant
        }
        #[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
        pub(crate) fn $is_def_fn_name(val: &$type) -> bool {
            val == &$constant
        }
    };
}

default! {
    /// The default control bounds
    const BOUNDS: (f32, f32) = (0.0, 1.0);
    bounds;
    is_bounds;
}

default! {
    /// The default wave octave
    const OCTAVE: i8 = 0;
    octave;
    is_octave;
}

default! {
    /// The default pitch bend range
    const BEND_RANGE: DynamicValue = DynamicValue::Static(12.0);
    bend_range;
    is_bend_range;
}

default! {
    /// The default ADSR attack
    const ATTACK: DynamicValue = DynamicValue::Static(0.05);
    attack;
    is_attack;
}

default! {
    /// The default ADSR decay
    const DECAY: DynamicValue = DynamicValue::Static(0.05);
    decay;
    is_decay;
}

default! {
    /// The default ADSR sustain
    const SUSTAIN: DynamicValue = DynamicValue::Static(0.7);
    sustain;
    is_sustain;
}

default! {
    /// The default ADSR release
    const RELEASE: DynamicValue = DynamicValue::Static(0.1);
    release;
    is_release;
}

default! {
    /// The default balance volume
    const VOLUME: DynamicValue = DynamicValue::Static(1.0);
    volume;
    is_volume;
}

default! {
    /// The default balance pan
    const PAN: DynamicValue = DynamicValue::Static(0.0);
    pan;
    is_pan;
}

default! {
    /// The default filter type
    const FILTER_TYPE: FilterType = FilterType::LowPass;
    filter_type;
    is_filter_type;
}
