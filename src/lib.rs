#![warn(missing_docs)]

/*!
Ryvm is an interface into a digital audio workstation (DAW). You can use Ryvm as a library or as a command-line app.
*/

macro_rules! mods {
    ($($vis:vis $m:ident),*) => ($(mod $m; $vis use $m::*;)*);
}

mods!(app, pub channel, pub device, drum, envelope, pub error, r#loop, midi, parts, pub state, track, utility);

pub use rodio::{default_output_device, output_devices};
