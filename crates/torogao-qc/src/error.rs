//! The checker's one typed error: a stable `QC-E-*` code plus
//! a human message. Hard failures are values, never panics, on
//! every path that touches untrusted input. Finding codes
//! (`QC_*`, contract section 1) are a separate namespace: a
//! finding is a verdict about the spec, an error is a failure
//! to evaluate it at all.

use std::fmt;

/// A typed failure: stable code, human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QcError {
    code: String,
    message: String,
}

impl QcError {
    /// Builds an error from its stable code and message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> QcError {
        QcError {
            code: code.into(),
            message: message.into(),
        }
    }

    /// The stable machine-readable code (for example
    /// `QC-E-PATH-ESCAPE`).
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The human-readable message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for QcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for QcError {}
