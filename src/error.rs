use std::{fmt, io};

use structopt::clap;

use crate::MidiError;

macro_rules! ryvm_error {
    ($(#[$attr:meta] $variant:ident($type:ty),)* also $(#[$attr2:meta] $monovariant:ident($message:literal)),* $(,)?) => {
        /// The Ryvm error type
        #[derive(Debug)]
        pub enum RyvmError {
            $(#[$attr] $variant($type),)*
            $(#[$attr2] $monovariant),*
        }

        impl fmt::Display for RyvmError {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                match self {
                    $(RyvmError::$variant(e) => write!(f, "{}", e),)*
                    $(RyvmError::$monovariant => write!(f, $message)),*
                }
            }
        }

        $(impl From<$type> for RyvmError {
            fn from(e: $type) -> Self {
                RyvmError::$variant(e)
            }
        })*
    };
}

ryvm_error!(
    /// An IO error
    IO(io::Error),
    /// A Midi error
    Midi(MidiError),
    /// A deserialization error
    Deserialization(ron::de::Error),
    /// A command line error
    CLI(clap::Error),
    /// An error with the file watcher
    Notify(notify::Error),

    also

    /// No available midi ports
    NoMidiPorts("There are no available midi ports")
);

/// The Ryvm result type
pub type RyvmResult<T> = Result<T, RyvmError>;
