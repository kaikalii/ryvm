macro_rules! mods {
    ($($m:ident),*) => ($(mod $m; pub use $m::*;)*);
}

mods!(app, instrument, track);

#[cfg(feature = "keyboard")]
mod keyboard;
#[cfg(feature = "keyboard")]
pub use keyboard::*;

use std::{
    io::stdin,
    iter::{once, repeat},
    sync::mpsc,
    thread,
    time::Duration,
};

use structopt::StructOpt;
use unicode_reader::CodePoints;

fn main() {
    let device = rodio::default_output_device().unwrap();
    let sink = rodio::Sink::new(&device);

    let instruments = Instruments::new();

    sink.append(instruments.clone());

    // Init stdin thread
    let stdin = stdin_recv();

    // Main loop
    loop {
        // Read commands
        if let Ok(text) = stdin.try_recv() {
            let args = once("ryvm").chain(text.split_whitespace());
            match RyvmApp::from_iter_safe(args) {
                Ok(app) => match app {
                    RyvmApp::Quit => break,
                    RyvmApp::Output { name } => {
                        instruments.update(|instrs| instrs.set_output(name))
                    }
                    RyvmApp::Tempo { tempo } => {
                        instruments.update(|instrs| instrs.set_tempo(tempo))
                    }
                    RyvmApp::Add { name, app } => {
                        instruments.update(|instrs| {
                            instrs.add(
                                name,
                                match app {
                                    AddApp::Number { num } => Instrument::Number(num),
                                    AddApp::Sine { input } => Instrument::sine(input),
                                    AddApp::Square { input } => Instrument::square(input),
                                    AddApp::Mixer { inputs } => Instrument::Mixer(
                                        inputs
                                            .into_iter()
                                            .zip(repeat(Balance::default()))
                                            .collect(),
                                    ),
                                },
                            )
                        });
                    }
                    RyvmApp::Edit {
                        name,
                        set,
                        inputs,
                        volume,
                        pan,
                    } => {
                        instruments.update(|instrs| {
                            if let Some(instr) = instrs.get_mut(name) {
                                match instr {
                                    Instrument::Number(n) => {
                                        if let Some(num) = set {
                                            *n = num;
                                        }
                                    }
                                    Instrument::Sine { input, .. }
                                    | Instrument::Square { input, .. } => {
                                        if let Some(new_input) = inputs.into_iter().next() {
                                            *input = new_input;
                                        }
                                    }
                                    Instrument::Mixer(map) => {
                                        if let Some(volume) = volume {
                                            for id in &inputs {
                                                map.entry(id.clone())
                                                    .or_insert_with(Balance::default)
                                                    .volume = volume;
                                            }
                                        }
                                        if let Some(pan) = pan {
                                            for id in &inputs {
                                                map.entry(id.clone())
                                                    .or_insert_with(Balance::default)
                                                    .pan = pan;
                                            }
                                        }
                                        for input in inputs {
                                            map.entry(input).or_insert_with(Balance::default);
                                        }
                                    }
                                }
                            }
                        });
                    }
                },
                Err(e) => println!("{}", e),
            }
            instruments.update(|instrs| println!("{:#?}", instrs));
        }
        // Sleep
        thread::sleep(Duration::from_millis(100));
    }
}

fn stdin_recv() -> mpsc::Receiver<String> {
    let (send, recv) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = String::new();
        let stdin = CodePoints::from(stdin());
        for c in stdin.filter_map(Result::ok) {
            if c == '\n' {
                let _ = send.send(buffer.trim().into());
                buffer.clear();
            } else {
                buffer.push(c);
            }
        }
    });
    recv
}
