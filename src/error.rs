use std::io;

use structopt::clap;
use thiserror::Error;

use crate::MidiError;

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
    Serialization(#[from] ron::ser::Error),
    /// A deserialization error
    #[error("Syntax error: {0}")]
    Deserialization(#[from] ron::de::Error),
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
    #[error("There are no available midi ports")]
    NoMidiPorts,
    /// The Ryvm state was dropped
    #[error("Attempted to send a command to a dropped ryvm state")]
    StateDropped,
}

/// The Ryvm result type
pub type RyvmResult<T> = Result<T, RyvmError>;
