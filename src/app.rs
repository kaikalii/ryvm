use clap::{App, Arg, SubCommand};

pub fn app() -> App<'static, 'static> {
    App::new("ryvm").args(&[])
}
