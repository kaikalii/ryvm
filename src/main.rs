macro_rules! mods {
    ($($vis:vis $m:ident),*) => ($(mod $m; $vis use $m::*;)*);
}

mod utility;
use utility::*;

mods!(
    app, channel, device, envelope, error, gamepad, input, library, r#loop, midi, onfly, sample,
    spec, state, track
);

use std::{
    io::{stdin, stdout, BufRead, Write},
    process::exit,
};

use colored::Colorize;
use rodio::DeviceTrait;
use structopt::StructOpt;

type Frame = u64;

fn main() {
    // Supress stderr
    let shh = shh::stderr();

    let app = RyvmApp::from_args();

    #[allow(clippy::single_match)]
    match app.sub {
        Some(RyvmSubcommand::OutputList) => {
            if let Err(e) = list_output_devices() {
                println!("{}", e.to_string().bright_red());
            }
            return;
        }
        None => {}
    }

    print!("Initializing...\r");
    stdout().flush().unwrap();

    let device = if let Some(output) = app.output {
        match rodio::output_devices() {
            Ok(mut devices) => {
                if let Some(device) = devices.find(|dev| {
                    dev.name()
                        .expect("Error getting device name")
                        .contains(&output)
                }) {
                    device
                } else {
                    println!(
                        "{}",
                        format!("No available audio output device matching {:?}", output)
                            .bright_red()
                    );
                    exit(1);
                }
            }
            Err(e) => {
                println!(
                    "{}",
                    format!("Error checkout output devices: {}", e).bright_red()
                );
                exit(1);
            }
        }
    } else {
        match rodio::default_output_device() {
            Some(device) => device,
            None => {
                println!(
                    "{}",
                    "Unable to get default audio output device".bright_red()
                );
                exit(1);
            }
        }
    };

    let sink = match std::panic::catch_unwind(|| rodio::Sink::new(&device)) {
        Ok(sink) => sink,
        Err(_) => {
            println!("{}", "Unable to initialize audio device".bright_red());
            exit(1);
        }
    };

    println!(
        "{}",
        format!(
            "Using audio output device {:?}",
            device.name().expect("Error getting device name")
        )
        .bright_cyan()
    );

    let (state, interface) = match State::new(app.file, app.sample_rate) {
        Ok(state) => state,
        Err(e) => {
            println!("{}", e);
            exit(1);
        }
    };

    sink.append(state);

    // Main loop
    for line in stdin().lock().lines().filter_map(Result::ok) {
        match interface.send_command(line) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => println!("{}", e.to_string().bright_red()),
        }
    }

    drop(shh);
}
