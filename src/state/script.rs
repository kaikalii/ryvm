use structopt::clap;

use crate::{load_script, RyvmCommand, State, StructOpt};

#[derive(Debug, Clone)]
pub struct Script {
    pub name: String,
    pub arguments: Vec<String>,
    pub unresolved_commands: Vec<(bool, Vec<String>)>,
}

impl State {
    pub fn load_script(&mut self, name: &str, reload: bool) {
        if self.scripts.get(name).is_none() || reload {
            if let Some((arguments, unresolved_commands)) = load_script(&name) {
                self.scripts.insert(
                    name.into(),
                    Script {
                        name: name.into(),
                        arguments,
                        unresolved_commands,
                    },
                );
            }
        }
    }
    pub fn run_script_by_name(&mut self, name: &str, args: &[String]) -> Result<(), String> {
        if let Some(script) = self.scripts.get(name).cloned() {
            self.run_script(args, name, script.arguments, script.unresolved_commands)
        } else {
            Ok(())
        }
    }
    pub fn run_script(
        &mut self,
        command_args: &[String],
        script_name: &str,
        script_args: Vec<String>,
        unresolved_commands: Vec<(bool, Vec<String>)>,
    ) -> Result<(), String> {
        let script_clap_args: Vec<clap::Arg> = script_args
            .iter()
            .enumerate()
            .map(|(i, arg_name)| {
                clap::Arg::with_name(arg_name)
                    .index(i as u64 + 1)
                    .required(true)
            })
            .collect();
        let script_app = clap::App::new(script_name).args(&script_clap_args);
        let matches = script_app
            .get_matches_from_safe(command_args)
            .map_err(|e| e.to_string())?;
        let mut resolved_commands = Vec::new();
        for (delay, unresolved_command) in unresolved_commands {
            let resolved_command: Vec<String> = unresolved_command
                .iter()
                .map(|arg| {
                    if let Some(script_arg) = script_args.iter().find(|sa| sa == &arg) {
                        matches.value_of(script_arg).unwrap().into()
                    } else {
                        arg.clone()
                    }
                })
                .collect();
            let parsed = RyvmCommand::from_iter_safe(&resolved_command);
            resolved_commands.push((delay, resolved_command, parsed))
        }
        // let mut depth = 0;
        for (delay, args, parsed) in resolved_commands {
            // if let Some("end") = args.get(1).map(|s| s.as_str()) {
            //     depth -= 1;
            // }
            // print!("> {}", (0..depth).map(|_| "  ").collect::<String>());
            // for arg in args.iter().skip(1) {
            //     print!("{} ", arg);
            // }
            // println!();
            // if let Some("script") = args.get(1).map(|s| s.as_str()) {
            //     depth += 1;
            // }
            self.queue_command(delay, args, parsed);
        }
        Ok(())
    }
}
