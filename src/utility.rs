use std::{
    fmt,
    sync::{
        atomic::{AtomicU32, Ordering},
        Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
};

use serde_derive::{Deserialize, Serialize};

pub fn parse_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut in_quotes = false;
    let mut arg = String::new();
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
    args
}

pub enum Delayed<T> {
    Running(Option<JoinHandle<T>>),
    Done(T),
}

impl<T> Default for Delayed<T>
where
    T: Default,
{
    fn default() -> Self {
        Delayed::Done(T::default())
    }
}

impl<T> Delayed<T>
where
    T: Send + 'static,
{
    pub fn new<F>(f: F) -> Self
    where
        F: FnOnce() -> T + Send + 'static,
    {
        Delayed::Running(Some(thread::spawn(f)))
    }
    pub fn resolve(&mut self) -> &mut T {
        match self {
            Delayed::Running(handle) => {
                let val = handle.take().unwrap().join().unwrap();
                *self = Delayed::Done(val);
                if let Delayed::Done(val) = self {
                    val
                } else {
                    unreachable!()
                }
            }
            Delayed::Done(val) => val,
        }
    }
}

#[derive(Default)]
pub struct U32Lock(AtomicU32);

impl fmt::Debug for U32Lock {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.load())
    }
}

impl Clone for U32Lock {
    fn clone(&self) -> Self {
        U32Lock::new(self.load())
    }
}

impl U32Lock {
    pub fn new(val: u32) -> Self {
        U32Lock(AtomicU32::new(val))
    }
    pub fn load(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }
    pub fn store(&self, val: u32) {
        self.0.store(val, Ordering::Relaxed)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
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
