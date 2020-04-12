macro_rules! mods {
    ($($m:ident),*) => ($(mod $m; pub use $m::*;)*);
}

mods!(app, instrument);

use rodio::Sink;

fn main() {
    let device = rodio::default_output_device().unwrap();
    let sink = Sink::new(&device);

    let (source, mixer) = Instrument::Mixer(vec![
        Instrument::square(440.0).balanced().pan(-1.0),
        Instrument::square(554.0).balanced().pan(1.0),
        Instrument::sine(554.0).balanced().pan(0.0),
    ])
    .source();
    sink.append(source);

    // Main loop
    loop {

        // Update
    }
}
