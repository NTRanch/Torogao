//! The generation base plan: the non-conditioning half of a
//! generation record (checkpoint, latent, sampler, seed). The
//! author layer owns this data in `project.ron`; this crate
//! owns the plan's validation rules and its graph role, so the
//! compiler is the single place where a plan becomes nodes.

use crate::error::GencondResult;
use crate::error::{CODE_RANGE, refuse};
use crate::schema::check_model_name;
use serde::{Deserialize, Serialize};

/// Latent dimensions are bounded by the sampler's canvas
/// class; the bound is a sanity ceiling, not a tuning claim.
const LATENT_DIM_MAX: u32 = 16_384;
/// Batch size bound.
const BATCH_MAX: u32 = 64;
/// Sampling-steps bound.
const STEPS_MAX: u32 = 1_000;
/// CFG bound (observed band is 1–10).
const CFG_MAX: f64 = 100.0;

/// The base plan a `Conditioning` record conditions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationPlan {
    /// Record id (the author layer's id rules apply).
    pub id: String,
    /// S-7 name of the base checkpoint.
    pub checkpoint: String,
    /// The latent canvas.
    pub latent: LatentSpec,
    /// The sampler configuration.
    pub sampler: SamplerSpec,
    /// The base seed; sweep offsets are the sweep verb's
    /// business (Stage 2), never the record's.
    pub seed: i64,
    /// Save-image filename prefix.
    pub filename_prefix: String,
}

/// The latent canvas (EmptyLatentImage inputs).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LatentSpec {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Batch size.
    pub batch_size: u32,
}

/// The sampler configuration (KSampler inputs).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamplerSpec {
    /// Steps.
    pub steps: u32,
    /// CFG scale.
    pub cfg: f64,
    /// Sampler name (oracle enumeration string).
    pub name: String,
    /// Scheduler name (oracle enumeration string).
    pub scheduler: String,
    /// Denoise strength, in [0, 1].
    pub denoise: f64,
}

impl GenerationPlan {
    /// Validates the plan's scalars and names.
    pub fn validate(&self) -> GencondResult<()> {
        check_model_name(&self.checkpoint)?;
        if self.latent.width == 0 || self.latent.width > LATENT_DIM_MAX {
            return refuse(CODE_RANGE, "latent width is outside [1, 16384]");
        }
        if self.latent.height == 0 || self.latent.height > LATENT_DIM_MAX {
            return refuse(CODE_RANGE, "latent height is outside [1, 16384]");
        }
        if self.latent.batch_size == 0 || self.latent.batch_size > BATCH_MAX {
            return refuse(CODE_RANGE, "batch size is outside [1, 64]");
        }
        if self.sampler.steps == 0 || self.sampler.steps > STEPS_MAX {
            return refuse(CODE_RANGE, "sampler steps are outside [1, 1000]");
        }
        if !self.sampler.cfg.is_finite() || !(0.0..=CFG_MAX).contains(&self.sampler.cfg) {
            return refuse(CODE_RANGE, "CFG must be finite and lie in [0, 100]");
        }
        if !self.sampler.denoise.is_finite() || !(0.0..=1.0).contains(&self.sampler.denoise) {
            return refuse(CODE_RANGE, "denoise must lie in [0, 1]");
        }
        if self.sampler.name.is_empty() || self.sampler.scheduler.is_empty() {
            return refuse(CODE_RANGE, "sampler and scheduler names must be non-empty");
        }
        if self.filename_prefix.is_empty()
            || self
                .filename_prefix
                .chars()
                .any(|ch| ch.is_control() || ch == '/' || ch == '\\')
        {
            return refuse(
                CODE_RANGE,
                "filename prefix must be non-empty and carry no path separators",
            );
        }
        Ok(())
    }
}

/// How one S-7 model name resolves for graph emission: the
/// install-relative file a loader names, the preset an
/// IP-Adapter unified loader takes, and the file's recorded
/// hash when the registry carries one (S-7/S-8 binding).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelBinding {
    /// The loader-visible file name (checkpoints, LoRAs,
    /// control models, adapters, vision encoders).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The IP-Adapter preset (adapter resolution only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// SHA-256 of the resolved file, when recorded (provenance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}
