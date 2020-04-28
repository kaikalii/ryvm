use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fmt,
    iter::{once, FromIterator},
    ops::Mul,
    str::FromStr,
};

use tinymap::{tiny_map, Inner, TinyMap};

use crate::{Balance, FocusType, Instruments, Letter};

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
    pub fn default_input_device() -> Self {
        "midi0".into()
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

#[derive(Clone, Copy, Default)]
pub struct Voice {
    pub left: f32,
    pub right: f32,
}

impl Voice {
    pub fn stereo(left: f32, right: f32) -> Self {
        Voice { left, right }
    }
    pub fn mono(both: f32) -> Self {
        Voice::stereo(both, both)
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

impl fmt::Debug for Voice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if (self.left - self.right).abs() < std::f32::EPSILON {
            write!(f, "{}", self.left)
        } else {
            write!(f, "({}, {})", self.left, self.right)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    NoteStart(Letter, u8, u8),
    NoteEnd(Letter, u8),
    EndAllNotes,
    PitchBend(f32),
    Controller(u8, u8),
    PadStart(Letter, u8, u8),
    PadEnd(Letter, u8),
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
    pub fn left(&self) -> f32 {
        if let Frame::Voice(voice) = self {
            voice.left
        } else {
            0.0
        }
    }
    pub fn right(&self) -> f32 {
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
        let controls: Vec<_> = iter.into_iter().collect();
        if controls.is_empty() {
            Frame::None
        } else {
            Frame::Controls(controls)
        }
    }
    pub fn is_some(&self) -> bool {
        match self {
            Frame::Voice(_) => true,
            Frame::Controls(controls) => !controls.is_empty(),
            Frame::None => false,
        }
    }
    pub fn some(self) -> Option<Self> {
        Some(self).filter(Frame::is_some)
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

impl FromIterator<Control> for Frame {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Control>,
    {
        Frame::controls(iter)
    }
}

impl Extend<Control> for Frame {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = Control>,
    {
        match self {
            Frame::None => *self = Frame::controls(iter),
            Frame::Controls(controls) => controls.extend(iter),
            _ => {}
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChannelId {
    Focus(bool, FocusType),
    Loop(bool, u8),
    Control,
    Dummy,
}

impl ChannelId {
    pub fn is_validated(self) -> bool {
        match self {
            ChannelId::Focus(validated, _) | ChannelId::Loop(validated, _) => validated,
            ChannelId::Dummy => false,
            ChannelId::Control => true,
        }
    }
}

type ChannelMapArray = [Inner<(ChannelId, Frame)>; 10];

#[derive(Debug, Clone)]
pub struct Channels(TinyMap<ChannelMapArray>);

impl Default for Channels {
    fn default() -> Self {
        Channels(TinyMap::new())
    }
}

impl Channels {
    pub fn new() -> Self {
        Channels::default()
    }
    pub fn end_all_notes() -> Self {
        (ChannelId::Dummy, Frame::from(Control::EndAllNotes)).into()
    }
    pub fn split_controls<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Control>,
    {
        let (keyboard, others): (Vec<Control>, _) = iter.into_iter().partition(|control| {
            matches!(
                control,
                Control::NoteStart(..)
                    | Control::NoteEnd(..)
                    | Control::PitchBend(..)
                    | Control::EndAllNotes
            )
        });
        let keyboard = Frame::controls(keyboard);
        let (drums, controllers): (Frame, Frame) = others
            .into_iter()
            .partition(|control| matches!(control, Control::PadStart(..) | Control::PadEnd(..)));
        keyboard
            .some()
            .into_iter()
            .map(|frame| (ChannelId::Focus(false, FocusType::Keyboard), frame))
            .chain(
                drums
                    .some()
                    .into_iter()
                    .map(|frame| (ChannelId::Focus(false, FocusType::Drum), frame)),
            )
            .chain(
                controllers
                    .some()
                    .into_iter()
                    .map(|frame| (ChannelId::Control, frame)),
            )
            .collect()
    }
    pub fn iter(&self) -> tiny_map::Iter<ChannelId, Frame> {
        self.0.iter()
    }
    // pub fn keys(&self) -> tiny_map::Keys<ChannelId, Frame> {
    //     self.0.keys()
    // }
    pub fn values(&self) -> tiny_map::Values<ChannelId, Frame> {
        self.0.values()
    }
    // pub fn values_mut(&mut self) -> tiny_map::ValuesMut<ChannelId, Frame> {
    //     self.0.values_mut()
    // }
    // pub fn entry(&mut self, id: ChannelId) -> tiny_map::Entry<ChannelMapArray> {
    //     self.0.entry(id)
    // }
    // pub fn get_mut(&mut self, id: &ChannelId) -> Option<&mut Frame> {
    //     self.0.get_mut(id)
    // }
    pub fn id_map<F>(&self, mut f: F) -> Channels
    where
        F: FnMut(&ChannelId, &Frame) -> Frame,
    {
        self.0
            .iter()
            .map(|(id, value)| (*id, f(id, value)))
            .collect()
    }
    pub fn validate(self, instr_id: &InstrId, instruments: &Instruments) -> Self {
        self.into_iter()
            .map(|(mut ch_id, ch)| {
                match &mut ch_id {
                    ChannelId::Loop(validated, num) => {
                        let validator = instruments
                            .loop_validators
                            .get(num)
                            .expect("no validator for loop");
                        *validated = *validated || validator == instr_id;
                    }
                    ChannelId::Focus(validated, focus) => {
                        *validated = *validated
                            || instruments
                                .focused
                                .get(focus)
                                .map(|foc| foc == instr_id)
                                .unwrap_or(false)
                    }
                    ChannelId::Dummy | ChannelId::Control => {}
                }
                (ch_id, ch)
            })
            .collect()
    }
}

impl From<(ChannelId, Frame)> for Channels {
    fn from((id, channel): (ChannelId, Frame)) -> Self {
        Channels(once((id, channel)).collect())
    }
}

impl<T> From<Option<T>> for Channels
where
    T: Into<Channels>,
{
    fn from(channel: Option<T>) -> Self {
        channel.map(Into::into).unwrap_or_default()
    }
}

impl FromIterator<(ChannelId, Frame)> for Channels {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = (ChannelId, Frame)>,
    {
        Channels(TinyMap::from_iter(iter))
    }
}

impl Extend<(ChannelId, Frame)> for Channels {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (ChannelId, Frame)>,
    {
        self.0.extend(iter)
    }
}

impl IntoIterator for Channels {
    type Item = (ChannelId, Frame);
    type IntoIter = tiny_map::IntoIter<ChannelId, Frame>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Cache for each frame
#[derive(Default)]
pub struct FrameCache {
    pub map: HashMap<InstrId, Channels>,
    pub visited: HashSet<InstrId>,
    pub default_channels: Channels,
}
