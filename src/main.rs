macro_rules! mods {
    ($($m:ident),*) => ($(mod $m; pub use $m::*;)*);
}

mods!(app, instrument);

use std::{io::stdin, iter, sync::mpsc, thread, time::Duration};

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
            let args = iter::once("ryvm").chain(text.split_whitespace());
            match RyvmApp::from_iter_safe(args) {
                Ok(app) => match app {
                    RyvmApp::Quit => break,
                    RyvmApp::Output { name } => {
                        instruments.update(|instrs| instrs.set_output(name))
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
                                        inputs.into_iter().map(Balanced::from).collect(),
                                    ),
                                },
                            )
                        });
                    }
                },
                Err(e) => println!("{}", e),
            }
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
