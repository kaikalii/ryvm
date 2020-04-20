use std::path::PathBuf;

use structopt::StructOpt;

use crate::InstrId;

#[derive(Debug, StructOpt)]
pub struct RyvmApp {
    #[structopt(index = 1)]
    pub name: Option<InstrId>,
    #[structopt(subcommand)]
    pub command: Option<RyvmCommand>,
}

#[derive(Debug, StructOpt)]
pub enum RyvmCommand {
    #[structopt(about = "Quit ryvm", alias = "exit")]
    Quit,
    Output {
        #[structopt(index = 1)]
        name: InstrId,
    },
    Tempo {
        #[structopt(index = 1)]
        tempo: f32,
    },
    #[structopt(about = "A number", alias = "num")]
    Number { num: f32 },
    #[structopt(about = "A sine wave synthesizer")]
    Sine {
        input: InstrId,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "A square wave synthesizer")]
    Square {
        input: InstrId,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "A saw wave synthesizer")]
    Saw {
        input: InstrId,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "A triangle wave synthesizer", alias = "tri")]
    Triangle {
        input: InstrId,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "A mixer")]
    Mixer { inputs: Vec<InstrId> },
    #[cfg(feature = "keyboard")]
    #[structopt(about = "Use you computer kyeboard as a music keyboard")]
    Keyboard {
        #[structopt(long, short)]
        octave: Option<u8>,
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
        #[structopt(long, short = "x")]
        repeat: Option<u8>,
        #[structopt(long, short, conflicts_with_all = &["path", "beat"])]
        remove: bool,
    },
    Loop {
        #[structopt(index = 1)]
        input: InstrId,
        #[structopt(index = 2)]
        measures: u8,
    },
    Start {
        #[structopt(index = 1, required = true)]
        inputs: Vec<InstrId>,
    },
    Stop {
        #[structopt(index = 1, required = true)]
        inputs: Vec<InstrId>,
    },
    Filter {
        #[structopt(index = 1)]
        input: String,
        #[structopt(index = 2)]
        setting: f32,
    },
}

#[derive(Debug, StructOpt)]
pub struct NumberCommand {
    #[structopt(index = 1)]
    pub val: f32,
}

#[derive(Debug, StructOpt)]
pub struct WaveCommand {
    #[structopt(index = 1)]
    pub input: InstrId,
}

#[derive(Debug, StructOpt)]
pub struct MixerCommand {
    #[structopt(index = 1)]
    pub inputs: Vec<InstrId>,
    #[structopt(long, short)]
    pub volume: Option<f32>,
    #[structopt(long, short)]
    pub pan: Option<f32>,
    #[structopt(long, short)]
    pub remove: bool,
}

#[derive(Debug, StructOpt)]
pub struct FilterCommand {
    #[structopt(long, short)]
    pub input: Option<InstrId>,
    #[structopt(long, short)]
    pub setting: Option<f32>,
}

#[cfg(feature = "keyboard")]
#[derive(Debug, StructOpt)]
pub struct KeyboardCommand {
    #[structopt(index = 1)]
    pub octave: u8,
}
