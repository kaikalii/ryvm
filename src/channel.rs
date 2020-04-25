use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fmt,
    iter::{once, FromIterator},
    str::FromStr,
};

use serde_derive::{Deserialize, Serialize};
use tinymap::{tiny_map, Inner, TinyMap};

use crate::{Balance, Letter, SampleType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum InstrIdType {
    Base,
    Filter(u8),
    Loop(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    pub fn as_filter(&self, filter_num: u8) -> Self {
        InstrId {
            name: self.name.clone(),
            ty: InstrIdType::Filter(filter_num),
        }
    }
    pub fn as_ref(&self) -> InstrIdRef {
        InstrIdRef {
            name: self.name.as_ref(),
            ty: self.ty,
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
        let mut name = String::new();
        let mut secondary = String::new();
        let mut is_loop = false;
        let mut is_filter = false;
        for c in s.chars() {
            match c {
                '#' => is_filter = true,
                '@' => is_loop = true,
                c => {
                    if is_filter || is_loop {
                        secondary.push(c);
                    } else {
                        name.push(c);
                    }
                }
            }
        }
        Ok(if let Ok(i) = secondary.parse::<u8>() {
            if is_filter {
                InstrId {
                    name,
                    ty: InstrIdType::Filter(i),
                }
            } else if is_loop {
                InstrId {
                    name,
                    ty: InstrIdType::Loop(i),
                }
            } else {
                InstrId::new_base(name)
            }
        } else {
            InstrId::new_base(name)
        })
    }
}

impl<'a> From<&'a InstrId> for InstrId {
    fn from(id_ref: &'a InstrId) -> Self {
        InstrId {
            name: id_ref.name.to_owned(),
            ty: id_ref.ty,
        }
    }
}

impl fmt::Display for InstrId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        <InstrIdRef as fmt::Display>::fmt(&self.as_ref(), f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstrIdRef<'a> {
    name: &'a str,
    ty: InstrIdType,
}

impl<'a> fmt::Display for InstrIdRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.ty {
            InstrIdType::Base => write!(f, "{}", self.name),
            InstrIdType::Filter(i) => write!(f, "{}#{}", self.name, i),
            InstrIdType::Loop(i) => write!(f, "{}@{}", self.name, i),
        }
    }
}

impl<'a> From<InstrIdRef<'a>> for InstrId {
    fn from(id_ref: InstrIdRef<'a>) -> Self {
        InstrId {
            name: id_ref.name.into(),
            ty: id_ref.ty,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Voice {
    pub left: SampleType,
    pub right: SampleType,
}

impl Voice {
    pub fn stereo(left: SampleType, right: SampleType) -> Self {
        Voice { left, right }
    }
    pub fn mono(both: SampleType) -> Self {
        Voice::stereo(both, both)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Control {
    StartNote(Letter, u8, u8),
    EndNote(Letter, u8),
    EndAllNotes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Frame {
    Voice(Voice),
    Controls(Vec<Control>),
    None,
}

impl Default for Frame {
    fn default() -> Self {
        Frame::None
    }
}

impl Frame {
    pub fn left(&self) -> SampleType {
        if let Frame::Voice(voice) = self {
            voice.left
        } else {
            0.0
        }
    }
    pub fn right(&self) -> SampleType {
        if let Frame::Voice(voice) = self {
            voice.right
        } else {
            0.0
        }
    }
    pub fn voice(&self) -> Voice {
        if let Frame::Voice(voice) = self {
            *voice
        } else {
            Default::default()
        }
    }
    #[cfg(feature = "keyboard")]
    pub fn controls<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Control>,
    {
        Frame::Controls(iter.into_iter().collect())
    }
    pub fn is_some(&self) -> bool {
        match self {
            Frame::Voice(_) => true,
            Frame::Controls(controls) => !controls.is_empty(),
            Frame::None => false,
        }
    }
}

impl From<Voice> for Frame {
    fn from(voice: Voice) -> Self {
        Frame::Voice(voice)
    }
}

impl From<Control> for Frame {
    fn from(conrol: Control) -> Self {
        Frame::Controls(vec![conrol])
    }
}

pub fn mix(list: &[(Voice, Balance)]) -> Frame {
    if list.is_empty() {
        return Frame::None;
    }
    let (left_sum, right_sum) = list.iter().fold((0.0, 0.0), |(lacc, racc), (voice, bal)| {
        let (l, r) = bal.stereo_volume();
        (lacc + voice.left * l, racc + voice.right * r)
    });
    Voice::stereo(left_sum, right_sum).into()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChannelId {
    Primary,
    Loop(InstrId),
}

type ChannelMapArray<T> = [Inner<(ChannelId, T)>; 10];

#[derive(Debug, Clone)]
pub struct Channels<T = Frame>(TinyMap<[Inner<(ChannelId, T)>; 10]>);

impl<T> Default for Channels<T> {
    fn default() -> Self {
        Channels(TinyMap::new())
    }
}

impl Channels {
    pub fn frames(&self) -> tiny_map::Values<ChannelId, Frame> {
        self.values()
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
    #[allow(dead_code)]
    pub fn iter(&self) -> tiny_map::Iter<ChannelId, T> {
        self.0.iter()
    }
    pub fn values(&self) -> tiny_map::Values<ChannelId, T> {
        self.0.values()
    }
    pub fn values_mut(&mut self) -> tiny_map::ValuesMut<ChannelId, T> {
        self.0.values_mut()
    }
    pub fn entry(&mut self, id: ChannelId) -> tiny_map::Entry<ChannelMapArray<T>> {
        self.0.entry(id)
    }
    pub fn get_mut(&mut self, id: &ChannelId) -> Option<&mut T> {
        self.0.get_mut(id)
    }
    pub fn id_map<F>(&self, mut f: F) -> Channels<T>
    where
        F: FnMut(&ChannelId, &T) -> T,
    {
        self.0
            .iter()
            .map(|(id, frame)| (id.clone(), f(id, frame)))
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
        Channels(TinyMap::from_iter(iter))
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
    type IntoIter = tiny_map::IntoIter<ChannelId, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Cache for each frame
#[derive(Default)]
pub struct FrameCache {
    pub map: HashMap<InstrId, Channels<Frame>>,
    pub visited: HashSet<InstrId>,
    pub default_channels: Channels<Frame>,
}
