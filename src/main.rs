macro_rules! mods {
    ($($m:ident),*) => ($(mod $m; pub use $m::*;)*);
}

mods!(app, channel, drum, instrument, instruments, track, utility);

#[cfg(feature = "keyboard")]
mod keyboard;
#[cfg(feature = "keyboard")]
pub use keyboard::*;

use std::{io::stdin, iter::once, sync::mpsc, thread, time::Duration};

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
            let args: Vec<String> = once("ryvm".into()).chain(parse_args(&text)).collect();
            let app = RyvmApp::from_iter_safe(&args);
            if let Ok(RyvmApp {
                command: Some(RyvmCommand::Quit),
                ..
            }) = &app
            {
                break;
            }
            instruments.update(|instrs| instrs.queue_command(args, app));
            // instruments.update(|instrs| println!("{:#?}", instrs));
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
