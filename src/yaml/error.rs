//! Error types for YAML operations.

use crate::tag::TagError;
use fyaml::error::Error as FyError;
use std::io;
use thiserror::Error;

/// Error type for YAML operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Error from fyaml library
    #[error("{0}")]
    Fy(#[from] FyError),

    /// I/O error
    #[error("{0}")]
    Io(String),

    /// Path navigation error
    #[error("{0}")]
    Path(String),

    /// Type mismatch error
    #[error("{0}")]
    Type(String),

    /// Generic error
    #[error("{0}")]
    Base(String),
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
