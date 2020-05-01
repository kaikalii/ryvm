use std::{
    f32, fmt,
    ops::{Add, AddAssign, Mul},
    sync::Arc,
};

use crate::CloneLock;

/// A lock used primarily to allow the manipulation of a rodio::Source
/// while it is already playing
#[derive(Debug)]
pub struct SourceLock<T>(Arc<CloneLock<T>>);

impl<T> Clone for SourceLock<T> {
    fn clone(&self) -> Self {
        SourceLock(Arc::clone(&self.0))
    }
}

impl<T> SourceLock<T> {
    pub fn new(inner: T) -> Self {
        SourceLock(Arc::new(CloneLock::new(inner)))
    }
    pub fn update<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        f(&mut *self.0.lock())
    }
}

impl<T> Iterator for SourceLock<T>
where
    T: Iterator,
{
    type Item = T::Item;
    fn next(&mut self) -> Option<Self::Item> {
        self.update(Iterator::next)
    }
}

#[derive(Clone, Copy, Default)]
pub struct Voice {
    pub left: f32,
    pub right: f32,
}

impl Voice {
    pub const SILENT: Self = Voice {
        left: 0.0,
        right: 0.0,
    };
    pub fn stereo(left: f32, right: f32) -> Self {
        Voice { left, right }
    }
    pub fn mono(both: f32) -> Self {
        Voice::stereo(both, both)
    }
    pub fn is_silent(self) -> bool {
        self.left.abs() < f32::EPSILON && self.right.abs() < f32::EPSILON
    }
}

impl Mul<f32> for Voice {
    type Output = Self;
    fn mul(self, m: f32) -> Self::Output {
        Voice {
            left: self.left * m,
            right: self.right * m,
        }
    }
}

impl Mul for Voice {
    type Output = Self;
    fn mul(self, other: Self) -> Self::Output {
        Voice {
            left: self.left * other.left,
            right: self.right * other.right,
        }
    }
}

impl AddAssign for Voice {
    fn add_assign(&mut self, other: Self) {
        self.left += other.left;
        self.right += other.right;
    }
}

impl Add for Voice {
    type Output = Self;
    fn add(mut self, other: Self) -> Self::Output {
        self += other;
        self
    }
}

impl fmt::Debug for Voice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.is_silent() {
            write!(f, "silent")
        } else if (self.left - self.right).abs() < std::f32::EPSILON {
            write!(f, "{}", self.left)
        } else {
            write!(f, "({}, {})", self.left, self.right)
        }
    }
}
