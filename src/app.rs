use std::{fmt, str::FromStr};

use structopt::StructOpt;

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

pub type OrString<N> = DynInput<N, String>;

/// An input specifying a control input on a controller
#[derive(Debug, Clone)]
pub struct ControlId {
    /// Either the port number or assigned name of a controller.
    ///
    /// These names are resolves by the `State`
    pub controller: Option<OrString<usize>>,
    /// Either the number of a control or its assigned name
    ///
    /// These names are resolved by the controller
    pub control: OrString<u8>,
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
            (Some(a), Some(b)) => (
                Some(a.parse::<OrString<usize>>()?),
                b.parse::<OrString<u8>>()?,
            ),
            (Some(a), None) => (None, a.parse::<OrString<u8>>()?),
            (None, Some(b)) => (None, b.parse::<OrString<u8>>()?),
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
    #[structopt(about = "Start recording a loop. Press enter to finish recording.")]
    Loop {
        #[structopt(help = "The name of the loop")]
        name: Option<String>,
        #[structopt(
            long,
            short = "x",
            help = "The length of the loop relative to the first one"
        )]
        length: Option<f32>,
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
    Midi(MidiSubcommand),
    #[structopt(about = "Set the current channel for manual-controlled devices \
    (Simply typing the number without this command will have the same effect)")]
    Ch {
        #[structopt(help = "The channel to set")]
        channel: u8,
    },
    #[structopt(about = "Load a spec file")]
    Load {
        #[structopt(help = "The name of the spec")]
        name: String,
        #[structopt(help = "The channel to load into")]
        channel: Option<u8>,
    },
}

#[derive(Debug, StructOpt)]
pub enum MidiSubcommand {
    #[structopt(about = "List the available midi ports")]
    List,
    #[structopt(about = "Initialize a new midi device")]
    Init {
        #[structopt(help = "The midi port to use. Defaults to the first avaiable non-thru port")]
        port: Option<usize>,
        #[structopt(long, short, help = "The name of the midi device")]
        name: Option<String>,
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
