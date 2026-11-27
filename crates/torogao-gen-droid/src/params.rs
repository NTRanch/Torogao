//! Request / response types and the seam's error vocabulary.

use std::fmt;

/// Hard cap on the composed positive prompt, in bytes.
///
/// The mobile core caps the *user's scene* at 2000 bytes and then
/// composes the full prompt (style prefix + cast identity +
/// signature + scene). This seam receives the composed prompt, so
/// its cap is set above the largest composition the core can
/// produce; anything longer is refused with
/// [`DroidError::PromptTooLong`] rather than truncated — the same
/// refuse-don't-mangle discipline the seat client uses.
pub const MAX_PROMPT_BYTES: usize = 4096;

/// One generation request, mirroring the fields the mobile core's
/// recipe of record fixes (checkpoint choice is the engine's load
/// parameter, not the request's).
#[derive(Debug, Clone, PartialEq)]
pub struct GenParams {
    /// Composed positive prompt.
    pub prompt: String,
    /// Negative prompt.
    pub negative_prompt: String,
    /// Seed; negative asks the backend for a fresh random seed.
    pub seed: i64,
    /// Sampling steps.
    pub steps: u32,
    /// Classifier-free guidance scale.
    pub cfg: f32,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Sampler name in the backend's vocabulary (e.g. `euler_a`,
    /// `euler`).
    pub sampler: String,
    /// Scheduler name in the backend's vocabulary (e.g. `normal`),
    /// when the backend distinguishes it.
    pub scheduler: Option<String>,
}

impl GenParams {
    /// The estate recipe of record for a composed prompt: 28 steps,
    /// CFG 6.0, euler_ancestral / normal, 832×1344.
    pub fn recipe(prompt: String, negative_prompt: String, seed: i64) -> Self {
        GenParams {
            prompt,
            negative_prompt,
            seed,
            steps: 28,
            cfg: 6.0,
            width: 832,
            height: 1344,
            sampler: "euler_a".to_owned(),
            scheduler: Some("normal".to_owned()),
        }
    }

    /// Validate the seam's bounds. Called on submit, before any
    /// backend work is queued.
    pub fn validate(&self) -> Result<(), DroidError> {
        if self.prompt.len() > MAX_PROMPT_BYTES {
            return Err(DroidError::PromptTooLong {
                bytes: self.prompt.len(),
                max: MAX_PROMPT_BYTES,
            });
        }
        if self.steps == 0 || self.steps > 200 {
            return Err(DroidError::InvalidParams(format!(
                "steps {} outside 1..=200",
                self.steps
            )));
        }
        if self.width == 0 || self.height == 0 || self.width % 8 != 0 || self.height % 8 != 0 {
            return Err(DroidError::InvalidParams(format!(
                "size {}x{} must be non-zero multiples of 8",
                self.width, self.height
            )));
        }
        Ok(())
    }
}

/// A decoded RGB image straight off the backend, before PNG encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major RGB, 3 bytes per pixel, `width * height * 3` long.
    pub rgb: Vec<u8>,
}

/// A finished still: PNG bytes plus dimensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// PNG-encoded image bytes.
    pub png: Vec<u8>,
}

/// The seam's errors. `EngineUnavailable` plays the role the seat
/// client's `SeatUnreachable` plays for the network engine: a typed,
/// first-class "this engine cannot serve right now", never a panic
/// and never a silent empty result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DroidError {
    /// The composed prompt exceeded [`MAX_PROMPT_BYTES`].
    PromptTooLong {
        /// Actual byte length.
        bytes: usize,
        /// The cap.
        max: usize,
    },
    /// The request parameters are outside the seam's bounds.
    InvalidParams(String),
    /// The engine is not loaded / the native library is absent /
    /// the model could not be loaded. Carries the backend's reason.
    EngineUnavailable(String),
    /// The backend ran and failed. Carries its status text.
    Backend(String),
    /// A poll/fetch named a job this engine does not know (never
    /// submitted, or already fetched).
    UnknownJob,
}

impl fmt::Display for DroidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DroidError::PromptTooLong { bytes, max } => {
                write!(f, "prompt is {bytes} bytes; the limit is {max}")
            }
            DroidError::InvalidParams(why) => write!(f, "invalid parameters: {why}"),
            DroidError::EngineUnavailable(why) => write!(f, "engine unavailable: {why}"),
            DroidError::Backend(why) => write!(f, "backend failed: {why}"),
            DroidError::UnknownJob => write!(f, "unknown job"),
        }
    }
}

impl std::error::Error for DroidError {}
