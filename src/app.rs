use std::{fmt, path::PathBuf, str::FromStr};

use structopt::StructOpt;

use crate::WaveForm;

/// An input type that first tries one type of input,
/// then the other
#[derive(Debug, Clone)]
pub enum DynInput<A, B> {
    First(A),
    Second(B),
}

impl<A, B> fmt::Display for DynInput<A, B>
where
    A: fmt::Display,
    B: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DynInput::First(a) => write!(f, "{}", a),
            DynInput::Second(b) => write!(f, "{}", b),
        }
    }
}

impl<A, B> FromStr for DynInput<A, B>
where
    A: FromStr,
    B: FromStr,
{
    type Err = B::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse::<A>() {
            Ok(a) => Ok(DynInput::First(a)),
            Err(_) => match s.parse::<B>() {
                Ok(b) => Ok(DynInput::Second(b)),
                Err(e) => Err(e),
            },
        }
    }
}

pub type U8OrString = DynInput<u8, String>;

#[derive(Debug, Clone)]
pub struct ControlId {
    pub controller: Option<U8OrString>,
    pub control: U8OrString,
}

impl fmt::Display for ControlId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.controller {
            Some(DynInput::First(port)) => write!(f, "{}-", port)?,
            Some(DynInput::Second(name)) => write!(f, "{}-", name)?,
            None => {}
        }
        match &self.control {
            DynInput::First(control) => write!(f, "{}", control),
            DynInput::Second(name) => write!(f, "{}", name),
        }
    }
}

impl FromStr for ControlId {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('-');
        let first = parts.next().filter(|s| !s.is_empty());
        let second = parts.next().filter(|s| !s.is_empty());
        let (controller, control) = match (first, second) {
            (Some(a), Some(b)) => (Some(a.parse::<U8OrString>()?), b.parse::<U8OrString>()?),
            (Some(a), None) => (None, a.parse::<U8OrString>()?),
            (None, Some(b)) => (None, b.parse::<U8OrString>()?),
            (None, None) => (None, DynInput::Second(String::new())),
        };
        Ok(ControlId {
            controller,
            control,
        })
    }
}

/// A Ryvm CLI command
#[derive(Debug, StructOpt)]
pub enum RyvmCommand {
    #[structopt(about = "Quit ryvm", alias = "exit")]
    Quit,
    #[structopt(about = "Set the project's relative tempo")]
    Tempo {
        #[structopt(help = "The new value for the relative tempo")]
        tempo: f32,
    },
    #[structopt(about = "Create a wave synthesizer")]
    Wave {
        #[structopt(help = "The waveform to use")]
        waveform: WaveForm,
        #[structopt(help = "The name of the synthesizer")]
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
        #[structopt(long, short, help = "The synth's pitch bend range")]
        bend: Option<f32>,
    },

    #[structopt(about = "Create a drum machine")]
    Drums {
        #[structopt(help = "The name of the drum machine")]
        name: String,
    },
    #[structopt(about = "Modify a drum machine")]
    Drum {
        #[structopt(help = "The name of the drum machine. Defaults to last created/used.")]
        machine_id: Option<String>,
        #[structopt(help = "The index of the drum to be edited. Defaults to next highest.")]
        index: Option<usize>,
        #[structopt(long, short, help = "Path to the sound file")]
        path: Option<PathBuf>,
        #[structopt(long, short, help = "Remove the specified drum", conflicts_with_all = &["path", "beat"])]
        remove: bool,
    },
    #[structopt(about = "Start recording a loop. Press enter to finish recording.")]
    Loop {
        #[structopt(help = "The name of the device being looped")]
        input: String,
        #[structopt(help = "The name of the loop")]
        name: Option<String>,
        #[structopt(
            long,
            short = "x",
            help = "The length of the loop relative to the first one"
        )]
        length: Option<f32>,
    },
    #[structopt(about = "Create a low-pass filter")]
    Filter {
        #[structopt(help = "The signal being filtered")]
        input: String,
        #[structopt(help = "Defines filter shape")]
        value: DynInput<f32, String>,
    },
    #[structopt(about = "Start playing a loop")]
    Play {
        #[structopt(required = true, help = "The names of the loops to play")]
        names: Vec<String>,
    },
    #[structopt(about = "Stop playing a loop")]
    Stop {
        #[structopt(help = "The names of the loops to stop")]
        names: Vec<String>,
        #[structopt(long, conflicts_with = "names", help = "Stop all loops")]
        all: bool,
        #[structopt(
            long,
            short,
            conflicts_with = "names",
            help = "Stop all loops and delete them"
        )]
        reset: bool,
    },
    #[structopt(about = "List all devices")]
    Ls {
        #[structopt(long, short, help = "Do not sort list")]
        unsorted: bool,
    },
    #[structopt(about = "Print a tree of all output devices")]
    Tree,
    #[structopt(about = "Choose which keyboard device to be controlled by the actual keyboard")]
    #[structopt(about = "Start a new script")]
    Script {
        #[structopt(help = "The name of the script")]
        name: String,
        #[structopt(help = "The arguments of the script")]
        args: Vec<String>,
    },
    #[structopt(about = "End a script")]
    End,
    #[structopt(about = "Remove an device", alias = "remove")]
    Rm {
        #[structopt(help = "The name of the device to be removed")]
        id: String,
        #[structopt(
            long,
            short,
            help = "Recursively remove all the device's unique inputs as well"
        )]
        recursive: bool,
    },
    #[structopt(about = "Load a script")]
    Load {
        #[structopt(help = "The name of the script to load")]
        name: String,
    },
    #[structopt(about = "Run a script, loading it first if necessary")]
    Run {
        #[structopt(help = "The name of the script to run")]
        name: String,
        #[structopt(help = "The arguments to pass to the script")]
        args: Vec<String>,
    },
    Midi(MidiSubcommand),
    #[structopt(about = "Set the current channel for manual-controlled devices \
    (Simply typing the number without this command will have the same effect)")]
    Ch {
        #[structopt(help = "The channel to set")]
        channel: u8,
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
    #[structopt(long, short, help = "Set the synth's pitch bend range")]
    pub bend: Option<f32>,
}

#[derive(Debug, StructOpt)]
pub struct FilterCommand {
    #[structopt(help = "Defines filter shape")]
    pub value: DynInput<f32, String>,
}

#[derive(Debug, StructOpt)]
pub enum MidiSubcommand {
    #[structopt(about = "List the available midi ports")]
    List,
    #[structopt(about = "Initialize a new midi device")]
    Init {
        #[structopt(help = "The midi port to use. Defaults to the first avaiable non-thru port")]
        port: Option<usize>,
        #[structopt(
            long,
            short,
            help = "The midi channel for this device will be controlled manually via the \"ch\" command"
        )]
        manual: bool,
        #[structopt(
            long,
            short = "c",
            // requires = "pad_start",
            help = "The midi channel on which pad press/release messages are sent from the controller"
        )]
        pad_channel: Option<u8>,
        #[structopt(
            long,
            short = "s",
            // requires = "pad_channel",
            help = "\
            The index if the first note at which pad press/release messages are \
            sent from the controller (C4 = 60)"
        )]
        pad_start: Option<u8>,
    },
}

#[derive(Debug, Default, StructOpt)]
pub struct RyvmApp {
    #[structopt(short = "r", long, about = "List the available midi ports")]
    pub sample_rate: Option<u32>,
}
