//! The application error type: every expected failure of
//! the shell (file access, container decode, engine
//! evaluation, parameter input, GPU work, command-line
//! shape) is a typed value with a bounded, path-free detail
//! string. Internal invariants use assertions instead.

use std::fmt;

/// One expected failure of the application shell.
#[derive(Debug)]
pub enum AppError {
    /// The command line does not match the documented usage.
    Cli(String),
    /// The engine refused or failed an evaluation step.
    Engine(String),
    /// The format layer refused the model file.
    Format(String),
    /// A file could not be read or written.
    Io {
        /// What was being attempted ("read model", "write render").
        context: &'static str,
        /// The OS error text.
        message: String,
    },
    /// A parameter assignment was rejected (unknown name,
    /// unparsable or non-finite value).
    Param(String),
    /// GPU or texture work failed (including "no adapter").
    Render(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Cli(detail) => write!(f, "usage error: {detail}"),
            AppError::Engine(detail) => write!(f, "engine evaluation failed: {detail}"),
            AppError::Format(detail) => write!(f, "model refused: {detail}"),
            AppError::Io { context, message } => write!(f, "{context} failed: {message}"),
            AppError::Param(detail) => write!(f, "parameter rejected: {detail}"),
            AppError::Render(detail) => write!(f, "render failed: {detail}"),
        }
    }
}

impl std::error::Error for AppError {}

/// The application result type.
pub type AppResult<T> = Result<T, AppError>;
