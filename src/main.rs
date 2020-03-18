macro_rules! mods {
    ($($m:ident),*) => ($(mod $m; use $m::*;)*);
}

mods!(control);

use piston_window::*;

fn main() {
    let mut con = Controller::default();
    let mut window: PistonWindow = WindowSettings::new("Album", con.window_size)
        .build()
        .unwrap();
    // Main loop
    while let Some(event) = window.next() {
        // Draw
        window.draw_2d(&event, |_context, graphics, _device| {
            clear([0.0, 0.0, 0.0, 1.0], graphics);
        });
        // Process events
        if let Event::Input(input, _) = event {
            match input {
                Input::Move(Motion::MouseCursor(pos)) => con.mouse_pos = pos,
                Input::Resize(args) => con.window_size = args.window_size,
                _ => {}
            }
        }
    }
}
