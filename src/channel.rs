use std::{
    collections::{hash_map, HashMap, HashSet},
    convert::Infallible,
    fmt,
    iter::{once, FromIterator},
    str::FromStr,
};

use serde_derive::{Deserialize, Serialize};

use crate::SampleType;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstrIdType {
    Base,
    Sub(String),
    Loop(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstrId {
    pub name: String,
    pub ty: InstrIdType,
}

impl Default for InstrId {
    fn default() -> Self {
        InstrId {
            name: String::new(),
            ty: InstrIdType::Base,
        }
    }
}

impl InstrId {
    pub fn new_base<S>(name: S) -> Self
    where
        S: Into<String>,
    {
        InstrId {
            name: name.into(),
            ty: InstrIdType::Base,
        }
    }
    pub fn as_loop(&self, loop_num: u8) -> Self {
        InstrId {
            name: self.name.clone(),
            ty: InstrIdType::Loop(loop_num),
        }
    }
    pub fn sub<S>(&self, sub_name: S) -> Self
    where
        S: Into<String>,
    {
        InstrId {
            name: self.name.clone(),
            ty: InstrIdType::Sub(sub_name.into()),
        }
    }
    pub fn as_ref(&self) -> InstrIdRef {
        InstrIdRef {
            name: self.name.as_ref(),
            ty: &self.ty,
        }
    }
}

impl<S> From<S> for InstrId
where
    S: AsRef<str>,
{
    fn from(s: S) -> Self {
        s.as_ref().parse().unwrap()
    }
}

impl FromStr for InstrId {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut iter = s.split('-').filter(|s| !s.is_empty());
        let name = iter.next().map(Into::into).unwrap_or_default();
        Ok(InstrId {
            name,
            ty: if let Some(sub_name) = iter.next() {
                InstrIdType::Sub(sub_name.into())
            } else {
                InstrIdType::Base
            },
        })
    }
}

impl<'a> From<&'a InstrId> for InstrId {
    fn from(id_ref: &'a InstrId) -> Self {
        InstrId {
            name: id_ref.name.to_owned(),
            ty: id_ref.ty.clone(),
        }
    }
}

impl fmt::Display for InstrId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        <InstrIdRef as fmt::Display>::fmt(&self.as_ref(), f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstrIdRef<'a> {
    name: &'a str,
    ty: &'a InstrIdType,
}

impl<'a> fmt::Display for InstrIdRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.ty {
            InstrIdType::Base => write!(f, "{}", self.name),
            InstrIdType::Sub(sub_name) => write!(f, "{}-{}", self.name, sub_name),
            InstrIdType::Loop(i) => write!(f, "{}-loop{}", self.name, i),
        }
    }
}

impl<'a> From<InstrIdRef<'a>> for InstrId {
    fn from(id_ref: InstrIdRef<'a>) -> Self {
        InstrId {
            name: id_ref.name.into(),
            ty: id_ref.ty.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Voice {
    pub left: SampleType,
    pub right: SampleType,
    pub velocity: SampleType,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Frame {
    pub first: Voice,
    pub extra: Vec<Voice>,
}

impl Voice {
    pub fn stereo(left: SampleType, right: SampleType) -> Self {
        Voice {
            left,
            right,
            velocity: 1.0,
        }
    }
    pub fn mono(both: SampleType) -> Self {
        Voice::stereo(both, both)
    }
    pub fn velocity(self, velocity: SampleType) -> Self {
        Voice { velocity, ..self }
    }
}

impl Frame {
    pub fn multi<I>(iter: I) -> Option<Self>
    where
        I: IntoIterator<Item = Voice>,
    {
        let mut iter = iter.into_iter();
        let mut frame = Frame {
            first: Voice::default(),
            extra: Vec::new(),
        };
        if let Some(first) = iter.next() {
            frame.first = first;
            frame.extra.extend(iter);
            Some(frame)
        } else {
            None
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = Voice> + '_ {
        once(self.first).chain(self.extra.iter().copied())
    }
}

impl IntoIterator for Frame {
    type Item = Voice;
    type IntoIter = Box<dyn Iterator<Item = Voice>>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(once(self.first).chain(self.extra.into_iter()))
    }
}

impl From<Voice> for Frame {
    fn from(voice: Voice) -> Self {
        Frame {
            first: voice,
            extra: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChannelId {
    Primary,
    Loop(InstrId),
}

#[derive(Debug, Clone)]
pub struct Channels<T = Frame>(HashMap<ChannelId, T>);

impl<T> Default for Channels<T> {
    fn default() -> Self {
        Channels(HashMap::new())
    }
}

impl<T> Channels<T> {
    pub fn new() -> Self {
        Channels::default()
    }
    pub fn primary(&self) -> Option<&T> {
        self.0.get(&ChannelId::Primary)
    }
    pub fn into_primary(mut self) -> Option<T> {
        self.0.remove(&ChannelId::Primary)
    }
    pub fn iter(&self) -> hash_map::Iter<ChannelId, T> {
        self.0.iter()
    }
    pub fn frames(&self) -> hash_map::Values<ChannelId, T> {
        self.0.values()
    }
    pub fn entry(&mut self, id: ChannelId) -> hash_map::Entry<ChannelId, T> {
        self.0.entry(id)
    }
    pub fn map<F>(&self, mut f: F) -> Channels<T>
    where
        F: FnMut(&T) -> T,
    {
        self.0
            .iter()
            .map(|(id, frame)| (id.clone(), f(frame)))
            .collect()
    }
    pub fn filter_map<F>(&self, mut f: F) -> Channels<T>
    where
        F: FnMut(&T) -> Option<T>,
    {
        self.0
            .iter()
            .filter_map(|(id, frame)| f(frame).map(|frame| (id.clone(), frame)))
            .collect()
    }
}

impl<T> From<T> for Channels<T> {
    fn from(frame: T) -> Self {
        Channels(once((ChannelId::Primary, frame)).collect())
    }
}

impl<T> From<Option<T>> for Channels<T> {
    fn from(frame: Option<T>) -> Self {
        Channels(frame.into_iter().map(|f| (ChannelId::Primary, f)).collect())
    }
}

impl<T> FromIterator<(ChannelId, T)> for Channels<T> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = (ChannelId, T)>,
    {
        Channels(HashMap::from_iter(iter))
    }
}

impl<T> Extend<(ChannelId, T)> for Channels<T> {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (ChannelId, T)>,
    {
        self.0.extend(iter)
    }
}

impl<T> IntoIterator for Channels<T> {
    type Item = (ChannelId, T);
    type IntoIter = hash_map::IntoIter<ChannelId, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Cache for each frame
#[derive(Default)]
pub(crate) struct FrameCache {
    pub map: HashMap<InstrId, Channels<Frame>>,
    pub visited: HashSet<InstrId>,
    pub default_channels: Channels<Frame>,
}
