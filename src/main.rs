macro_rules! mods {
    ($($m:ident),*) => ($(mod $m; pub use $m::*;)*);
}

mods!(instrument);

use rodio::Sink;

fn main() {
    let device = rodio::default_output_device().unwrap();
    let sink = Sink::new(&device);

    let (source, mixer) = Instrument::Mixer(vec![
        Balanced::from(Instrument::square(440.0)).pan(-1.0),
        Balanced::from(Instrument::square(554.0)).pan(1.0),
        Balanced::from(Instrument::sine(554.0)).pan(0.0),
    ])
    .source();
    sink.append(source);

    // Main loop
    loop {

        // Update
    }
}
