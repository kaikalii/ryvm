use std::{
    io::{stdin, BufRead},
    process::exit,
};

use structopt::StructOpt;

use ryvm::{RyvmApp, State};

fn main() {
    let app = RyvmApp::from_iter_safe(std::env::args()).unwrap_or_default();

    let (state, interface) = match State::new(app.file, app.sample_rate) {
        Ok(state) => state,
        Err(e) => {
            println!("{}", e);
            exit(1);
        }
    };

    let device = match rodio::default_output_device() {
        Some(device) => device,
        None => {
            println!("Unable to get default audio output device");
            exit(1);
        }
    };

    let sink = match std::panic::catch_unwind(|| rodio::Sink::new(&device)) {
        Ok(sink) => sink,
        Err(_) => {
            println!("Unable to initialize audio device");
            exit(1);
        }
    };

    sink.append(state);

    // Main loop
    for line in stdin().lock().lines().filter_map(Result::ok) {
        match interface.send_command(line) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => println!("{}", e),
        }
    }
}
