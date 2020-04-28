use std::{
    env::{current_dir, current_exe},
    fmt,
    iter::once,
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};

use crossbeam_utils::atomic::AtomicCell;
use find_folder::Search;

// pub fn adjust_i(i: u32, recording_tempo: f32, current_tempo: f32) -> u32 {
//     (i as f32 * current_tempo.abs() / recording_tempo.abs()).round() as u32
// }

pub fn parse_args(s: &str) -> (bool, Vec<String>) {
    let mut args = Vec::new();
    let mut in_quotes = false;
    let mut arg = String::new();
    let mut delay = false;
    macro_rules! insert_arg {
        () => {{
            let mut next_arg = String::new();
            std::mem::swap(&mut next_arg, &mut arg);
            args.push(next_arg);
        }};
    }
    for c in s.chars() {
        match c {
            '"' => {
                if in_quotes {
                    in_quotes = false;
                    insert_arg!();
                } else {
                    in_quotes = true;
                }
            }
            '`' => delay = true,
            c if c.is_whitespace() => {
                if in_quotes {
                    arg.push(c)
                } else if !arg.is_empty() {
                    insert_arg!();
                }
            }
            c => arg.push(c),
        }
    }
    if !arg.is_empty() {
        insert_arg!();
    }
    (delay, args)
}

pub type DelayedCommands = (bool, Vec<String>);

pub fn load_script(name: &str) -> Option<(Vec<String>, Vec<DelayedCommands>)> {
    let folder = "scripts";
    let search = Search::KidsThenParents(2, 1);
    let scripts_path = search
        .of(current_dir().ok()?)
        .for_folder(&folder)
        .or_else(|_| search.of(current_exe()?).for_folder(&folder))
        .ok()?;
    let script_path = scripts_path.join(PathBuf::from(name).with_extension("ryvm"));
    let script_str = std::fs::read_to_string(script_path).ok()?;
    let lines = script_str.lines().filter(|line| !line.trim().is_empty());
    let mut script_args = Vec::new();
    let mut commands = Vec::new();
    for line in lines {
        let mut is_args = false;
        let line = if line.starts_with(':') {
            is_args = true;
            &line[1..]
        } else {
            line
        };
        let (delay, args) = parse_args(line);
        if is_args {
            script_args = args;
        } else {
            commands.push((
                delay,
                once("ryvm".to_string()).chain(args).collect::<Vec<_>>(),
            ));
        }
    }
    Some((script_args, commands))
}

#[derive(Debug, Default)]
pub struct CloneLock<T>(Mutex<T>);

impl<T> Clone for CloneLock<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        CloneLock::new(self.lock().clone())
    }
}

impl<T> CloneLock<T> {
    pub fn new(val: T) -> Self {
        CloneLock(Mutex::new(val))
    }
    pub fn lock(&self) -> MutexGuard<T> {
        self.0.lock().unwrap()
    }
}

#[derive(Default)]
pub struct CloneCell<T>(AtomicCell<T>);

impl<T> CloneCell<T> {
    pub fn new(val: T) -> Self {
        CloneCell(AtomicCell::new(val))
    }
}

impl<T> Clone for CloneCell<T>
where
    T: Copy,
{
    fn clone(&self) -> Self {
        CloneCell::new(self.load())
    }
}

impl<T> Deref for CloneCell<T> {
    type Target = AtomicCell<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for CloneCell<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> fmt::Debug for CloneCell<T>
where
    T: fmt::Debug + Copy,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        <AtomicCell<T> as fmt::Debug>::fmt(&self.0, f)
    }
}
