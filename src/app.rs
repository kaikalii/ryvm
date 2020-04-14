use clap::{App, Arg, ArgGroup, SubCommand};

pub fn app() -> App<'static, 'static> {
    App::new("ryvm").subcommands(vec![
        SubCommand::with_name("quit")
            .about("Quit ryvm")
            .alias("exit"),
        SubCommand::with_name("add")
            .about("Add an instrument")
            .args(&[
                // Name
                Arg::with_name("NAME")
                    .help("The name of the new instrument")
                    .index(1)
                    .required(true),
                // Types
                Arg::with_name("mixer").long("mixer"),
                Arg::with_name("number")
                    .long("number")
                    .long("num")
                    .short("n")
                    .takes_value(true)
                    .value_name("n"),
                Arg::with_name("sine")
                    .long("sine")
                    .short("sin")
                    .takes_value(true)
                    .value_name("input"),
                Arg::with_name("square")
                    .long("square")
                    .takes_value(true)
                    .value_name("input"),
            ])
            .groups(&[ArgGroup::with_name("TYPE")
                .args(&["mixer", "number", "sine", "square"])
                .required(true)]),
    ])
}
