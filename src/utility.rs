use std::{
    fmt,
    iter::once,
    ops::{Deref, DerefMut},
    sync::{Mutex, MutexGuard},
};

use crossbeam_utils::atomic::AtomicCell;

pub fn adjust_i(i: u32, recording_tempo: f32, current_tempo: f32) -> u32 {
    (i as f32 * current_tempo.abs() / recording_tempo.abs()).round() as u32
}

pub fn parse_commands(text: &str) -> Option<Vec<(bool, Vec<String>)>> {
    if text.trim().is_empty() {
        None
    } else {
        Some(
            text.split(',')
                .map(|text| {
                    let (delay, parsed) = parse_args(text.trim());
                    (delay, once("ryvm".into()).chain(parsed).collect::<Vec<_>>())
                })
                .collect(),
        )
    }
}

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
