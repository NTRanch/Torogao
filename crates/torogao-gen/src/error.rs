//! Typed errors for torogao-gen. One closed enum; every
//! variant carries a stable machine-readable code that run
//! records and callers switch on. No stringly errors cross
//! the crate boundary.

use std::fmt;

/// Errors the generation prototype can return.
#[derive(Debug)]
pub enum GenError {
    /// A candle tensor operation failed.
    Candle(candle::Error),
    /// A run parameter fell outside the limits module bounds.
    Limit {
        /// The parameter that violated its bound.
        field: &'static str,
        /// What was given versus what the bound allows.
        detail: String,
    },
    /// Weight files were missing, truncated, or failed the
    /// strict loader (missing tensor under the expected name).
    Weights {
        /// What failed, in operator terms.
        detail: String,
    },
    /// Tokenizer load or encode failed.
    Tokenizer {
        /// What failed, in operator terms.
        detail: String,
    },
    /// Image encode/write failed.
    Image {
        /// What failed, in operator terms.
        detail: String,
    },
    /// A Stage 2 conditioning input was refused before any
    /// GPU work: a missing reference or weights file, a
    /// pose map of the wrong size, or a uniform/blank pose
    /// map (a pose source that failed upstream, e.g. an
    /// estimator blind on its input, must never silently
    /// condition a run).
    Conditioning {
        /// What was refused, in operator terms.
        detail: String,
    },
    /// Plain I/O failure (paths, run records).
    Io(std::io::Error),
}

impl GenError {
    /// The stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Candle(_) => "gen_candle",
            Self::Limit { .. } => "gen_limit",
            Self::Weights { .. } => "gen_weights",
            Self::Tokenizer { .. } => "gen_tokenizer",
            Self::Image { .. } => "gen_image",
            Self::Conditioning { .. } => "gen_conditioning",
            Self::Io(_) => "gen_io",
        }
    }
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Candle(e) => write!(f, "gen_candle: {e}"),
            Self::Limit { field, detail } => write!(f, "gen_limit: {field}: {detail}"),
            Self::Weights { detail } => write!(f, "gen_weights: {detail}"),
            Self::Tokenizer { detail } => write!(f, "gen_tokenizer: {detail}"),
            Self::Image { detail } => write!(f, "gen_image: {detail}"),
            Self::Conditioning { detail } => write!(f, "gen_conditioning: {detail}"),
            Self::Io(e) => write!(f, "gen_io: {e}"),
        }
    }
}

impl std::error::Error for GenError {}

impl From<candle::Error> for GenError {
    fn from(e: candle::Error) -> Self {
        Self::Candle(e)
    }
}

impl From<std::io::Error> for GenError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// The crate result type.
pub type Result<T> = std::result::Result<T, GenError>;
