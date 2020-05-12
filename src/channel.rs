use std::{
    collections::{hash_map, HashMap, HashSet},
    f32, fmt,
    ops::{Add, AddAssign, Mul},
};

use crate::{name_from_str, Control, Name, Node, Port, State};

/// A midi channel that can contain many nodes
#[derive(Debug, Default)]
pub struct Channel {
    nodes: HashMap<Name, Node>,
}

impl Channel {
    /// Get a node by name
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Node> {
        self.nodes.get(name)
    }
    /// Get a node map entry
    pub fn entry(&mut self, name: Name) -> hash_map::Entry<Name, Node> {
        self.nodes.entry(name)
    }
    /// Get an iterator over the node names
    #[must_use]
    pub fn node_names(&self) -> hash_map::Keys<Name, Node> {
        self.nodes.keys()
    }
    /// Get an iterator over the nodes
    #[must_use]
    pub fn nodes(&self) -> hash_map::Values<Name, Node> {
        self.nodes.values()
    }
    /// Get an iterator over mutable references to the nodes
    #[must_use]
    pub fn nodes_mut(&mut self) -> hash_map::ValuesMut<Name, Node> {
        self.nodes.values_mut()
    }
    /// Get an iterator over names and nodes
    #[must_use]
    pub fn names_nodes(&self) -> hash_map::Iter<Name, Node> {
        self.nodes.iter()
    }
    // /// Get an iterator over names and mutable references to the nodes
    // #[must_use]
    // pub fn names_nodes_mut(&mut self) -> hash_map::IterMut<Name, Node> {
    //     self.nodes.iter_mut()
    // }
    /// Get an iterator over the names of nodes in this channel that should be output
    pub fn outputs(&self) -> impl Iterator<Item = &str> + '_ {
        self.node_names()
            .map(AsRef::as_ref)
            .filter(move |name| !self.nodes().any(|node| node.inputs().contains(name)))
    }
    /// Retain nodes that satisfy the predicate
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&Name, &mut Node) -> bool,
    {
        self.nodes.retain(f)
    }
    // /// Clear all nodes
    // pub fn clear(&mut self) {
    //     self.nodes.clear();
    // }
    /// Remove a node and optionally recursively delete all of its unique inputs
    pub fn remove(&mut self, name: &str, recursive: bool) {
        if let Some(node) = self.get(name) {
            if recursive {
                let inputs: Vec<Name> = node.inputs().into_iter().map(name_from_str).collect();
                for input in inputs {
                    if !self
                        .nodes
                        .iter()
                        .filter(|(i, _)| i != &name)
                        .any(|(_, node)| node.inputs().contains(&&*input))
                    {
                        self.remove(&input, recursive);
                    }
                }
            }
            self.nodes.remove(name);
        }
    }
    #[must_use]
    pub fn next_from(
        &self,
        channel_num: u8,
        name: &str,
        state: &State,
        cache: &mut FrameCache,
    ) -> Voice {
        let full_name = (channel_num, name_from_str(name));
        if cache.visited.contains(&full_name) {
            // Avoid infinite loops
            Voice::mono(0.0)
        } else {
            cache.visited.insert(full_name.clone());
            if let Some(node) = self.get(name) {
                if let Some(voice) = cache.voices.get(&full_name) {
                    *voice
                } else {
                    let voice = node.next(channel_num, self, state, cache, name);
                    cache.voices.insert(full_name, voice);
                    voice
                }
            } else {
                Voice::SILENT
            }
        }
    }
}

pub struct FrameCache {
    pub voices: HashMap<(u8, Name), Voice>,
    pub controls: HashMap<(Port, u8), Vec<Control>>,
    pub audio_input: HashMap<Name, Voice>,
    pub visited: HashSet<(u8, Name)>,
    pub from_loop: bool,
}

impl FrameCache {
    // pub fn all_controls(&self) -> impl Iterator<Item = Control> + '_ {
    //     self.controls.values().flat_map(|v| v.iter().copied())
    // }
    pub fn channel_controls(&self, channel: u8) -> impl Iterator<Item = Control> + '_ {
        self.controls
            .iter()
            .filter(move |((_, ch), _)| ch == &channel)
            .flat_map(|(_, controls)| controls.iter().copied())
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
