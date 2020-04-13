macro_rules! mods {
    ($($m:ident),*) => ($(mod $m; pub use $m::*;)*);
}

mods!(app, instrument);

use std::{io::stdin, iter, sync::mpsc, thread};

use unicode_reader::CodePoints;

fn main() {
    // let device = rodio::default_output_device().unwrap();
    // let sink = rodio::Sink::new(&device);
    //
    // let instruments = Instruments::new();
    // instruments.update(|instruments| {
    //     instruments.add("wave1", Instrument::square(440.0));
    //     instruments.add("wave2", Instrument::square(554.0));
    //     instruments.add("wave3", Instrument::sine(554.0));
    //     instruments.add(
    //         "mixer",
    //         Instrument::Mixer(vec![
    //             Balanced::from("wave1".to_string()).pan(-1.0),
    //             Balanced::from("wave2".to_string()).pan(1.0),
    //             Balanced::from("wave3".to_string()).pan(0.0),
    //         ]),
    //     );
    //     instruments.set_output("mixer");
    // });
    //
    // sink.append(instruments);

    // Init stdin thread
    let stdin = stdin_recv();
    // Init command app
    let mut app = app();

    // Main loop
    loop {
        // Read commands
        if let Ok(text) = stdin.try_recv() {
            let args = iter::once("ryvm").chain(text.split_whitespace());
            match app.get_matches_from_safe_borrow(args) {
                Ok(matches) => {
                    if matches.subcommand_matches("quit").is_some() {
                        break;
                    }
                }
                Err(e) => println!("{}", e),
            }
        }
        // Update
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
