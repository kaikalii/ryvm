use std::{
    cmp::Ordering,
    fmt,
    iter::once,
    ops::{Deref, DerefMut},
    sync::{Mutex, MutexGuard},
};

use crossbeam_utils::atomic::AtomicCell;
use rodio::DeviceTrait;
use serde_derive::{Deserialize, Serialize};

use crate::{InputError, Name, NAME_CAPACITY};

#[macro_export]
macro_rules! colorprintln {
    ($fmt:literal, $col:ident $(,$item:expr)* $(,)?) => {
        println!("{}", colored::Colorize::$col(format!($fmt, $($item),*).as_str()))
    };
}

pub fn list_output_devices() -> Result<(), InputError> {
    for (i, device) in rodio::output_devices()?.enumerate() {
        colorprintln!("{}. {}", bright_cyan, i, device.name()?);
    }
    Ok(())
}

pub fn name_from_str(s: &str) -> Name {
    Name::from(&s[..s.len().min(NAME_CAPACITY)]).unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Float(pub f32);

impl Eq for Float {}

impl Ord for Float {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Less)
    }
}

impl From<f32> for Float {
    fn from(f: f32) -> Self {
        Float(f)
    }
}

impl From<Float> for f32 {
    fn from(f: Float) -> f32 {
        f.0
    }
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
