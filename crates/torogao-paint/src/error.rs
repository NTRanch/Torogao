//! Typed refusals for the paint core.
//!
//! Every expected failure is one of these variants, returned
//! before any allocation or mutation the failure concerns
//! (validate, then prepare, then commit). Internal facts a
//! correct implementation has already established are
//! `assert!`s, never error returns.

use std::fmt;

/// The paint core's error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaintError {
    /// A named bound was exceeded: a canvas dimension, the
    /// layer count, the stamp count, a file size. The refusal
    /// names the bound, the value offered, and the limit.
    Limit {
        /// What was being bounded (e.g. `"canvas width"`).
        what: &'static str,
        /// The value that was refused.
        value: u64,
        /// The bound it exceeded.
        limit: u64,
    },
    /// An in-memory value is outside its domain (a non-finite
    /// sample coordinate, a spacing fraction outside (0, 1]).
    Invalid {
        /// What was wrong, in one bounded line.
        message: String,
    },
    /// An op would push the document's resident bytes past
    /// the memory cap. Refused at pre-flight, before any
    /// pixel is touched, so the document is unchanged.
    MemoryCap {
        /// The cap in force, in bytes.
        cap_bytes: u64,
        /// Resident bytes before the op.
        resident_bytes: u64,
        /// Additional resident bytes the op would need.
        additional_bytes: u64,
    },
    /// A referenced layer id does not exist in the document.
    LayerNotFound {
        /// The missing layer id.
        id: u32,
    },
    /// A document file or serialized value failed strict
    /// parsing or cross-validation.
    Format {
        /// What failed, in one bounded line.
        message: String,
    },
    /// The document's format major version is not one this
    /// build reads. Refused by name, never guessed at.
    UnsupportedVersion {
        /// The major version found in the header.
        found: u32,
    },
    /// The base raster's bytes do not hash to the SHA-256
    /// recorded in the header.
    BaseHashMismatch {
        /// The header's recorded hash (hex).
        expected: String,
        /// The hash of the bytes actually read (hex).
        actual: String,
    },
    /// A filesystem operation failed.
    Io {
        /// The operation (`"read"`, `"write"`, `"create dir"`).
        op: &'static str,
        /// The path involved.
        path: String,
        /// The OS error message.
        message: String,
    },
    /// PNG encode or decode failed.
    Png {
        /// The PNG layer's message.
        message: String,
    },
}

impl PaintError {
    /// A `Limit` refusal for `what` at `value` over `limit`.
    pub fn limit(what: &'static str, value: u64, limit: u64) -> PaintError {
        PaintError::Limit { what, value, limit }
    }

    /// An `Invalid` refusal with a bounded message.
    pub fn invalid(message: impl Into<String>) -> PaintError {
        PaintError::Invalid {
            message: message.into(),
        }
    }

    /// A `Format` refusal with a bounded message.
    pub fn format(message: impl Into<String>) -> PaintError {
        PaintError::Format {
            message: message.into(),
        }
    }

    /// An `Io` refusal for `op` on `path` from a std error.
    pub fn io(op: &'static str, path: &std::path::Path, err: &std::io::Error) -> PaintError {
        PaintError::Io {
            op,
            path: path.display().to_string(),
            message: err.to_string(),
        }
    }
}

impl fmt::Display for PaintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaintError::Limit { what, value, limit } => {
                write!(f, "{what} {value} exceeds the limit {limit}")
            }
            PaintError::Invalid { message } => write!(f, "invalid value: {message}"),
            PaintError::MemoryCap {
                cap_bytes,
                resident_bytes,
                additional_bytes,
            } => write!(
                f,
                "memory cap refused: resident {resident_bytes} + additional \
                 {additional_bytes} exceeds the cap {cap_bytes} bytes"
            ),
            PaintError::LayerNotFound { id } => write!(f, "layer {id} does not exist"),
            PaintError::Format { message } => write!(f, "format refusal: {message}"),
            PaintError::UnsupportedVersion { found } => {
                write!(f, "unsupported torogao-paint major version {found}")
            }
            PaintError::BaseHashMismatch { expected, actual } => write!(
                f,
                "base raster hash mismatch: header records {expected}, bytes hash to {actual}"
            ),
            PaintError::Io { op, path, message } => {
                write!(f, "io {op} failed for {path}: {message}")
            }
            PaintError::Png { message } => write!(f, "png failure: {message}"),
        }
    }
}

impl std::error::Error for PaintError {}

/// The paint core's result type.
pub type PaintResult<T> = Result<T, PaintError>;
