macro_rules! mods {
    ($($vis:vis $m:ident),*) => ($(mod $m; $vis use $m::*;)*);
}

mod utility;
use utility::*;

mods!(
    app, channel, envelope, error, gamepad, input, library, r#loop, midi, node, onfly, sample,
    spec, state, track
);

use std::io::{stdin, stdout, BufRead, Write};

use colored::Colorize;
use rodio::DeviceTrait;
use structopt::StructOpt;

type Frame = u64;

fn main() {
    if let Err(e) = run() {
        colorprintln!("{}", bright_red, e);
        std::process::exit(1);
    }
}

fn run() -> RyvmResult<()> {
    let app = RyvmApp::from_args();

    // Supress stderr
    let shh = if app.nosuppress {
        None
    } else {
        Some(shh::stderr())
    };

    // Check subcommand
    match app.sub {
        Some(RyvmSubcommand::OutputList) => {
            list_output_devices()?;
            return Ok(());
        }
        Some(RyvmSubcommand::InputList) => {
            list_input_devices()?;
            return Ok(());
        }
        None => {}
    }

    print!("Initializing...\r");
    stdout().flush().unwrap();

    // Initialize device
    let device = if let Some(output) = app.output {
        if let Some(device) = rodio::output_devices()
            .map_err(InputError::from)?
            .find(|dev| {
                dev.name()
                    .expect("Error getting device name")
                    .contains(&output)
            })
        {
            device
        } else {
            return Err(RyvmError::NoMatchingNode(output));
        }
    } else {
        rodio::default_output_device().ok_or(RyvmError::NoDefaultOutputNode)?
    };

    // Create the audio sync
    let sink = std::panic::catch_unwind(|| rodio::Sink::new(&device))
        .map_err(|_| RyvmError::UnableToInitializeNode)?;

    colorprintln!(
        "Using audio output device {:?}",
        bright_cyan,
        device.name().expect("Error getting device name")
    );

    // Initialize state
    let (state, interface) = State::new(app.file, app.sample_rate)?;

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

    Ok(())
}
