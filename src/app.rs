use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub enum RyvmApp {
    #[structopt(about = "Quit ryvm", alias = "exit")]
    Quit,
    Output {
        name: String,
    },
    Add {
        #[structopt(index = 1)]
        name: String,
        #[structopt(subcommand)]
        app: AddApp,
    },
    Edit {
        #[structopt(index = 1)]
        name: String,
        #[structopt(long, short)]
        set: Option<f32>,
        #[structopt(long = "input", short, index = 2, allow_hyphen_values = true)]
        inputs: Vec<String>,
        #[structopt(long, short)]
        volume: Option<f32>,
        #[structopt(long, short)]
        pan: Option<f32>,
    },
}

#[derive(Debug, StructOpt)]
pub enum AddApp {
    #[structopt(about = "A number", alias = "num")]
    Number { num: f32 },
    #[structopt(about = "A sine wave synthesizer")]
    Sine { input: String },
    #[structopt(about = "A square wave synthesizer")]
    Square { input: String },
    #[structopt(about = "A mixer")]
    Mixer { inputs: Vec<String> },
}
