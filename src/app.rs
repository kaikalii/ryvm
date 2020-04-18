use std::path::PathBuf;

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct RyvmApp {
    #[structopt(index = 1)]
    pub name: Option<String>,
    #[structopt(index = 2)]
    pub inputs: Vec<String>,
    #[structopt(long, short)]
    pub remove: Vec<String>,
    #[structopt(long, short)]
    pub volume: Option<f32>,
    #[structopt(long, short)]
    pub pan: Option<f32>,
    #[structopt(subcommand)]
    pub command: Option<RyvmCommand>,
}

#[derive(Debug, StructOpt)]
pub enum RyvmCommand {
    #[structopt(about = "Quit ryvm", alias = "exit")]
    Quit,
    Output {
        #[structopt(index = 1)]
        name: String,
    },
    Tempo {
        #[structopt(index = 1)]
        tempo: f32,
    },
    #[structopt(about = "A number", alias = "num")]
    Number { num: f32 },
    #[structopt(about = "A sine wave synthesizer")]
    Sine {
        input: String,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "A square wave synthesizer")]
    Square {
        input: String,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "A saw wave synthesizer")]
    Saw {
        input: String,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "A triangle wave synthesizer", alias = "tri")]
    Triangle {
        input: String,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "A mixer")]
    Mixer { inputs: Vec<String> },
    #[cfg(feature = "keyboard")]
    #[structopt(about = "Use you computer kyeboard as a music keyboard")]
    Keyboard {
        #[structopt(long, short = "o")]
        base_octave: Option<u8>,
    },
    #[structopt(about = "A drum machine")]
    Drums,
    Drum {
        #[structopt(index = 1)]
        index: Option<usize>,
        #[structopt(long, short)]
        path: Option<PathBuf>,
        #[structopt(long, short, allow_hyphen_values = true)]
        beat: Option<String>,
        #[structopt(long, short, conflicts_with_all = &["path", "beat"])]
        remove: bool,
    },
    Loop {
        #[structopt(index = 1)]
        input: String,
        #[structopt(index = 2)]
        measures: u8,
    },
}
