use std::{
    fmt,
    sync::{
        atomic::{AtomicU32, Ordering},
        Mutex, MutexGuard,
    },
};

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
