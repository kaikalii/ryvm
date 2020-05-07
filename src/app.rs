use structopt::StructOpt;

/// A Ryvm CLI command
#[derive(Debug, StructOpt)]
pub(crate) enum RyvmCommand {
    #[structopt(about = "Quit ryvm", alias = "exit")]
    Quit,
    #[structopt(about = "Start recording a loop. Press enter to finish recording.")]
    Loop {
        #[structopt(help = "The number of the loop to record")]
        num: Option<u8>,
        #[structopt(
            long,
            short = "x",
            help = "The length of the loop relative to the first one"
        )]
        length: Option<f32>,
    },
    #[structopt(about = "Start playing a loop")]
    Play {
        #[structopt(required = true, help = "The numbers of the loops to play")]
        loops: Vec<u8>,
    },
    #[structopt(about = "Stop playing a loop")]
    Stop {
        #[structopt(help = "The numbers of the loops to stop")]
        loops: Vec<u8>,
        #[structopt(long, conflicts_with = "names", help = "Stop all loops")]
        all: bool,
        #[structopt(
            long,
            short,
            conflicts_with = "loops",
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
    #[structopt(about = "Set the current channel for manual-controlled devices")]
    Ch {
        #[structopt(help = "The channel to set")]
        channel: u8,
    },
    #[structopt(about = "List all available midi ports")]
    Midi(MidiSubCommand),
    #[structopt(about = "Load a spec file")]
    Load {
        #[structopt(help = "The name of the spec")]
        name: String,
        #[structopt(help = "The channel to load into")]
        channel: Option<u8>,
    },
}

#[derive(Debug, StructOpt)]
pub(crate) enum MidiSubCommand {
    #[structopt(about = "List all available midi ports")]
    List,
    #[structopt(about = "Monitor midi input. Use again to stop.")]
    Monitor,
}

/// The command line argument parser for Ryvm
#[derive(Debug, Default, StructOpt)]
pub struct RyvmApp {
    #[structopt(
        short = "r",
        long,
        default_value = "44100",
        about = "List the available midi ports"
    )]
    /// The sample rate for the session
    pub sample_rate: u32,
}
