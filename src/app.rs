use std::{convert::Infallible, fmt, path::PathBuf, str::FromStr};

use serde_derive::{Deserialize, Serialize};
use structopt::StructOpt;

use crate::{InstrId, SampleType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DynInput {
    Id(InstrId),
    Num(SampleType),
}

impl fmt::Display for DynInput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DynInput::Id(id) => write!(f, "{}", id),
            DynInput::Num(n) => write!(f, "{}", n),
        }
    }
}

impl FromStr for DynInput {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(s.parse::<SampleType>()
            .map(DynInput::Num)
            .unwrap_or_else(|_| s.parse::<InstrId>().map(DynInput::Id).unwrap()))
    }
}

#[derive(Debug, StructOpt)]
pub enum RyvmCommand {
    #[structopt(about = "Quit ryvm", alias = "exit")]
    Quit,
    #[structopt(about = "Set the output instrument")]
    Output {
        #[structopt(index = 1)]
        name: InstrId,
    },
    #[structopt(about = "Set the project tempo")]
    Tempo {
        #[structopt(index = 1)]
        tempo: f32,
    },
    #[structopt(about = "Create a number source", alias = "num")]
    Number {
        #[structopt(index = 1)]
        name: InstrId,
        #[structopt(index = 2)]
        num: f32,
    },
    #[structopt(about = "Create a sine wave synthesizer")]
    Sine {
        #[structopt(index = 1)]
        name: InstrId,
        #[structopt(index = 2)]
        input: Option<InstrId>,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "Create a square wave synthesizer")]
    Square {
        #[structopt(index = 1)]
        name: InstrId,
        #[structopt(index = 2)]
        input: Option<InstrId>,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "Create a saw wave synthesizer")]
    Saw {
        #[structopt(index = 1)]
        name: InstrId,
        #[structopt(index = 2)]
        input: Option<InstrId>,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "Create a triangle wave synthesizer", alias = "tri")]
    Triangle {
        #[structopt(index = 1)]
        name: InstrId,
        #[structopt(index = 2)]
        input: Option<InstrId>,
        #[structopt(long, short)]
        voices: Option<u32>,
    },
    #[structopt(about = "Create a mixer")]
    Mixer {
        #[structopt(index = 1)]
        name: InstrId,
        #[structopt(index = 2)]
        inputs: Vec<InstrId>,
    },
    #[cfg(feature = "keyboard")]
    #[structopt(about = "Use you computer kyeboard as a music keyboard")]
    Keyboard {
        #[structopt(index = 1)]
        name: InstrId,
        #[structopt(long, short)]
        octave: Option<u8>,
    },
    #[structopt(about = "Create a drum machine")]
    Drums {
        #[structopt(index = 1)]
        name: InstrId,
    },
    #[structopt(about = "Modify a drum machine")]
    Drum {
        #[structopt(
            index = 1,
            help = "The id of the drum machine. Defaults to last created/used."
        )]
        machine_id: Option<InstrId>,
        #[structopt(
            index = 2,
            help = "The index of the drum to be edited. Defaults to next highest."
        )]
        index: Option<usize>,
        #[structopt(long, short, help = "Path to the sound file")]
        path: Option<PathBuf>,
        #[structopt(
            long,
            short,
            allow_hyphen_values = true,
            help = "A string repesentation of the beat"
        )]
        beat: Option<String>,
        #[structopt(
            long,
            short = "x",
            help = "Repeat the entered beat some number of times",
            requires = "beat"
        )]
        repeat: Option<u8>,
        #[structopt(long, short, help = "Remove the specified drum", conflicts_with_all = &["path", "beat"])]
        remove: bool,
    },
    #[structopt(about = "Create a loop")]
    Loop {
        #[structopt(index = 1, help = "The instrument to be looped")]
        input: InstrId,
        #[structopt(
            index = 2,
            help = "How many measures each loop will last before looping"
        )]
        measures: u8,
    },
    #[structopt(about = "Start (a) loop(s)")]
    Start {
        #[structopt(index = 1, required = true, help = "The loops to start")]
        inputs: Vec<InstrId>,
    },
    #[structopt(about = "Stop (a) loop(s)")]
    Stop {
        #[structopt(index = 1, required = true, help = "The loops to stop")]
        inputs: Vec<InstrId>,
    },
    #[structopt(about = "Create a low-pass filter")]
    Filter {
        #[structopt(index = 1, help = "The signal being filtered")]
        input: InstrId,
        #[structopt(index = 2, help = "Defines filter shape")]
        value: DynInput,
    },
    #[structopt(about = "List all instruments")]
    Ls {
        #[structopt(long, short, help = "Do not sort list")]
        unsorted: bool,
    },
    #[structopt(about = "Print a tree of all output instruments")]
    Tree,
    #[structopt(
        about = "Choose which keyboard instrument to be controlled by the actual keyboard"
    )]
    Focus {
        #[structopt(index = 1, help = "The id of the keyboard instrument")]
        id: InstrId,
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
    #[structopt(index = 1)]
    pub value: DynInput,
}

#[cfg(feature = "keyboard")]
#[derive(Debug, StructOpt)]
pub struct KeyboardCommand {
    #[structopt(long, short)]
    pub octave: Option<u8>,
}
