#![deny(missing_docs)]

/*!
Ryvm is an interface into a digital audio workstation (DAW). You can use Ryvm as a library or as a command-line app.
*/

macro_rules! mods {
    ($($m:ident),*) => ($(mod $m; use $m::*;)*);
}

mods!(app, channel, device, drum, envelope, midi, parts, script, state, track, utility);

use std::{iter::once, sync::mpsc, thread, time::Duration};

use structopt::StructOpt;

pub use rodio::{default_output_device, output_devices};

/// A Ryvm context
pub struct Ryvm {
    send: mpsc::Sender<String>,
}

impl Ryvm {
    /// Create a new Ryvm context
    pub fn new(device: rodio::Device) -> Self {
        let (send, recv) = mpsc::channel::<String>();

        thread::spawn(move || {
            let sink = match std::panic::catch_unwind(|| rodio::Sink::new(&device)) {
                Ok(sink) => sink,
                Err(_) => {
                    println!("Unable to initialize audio device");
                    std::process::exit(1);
                }
            };

            let state = State::new();

            sink.append(state.clone());

            // Main loop
            'main_loop: loop {
                // Read commands
                if let Ok(text) = recv.try_recv() {
                    if text.trim().is_empty() {
                        state.update(State::stop_recording);
                        continue;
                    }
                    for (delay, args) in text.split(',').map(|text| {
                        let (delay, parsed) = parse_args(text.trim());
                        (delay, once("ryvm".into()).chain(parsed).collect::<Vec<_>>())
                    }) {
                        let app = RyvmCommand::from_iter_safe(&args);
                        if let Ok(RyvmCommand::Quit) = &app {
                            break 'main_loop;
                        }
                        state.update(|state| state.queue_command(delay, args, app));
                    }
                }
                // Sleep
                thread::sleep(Duration::from_millis(100));
            }
        });
        Ryvm { send }
    }
    /// Send a command to the Ryvm context
    pub fn send_command<S>(&self, command: S)
    where
        S: Into<String>,
    {
        let _ = self.send.send(command.into());
    }
}

impl Drop for Ryvm {
    fn drop(&mut self) {
        self.send_command("exit");
    }
}
