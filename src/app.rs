use std::{convert::Infallible, fmt, path::PathBuf, str::FromStr};

use structopt::StructOpt;

use crate::{InstrId, SampleType, WaveForm};

/// An input type that can either be a static number or the
/// id of an instrument from which to get a number
#[derive(Debug, Clone)]
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

/// A Ryvm CLI command
#[derive(Debug, StructOpt)]
pub enum RyvmCommand {
    #[structopt(about = "Quit ryvm", alias = "exit")]
    Quit,
    #[structopt(about = "Set the output instrument")]
    Output {
        #[structopt(index = 1, help = "The id of the new output instrument")]
        name: InstrId,
    },
    #[structopt(about = "Set the project tempo")]
    Tempo {
        #[structopt(index = 1, help = "The new value for the tempo")]
        tempo: f32,
    },
    #[structopt(about = "Create a number source", alias = "num")]
    Number {
        #[structopt(index = 1, help = "The name of the number")]
        name: InstrId,
        #[structopt(index = 2, help = "The value of the number")]
        num: f32,
    },
    #[structopt(about = "Create a wave synthesizer")]
    Wave {
        #[structopt(index = 1, help = "The waveform to use")]
        waveform: WaveForm,
        #[structopt(index = 2, help = "The name of the synthesizer")]
        name: InstrId,
        #[cfg(feature = "keyboard")]
        #[structopt(
            index = 3,
            help = "The id of the instrument supplying the frequency for the wave"
        )]
        input: Option<InstrId>,
        #[cfg(not(feature = "keyboard"))]
        #[structopt(
            index = 3,
            help = "The id of the instrument supplying the frequency for the wave"
        )]
        input: InstrId,
        #[structopt(
            long,
            short,
            allow_hyphen_values = true,
            help = "The synth's octave relative to its input"
        )]
        octave: Option<i8>,
        #[structopt(long, short, help = "The synth's attack")]
        attack: Option<SampleType>,
        #[structopt(long, short, help = "The synth's decay")]
        decay: Option<SampleType>,
        #[structopt(long, short, help = "The synth's sustain")]
        sustain: Option<SampleType>,
        #[structopt(long, short, help = "The synth's release")]
        release: Option<SampleType>,
    },
    #[structopt(about = "Create a mixer")]
    Mixer {
        #[structopt(index = 1, help = "The name of the mixer")]
        name: InstrId,
        #[structopt(index = 2, help = "The mixer's inputs")]
        inputs: Vec<InstrId>,
    },
    #[cfg(feature = "keyboard")]
    #[structopt(about = "Use you computer kyeboard as a music keyboard")]
    Keyboard {
        #[structopt(index = 1, help = "The name of the keyboard interface")]
        name: InstrId,
    },
    #[structopt(about = "Create a new midi instrument")]
    Midi(MidiSubcommand),
    #[structopt(about = "Create a drum machine")]
    Drums {
        #[structopt(index = 1, help = "The name of the drum machine")]
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
    #[cfg(feature = "keyboard")]
    #[structopt(about = "Set the active keyboard")]
    Focus {
        #[structopt(index = 1, help = "The id of the keyboard instrument")]
        id: InstrId,
    },
    #[structopt(about = "Start a new script")]
    Script {
        #[structopt(index = 1, help = "The name of the script")]
        name: InstrId,
        #[structopt(index = 2, help = "The arguments of the script")]
        args: Vec<String>,
    },
    #[structopt(about = "End a script")]
    End,
    #[structopt(about = "Remove an instrument", alias = "remove")]
    Rm {
        #[structopt(index = 1, help = "The id of the instrument to be removed")]
        id: InstrId,
        #[structopt(
            long,
            short,
            help = "Recursively remove all the instrument's unique inputs as well"
        )]
        recursive: bool,
    },
}

#[derive(Debug, StructOpt)]
pub struct NumberCommand {
    #[structopt(index = 1, help = "Set the value of the number")]
    pub val: f32,
}

#[derive(Debug, StructOpt)]
pub struct WaveCommand {
    #[structopt(
        index = 1,
        help = "The id of the instrument supplying the frequency for the wave"
    )]
    pub input: Option<InstrId>,
    #[structopt(long, short, help = "Set the synth's octave relative to its input")]
    pub octave: Option<i8>,
    #[structopt(long, short, help = "Set the synth's attack")]
    pub attack: Option<SampleType>,
    #[structopt(long, short, help = "Set the synth's decay")]
    pub decay: Option<SampleType>,
    #[structopt(long, short, help = "Set the synth's sustain")]
    pub sustain: Option<SampleType>,
    #[structopt(long, short, help = "Set the synth's release")]
    pub release: Option<SampleType>,
}

#[derive(Debug, StructOpt)]
pub struct MixerCommand {
    #[structopt(index = 1, help = "Add to the mixer's inputs")]
    pub inputs: Vec<InstrId>,
    #[structopt(long, short, help = "Set the volume of the specified inputs")]
    pub volume: Option<f32>,
    #[structopt(long, short, help = "Set the pan of the specified inputs")]
    pub pan: Option<f32>,
    #[structopt(long, short, help = "Remove the specified inputs instead")]
    pub remove: bool,
}

#[derive(Debug, StructOpt)]
pub struct FilterCommand {
    #[structopt(index = 1, help = "Defines filter shape")]
    pub value: DynInput,
}

#[derive(Debug, StructOpt)]
pub enum MidiSubcommand {
    #[structopt(about = "List the available midi ports")]
    List,
    #[structopt(about = "Create a new midi inistrument")]
    New {
        #[structopt(index = 1, help = "The name for the midi instrument")]
        name: InstrId,
        #[structopt(
            index = 2,
            help = "The index of the midi port to use (run \"midi list\" to list ports)"
        )]
        port: Option<usize>,
    },
}
