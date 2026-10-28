//! The companion's typed refusal vocabulary. Every failure
//! the binary can hit is one of these codes plus a message —
//! a missing layer-shell global, a corrupt state file, an
//! unknown output name — never a panic and never a silent
//! fallback to a different window class. The one deliberate
//! softening is the persisted-state path: a corrupt state
//! file refuses typed *and* the companion starts at default
//! placement (Gate G7), because a desktop pet that will not
//! start over a bad JSON file is worse than one that says
//! what happened and comes up where it used to be expected.

use std::fmt;

/// One companion failure: a stable code plus a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanionError {
    /// Stable refusal code (`TG-COMP-*`).
    pub code: &'static str,
    /// Human-readable detail.
    pub message: String,
}

impl CompanionError {
    /// Builds one refusal.
    pub fn new(code: &'static str, message: impl Into<String>) -> CompanionError {
        CompanionError {
            code,
            message: message.into(),
        }
    }

    /// A command-line refusal.
    pub fn cli(message: impl Into<String>) -> CompanionError {
        CompanionError::new("TG-COMP-CLI", message)
    }

    /// A model load / evaluation refusal.
    pub fn model(message: impl Into<String>) -> CompanionError {
        CompanionError::new("TG-COMP-MODEL", message)
    }
}

impl fmt::Display for CompanionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code, self.message)
    }
}

impl std::error::Error for CompanionError {}

/// The companion's result type.
pub type CompanionResult<T> = Result<T, CompanionError>;

#[cfg(test)]
mod tests {
    use super::CompanionError;

    #[test]
    fn refusal_carries_code_and_message() {
        let err = CompanionError::new("TG-COMP-TEST", "a named failure");
        assert_eq!(err.code, "TG-COMP-TEST");
        assert_eq!(err.to_string(), "TG-COMP-TEST a named failure");
    }
}
