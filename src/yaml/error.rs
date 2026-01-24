//! Error types for YAML operations.

use crate::tag::TagError;
use fyaml::error::Error as FyError;
use std::io;

/// Error type for YAML operations.
#[derive(Debug)]
pub enum Error {
    /// Error from fyaml library
    Fy(FyError),
    /// I/O error
    Io(String),
    /// Path navigation error
    Path(String),
    /// Type mismatch error
    Type(String),
    /// Generic error
    Base(String),
}

impl std::error::Error for Error {}

impl From<FyError> for Error {
    fn from(e: FyError) -> Self {
        Error::Fy(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Error::Base(e)
    }
}

impl From<&str> for Error {
    fn from(e: &str) -> Self {
        Error::Base(e.to_string())
    }
}

impl From<TagError> for Error {
    fn from(e: TagError) -> Self {
        Error::Base(e.to_string())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Fy(e) => write!(f, "{}", e),
            Error::Io(e) => write!(f, "{}", e),
            Error::Path(e) => write!(f, "{}", e),
            Error::Type(e) => write!(f, "{}", e),
            Error::Base(e) => write!(f, "{}", e),
        }
    }
}
