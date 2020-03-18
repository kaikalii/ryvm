macro_rules! mods {
    ($($m:ident),*) => ($(mod $m; pub use $m::*;)*);
}

mods!(control, instrument);

use piston_window::*;
use rodio::Sink;

fn main() {
    let mut con = Controller::default();
    let mut window: PistonWindow = WindowSettings::new("Album", con.window_size)
        .exit_on_esc(true)
        .build()
        .unwrap();

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
    while let Some(event) = window.next() {
        // Draw
        window.draw_2d(&event, |_context, graphics, _device| {
            clear([0.0, 0.0, 0.0, 1.0], graphics);
        });
        // Process events
        if let Event::Input(input, _) = event {
            match input {
                Input::Move(Motion::MouseCursor(pos)) => {
                    con.mouse_pos = pos;
                    const POW_DELTA: SampleType = 0.011;
                    mixer.update(|mixer| {
                        mixer[0]
                            .instr
                            .set_freq((pos[1] as f32).powf(1.0 + POW_DELTA * 0.0) + 1.0);
                        mixer[1]
                            .instr
                            .set_freq((pos[1] as f32).powf(1.0 + POW_DELTA * 4.0) + 1.0);
                        mixer[2]
                            .instr
                            .set_freq((pos[1] as f32).powf(1.0 + POW_DELTA * 3.0) + 1.0);
                    });
                }
                Input::Resize(args) => con.window_size = args.window_size,
                _ => {}
            }
        }
        // Update
    }
}
