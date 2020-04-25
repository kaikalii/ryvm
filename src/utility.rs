use std::sync::{Mutex, MutexGuard};

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
