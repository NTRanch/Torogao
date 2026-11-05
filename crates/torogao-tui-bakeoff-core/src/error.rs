//! Typed failures for the bake-off workload core.

use std::fmt;
use std::path::PathBuf;

/// A workload result.
pub type Result<T> = std::result::Result<T, BakeoffError>;

/// A bounded, non-secret-bearing workload failure.
#[derive(Debug)]
pub enum BakeoffError {
    /// An input/output operation failed at a named path.
    Io {
        /// The path involved in the failed operation.
        path: PathBuf,
        /// The operating-system error.
        source: std::io::Error,
    },
    /// JSON input or output did not fit the expected shape.
    Json(serde_json::Error),
    /// The Torogao authoring dispatcher refused or could not load data.
    Author(String),
    /// An MCP or A2A protocol operation failed.
    Protocol(String),
    /// A loopback network operation failed.
    Network(String),
    /// A cryptographic operation failed its stated contract.
    Crypto(String),
}

impl fmt::Display for BakeoffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "I/O at '{}': {source}", path.display())
            }
            Self::Json(error) => write!(formatter, "JSON failure: {error}"),
            Self::Author(message) => write!(formatter, "authoring failure: {message}"),
            Self::Protocol(message) => write!(formatter, "protocol failure: {message}"),
            Self::Network(message) => write!(formatter, "network failure: {message}"),
            Self::Crypto(message) => write!(formatter, "cryptography failure: {message}"),
        }
    }
}

impl std::error::Error for BakeoffError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for BakeoffError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
