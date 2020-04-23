use std::io::{stdin, BufRead};

use ryvm::Ryvm;

fn main() {
    let device = rodio::default_output_device().unwrap();

    let ryvm = Ryvm::new(device);

    // Command loop
    for line in stdin().lock().lines().filter_map(Result::ok) {
        ryvm.send_command(&line);
        match line.trim() {
            "exit" | "quit" => break,
            _ => {}
        }
    }
}
