//! Typed errors for torogao-rig. Every failure carries a stable
//! `TRG-` code so gate scripts can match on the class of failure
//! instead of message text.

use std::fmt;

/// One rig failure: a stable code plus a bounded human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigError {
    /// Stable machine-readable code, e.g. `TRG-INP-001`.
    pub code: &'static str,
    /// Bounded context: what was being read and what was wrong.
    pub message: String,
}

impl RigError {
    /// Construct an error from a stable code and a message.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for RigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RigError {}
