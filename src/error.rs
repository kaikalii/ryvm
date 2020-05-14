use std::io;

use structopt::clap;
use thiserror::Error;

use crate::{input::InputError, midi::MidiError, ty::Name};

/// The Ryvm error type
#[derive(Debug, Error)]
pub enum RyvmError {
    /// An IO error
    #[error("IO error: {0}")]
    IO(#[from] io::Error),
    /// A Midi error
    #[error("Midi error: {0}")]
    Midi(#[from] MidiError),
    /// A serialization error
    #[error("Fatal serilization error: {0}")]
    Serialization(#[from] toml::ser::Error),
    /// A deserialization error
    #[error("Syntax error: {0}")]
    Deserialization(#[from] toml::de::Error),
    /// A command line error
    #[error("{0}")]
    CLI(#[from] clap::Error),
    /// An error with the file watcher
    #[error("Watcher error: {0}")]
    Notify(#[from] notify::Error),
    /// An error deocoding an audio file
    #[error("Audio decoder error: {0}")]
    Decode(#[from] rodio::decoder::DecoderError),
    /// An error encoding/decoding a loop
    #[error("Loop decoder error: {0}")]
    Loop(#[from] serde_cbor::Error),
    /// No available midi ports
    #[error("There are no available midi ports for {0:?}")]
    NoMidiPorts(Name),
    /// The Ryvm state was dropped
    #[error("Attempted to send a command to a dropped ryvm state")]
    StateDropped,
    /// An error with audio input
    #[error("Input error: {0}")]
    InputNode(#[from] InputError),
    /// A node that requires input was not assigned it
    #[error(
        "No input specified for {0}. It must either have the \
        'input' field specified or be listed after another node."
    )]
    NoInputSpecified(Name),
    /// No node matching search
    #[error("No node found matching {0}")]
    NoMatchingNode(String),
    /// No default output node
    #[error("No default output node available")]
    NoDefaultOutputNode,
    /// Unable to initialize audio node
    #[error("Unable to initialize audio node")]
    UnableToInitializeNode,
    /// No default input node
    #[error("Tried to create an input-pass, but there is no audio input specified")]
    NoAudioInputForPass,
}

/// The Ryvm result type
pub type RyvmResult<T> = Result<T, RyvmError>;
