macro_rules! mods {
    ($($m:ident),*) => ($(mod $m; pub use $m::*;)*);
}

mods!(app, instrument);

use rodio::Sink;

fn main() {
    let device = rodio::default_output_device().unwrap();
    let sink = Sink::new(&device);

    let instruments = Instruments::new();
    instruments.update(|instruments| {
        instruments.add("wave1", Instrument::square(440.0));
        instruments.add("wave2", Instrument::square(554.0));
        instruments.add("wave3", Instrument::sine(554.0));
        instruments.add(
            "mixer",
            Instrument::Mixer(vec![
                Balanced::from("wave1".to_string()).pan(-1.0),
                Balanced::from("wave2".to_string()).pan(1.0),
                Balanced::from("wave3".to_string()).pan(0.0),
            ]),
        );
        instruments.set_output("mixer");
    });

    sink.append(instruments);

    // Main loop
    loop {

        // Update
    }
}
