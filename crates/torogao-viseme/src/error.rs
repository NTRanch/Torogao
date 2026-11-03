//! The crate's one typed error: a stable `TV-` code plus a
//! human message, the shape every Torogao registry surface
//! reports. Errors are values, never panics, on every path
//! that touches untrusted input (cue files and mapping data
//! are untrusted input).

use std::fmt;

/// A typed failure: stable code, human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisemeError {
    code: String,
    message: String,
}

impl VisemeError {
    /// Builds an error from its stable code and message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> VisemeError {
        VisemeError {
            code: code.into(),
            message: message.into(),
        }
    }

    /// The stable machine-readable code (for example
    /// `TV-CUE-UNMAPPED-SHAPE`).
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The human-readable explanation.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for VisemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for VisemeError {}

/// The result type every fallible viseme operation returns.
pub type VisemeResult<T> = Result<T, VisemeError>;

#[cfg(test)]
mod tests {
    use super::VisemeError;

    #[test]
    fn error_display_is_code_then_message() {
        let err = VisemeError::new("TV-CUE-EMPTY", "no cues");
        assert_eq!(err.code(), "TV-CUE-EMPTY");
        assert_eq!(err.message(), "no cues");
        assert_eq!(err.to_string(), "TV-CUE-EMPTY: no cues");
    }
}
