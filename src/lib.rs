#![warn(missing_docs)]

/*!
Ryvm is an interface into a digital audio workstation (DAW). You can use Ryvm as a library or as a command-line app.
*/

macro_rules! mods {
    ($($vis:vis $m:ident),*) => ($(mod $m; $vis use $m::*;)*);
}

mods!(pub app, pub channel, pub device, drum, envelope, pub error, gamepad, r#loop, midi, onfly, pub state, track, utility);

pub use rodio::{default_output_device, output_devices};

type Frame = u64;
