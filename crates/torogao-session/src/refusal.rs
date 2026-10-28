//! Typed refusals for the session surface.
//!
//! Codes are the interop design's taxonomy: `TG-SES-*` for the
//! session surface itself, `TG-AUTH-*` passed through unchanged
//! from the authoring taxonomy (`torogao-author`).

use std::fmt;

/// A typed refusal. Codes are the DESIGN §1.4 taxonomy: `TG-SES-*` for the
/// session surface, `TG-AUTH-*` passed through from the authoring taxonomy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: String,
    pub message: String,
}

impl Refusal {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Refusal {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Refusal {}

pub type R<T> = Result<T, Refusal>;
