//! The port's one typed error: a stable `TG-` code plus a
//! human message, the shape every registry surface reports
//! (the D registry's fail results carry exactly these two
//! fields). Errors are values, never panics, on every path
//! that touches untrusted input.

use std::fmt;

/// A typed failure: stable code, human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorogaoError {
    code: String,
    message: String,
}

impl TorogaoError {
    /// Builds an error from its stable code and message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> TorogaoError {
        TorogaoError {
            code: code.into(),
            message: message.into(),
        }
    }

    /// The stable machine-readable code (for example
    /// `TG-CONTAINER-MALFORMED`).
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The human-readable explanation.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TorogaoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for TorogaoError {}

/// The result type every fallible format operation returns.
pub type FormatResult<T> = Result<T, TorogaoError>;

#[cfg(test)]
mod tests {
    use super::TorogaoError;

    #[test]
    fn error_carries_code_and_message() {
        let err = TorogaoError::new("TG-TEST", "a message");
        assert_eq!(err.code(), "TG-TEST");
        assert_eq!(err.message(), "a message");
        assert_eq!(err.to_string(), "TG-TEST: a message");
    }
}
