use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fmt,
    iter::{once, FromIterator},
    str::FromStr,
};

use tinymap::{tiny_map, Inner, TinyMap};

use crate::{Balance, Letter, SampleType};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InstrId {
    Base(String),
    Filter(String, u8),
    Loop(u8),
    Nothing,
}

impl Default for InstrId {
    fn default() -> Self {
        InstrId::Nothing
    }
}

impl InstrId {
    pub fn new_base<S>(name: S) -> Self
    where
        S: Into<String>,
    {
        InstrId::Base(name.into())
    }
    pub fn as_filter(&self, filter_num: u8) -> Self {
        match self {
            InstrId::Base(name) => InstrId::Filter(name.clone(), filter_num),
            InstrId::Filter(name, _) => InstrId::Filter(name.clone(), filter_num),
            id => id.clone(),
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
        if s.trim() == "<nothing>" {
            return Ok(InstrId::Nothing);
        }
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
                InstrId::Filter(name, i)
            } else if is_loop {
                InstrId::Loop(i)
            } else {
                InstrId::new_base(name)
            }
        } else {
            InstrId::new_base(name)
        })
    }
}

impl fmt::Display for InstrId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            InstrId::Base(name) => write!(f, "{}", name),
            InstrId::Filter(name, i) => write!(f, "{}#{}", name, i),
            InstrId::Loop(i) => write!(f, "@{}", i),
            InstrId::Nothing => write!(f, "<nothing>"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    StartNote(Letter, u8, u8),
    EndNote(Letter, u8),
    EndAllNotes,
    PitchBend(SampleType),
    Controller(u8, SampleType),
}

#[derive(Debug, Clone)]
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
