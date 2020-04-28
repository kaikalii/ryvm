use std::{convert::Infallible, fmt, path::PathBuf, str::FromStr};

use structopt::StructOpt;

use crate::WaveForm;

/// An input type that can either be a static number or the
/// id of an instrument from which to get a number
#[derive(Debug, Clone)]
pub enum DynInput {
    Id(String),
    Num(f32),
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
        Ok(s.parse::<f32>()
            .map(DynInput::Num)
            .unwrap_or_else(|_| s.parse::<String>().map(DynInput::Id).unwrap()))
    }
}

/// A Ryvm CLI command
#[derive(Debug, StructOpt)]
pub enum RyvmCommand {
    #[structopt(about = "Quit ryvm", alias = "exit")]
    Quit,
    #[structopt(about = "Set the project's relative tempo")]
    Tempo {
        #[structopt(index = 1, help = "The new value for the relative tempo")]
        tempo: f32,
    },
    #[structopt(about = "Create a wave synthesizer")]
    Wave {
        #[structopt(index = 1, help = "The waveform to use")]
        waveform: WaveForm,
        #[structopt(index = 2, help = "The name of the synthesizer")]
        name: String,
        #[structopt(
            long,
            short,
            allow_hyphen_values = true,
            help = "The synth's octave relative to its input"
        )]
        octave: Option<i8>,
        #[structopt(long, short, help = "The synth's attack")]
        attack: Option<f32>,
        #[structopt(long, short, help = "The synth's decay")]
        decay: Option<f32>,
        #[structopt(long, short, help = "The synth's sustain")]
        sustain: Option<f32>,
        #[structopt(long, short, help = "The synth's release")]
        release: Option<f32>,
    },
    #[structopt(about = "Create a drum machine")]
    Drums {
        #[structopt(index = 1, help = "The name of the drum machine")]
        name: String,
    },
    #[structopt(about = "Modify a drum machine")]
    Drum {
        #[structopt(
            index = 1,
            help = "The id of the drum machine. Defaults to last created/used."
        )]
        machine_id: Option<String>,
        #[structopt(
            index = 2,
            help = "The index of the drum to be edited. Defaults to next highest."
        )]
        index: Option<usize>,
        #[structopt(long, short, help = "Path to the sound file")]
        path: Option<PathBuf>,
        #[structopt(long, short, help = "Remove the specified drum", conflicts_with_all = &["path", "beat"])]
        remove: bool,
    },
    #[structopt(about = "Create a low-pass filter")]
    Filter {
        #[structopt(index = 1, help = "The signal being filtered")]
        input: String,
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
    #[structopt(about = "Start a new script")]
    Script {
        #[structopt(index = 1, help = "The name of the script")]
        name: String,
        #[structopt(index = 2, help = "The arguments of the script")]
        args: Vec<String>,
    },
    #[structopt(about = "End a script")]
    End,
    #[structopt(about = "Remove an instrument", alias = "remove")]
    Rm {
        #[structopt(index = 1, help = "The id of the instrument to be removed")]
        id: String,
        #[structopt(
            long,
            short,
            help = "Recursively remove all the instrument's unique inputs as well"
        )]
        recursive: bool,
    },
    #[structopt(about = "Load a script")]
    Load {
        #[structopt(index = 1, help = "The name of the script to load")]
        name: String,
    },
    #[structopt(about = "Run a script, loading it first if necessary")]
    Run {
        #[structopt(index = 1, help = "The name of the script to run")]
        name: String,
        #[structopt(index = 2, help = "The arguments to pass to the script")]
        args: Vec<String>,
    },
}

#[derive(Debug, StructOpt)]
pub struct WaveCommand {
    #[structopt(long, short, help = "Set the synth's octave relative to its input")]
    pub octave: Option<i8>,
    #[structopt(long, short, help = "Set the synth's attack")]
    pub attack: Option<f32>,
    #[structopt(long, short, help = "Set the synth's decay")]
    pub decay: Option<f32>,
    #[structopt(long, short, help = "Set the synth's sustain")]
    pub sustain: Option<f32>,
    #[structopt(long, short, help = "Set the synth's release")]
    pub release: Option<f32>,
    #[structopt(long, short, help = "Set the synth's waveform")]
    pub form: Option<WaveForm>,
}

#[derive(Debug, StructOpt)]
pub struct FilterCommand {
    #[structopt(index = 1, help = "Defines filter shape")]
    pub value: DynInput,
}

#[derive(Debug, Default, StructOpt)]
pub struct RyvmApp {
    #[structopt(short = "r", long, about = "List the available midi ports")]
    pub sample_rate: Option<u32>,
}
