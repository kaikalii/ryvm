use clap::{App, SubCommand};

pub fn app() -> App<'static, 'static> {
    App::new("ryvm").subcommands(vec![SubCommand::with_name("quit")
        .about("Quit ryvm")
        .alias("exit")])
}
