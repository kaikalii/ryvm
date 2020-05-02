use std::{
    io::{stdin, BufRead},
    process::exit,
    sync::mpsc,
    thread,
    time::Duration,
};

use ryvm::State;

fn main() {
    let state = match State::new() {
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

    sink.append(state.clone());

    // Spawn command entry thread
    let (send, recv) = mpsc::channel();
    thread::spawn(move || {
        for line in stdin().lock().lines().filter_map(Result::ok) {
            let _ = send.send(line);
        }
    });

    // Main loop
    loop {
        // Read commands
        if let Ok(text) = recv.try_recv() {
            match state.update(|state| state.queue_command(&text)) {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => println!("{}", e),
            }
        }
        // Sleep
        thread::sleep(Duration::from_millis(100));
    }
}
