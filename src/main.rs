mod utility;

mod app;
mod channel;
mod envelope;
mod error;
mod gamepad;
mod input;
mod library;
mod r#loop;
mod midi;
mod node;
mod onfly;
mod sample;
mod spec;
mod state;
mod track;

mod ty {
    pub use crate::{
        channel::Voice,
        midi::{Control, Port},
        spec::Name,
        track::Letter,
        utility::Float,
        Frame,
    };
}

use std::io::{stdin, stdout, BufRead, Write};

use colored::Colorize;
use rodio::DeviceTrait;
use structopt::StructOpt;

use error::{RyvmError as Error, RyvmResult as Result};

pub type Frame = u64;

fn main() {
    if let Err(e) = run() {
        colorprintln!("{}", bright_red, e);
        std::process::exit(1);
    }
}

fn run() -> crate::Result<()> {
    let app = app::RyvmApp::from_args();

    // Supress stderr
    let shh = if app.nosuppress {
        None
    } else {
        Some(shh::stderr())
    };

    // Check subcommand
    match app.sub {
        Some(app::RyvmSubcommand::OutputList) => {
            utility::list_output_devices()?;
            return Ok(());
        }
        Some(app::RyvmSubcommand::InputList) => {
            utility::list_input_devices()?;
            return Ok(());
        }
        None => {}
    }

    print!("Initializing...\r");
    stdout().flush().unwrap();

    // Initialize device
    let device = if let Some(output) = app.output {
        if let Some(device) = rodio::output_devices()
            .map_err(input::InputError::from)?
            .find(|dev| {
                dev.name()
                    .expect("Error getting device name")
                    .contains(&output)
            })
        {
            device
        } else {
            return Err(crate::Error::NoMatchingNode(output));
        }
    } else {
        rodio::default_output_device().ok_or(crate::Error::NoDefaultOutputNode)?
    };

    // Create the audio sync
    let sink = std::panic::catch_unwind(|| rodio::Sink::new(&device))
        .map_err(|_| crate::Error::UnableToInitializeNode)?;

    colorprintln!(
        "Using audio output device {:?}",
        bright_cyan,
        device.name().expect("Error getting device name")
    );

    // Initialize state
    let (state, interface) = state::State::new(app.file, app.sample_rate)?;

    sink.append(state);

    // Main loop
    for line in stdin().lock().lines().filter_map(std::result::Result::ok) {
        match interface.send_command(line) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => println!("{}", e.to_string().bright_red()),
        }
    }

    drop(shh);

    Ok(())
}
