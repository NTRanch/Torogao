//! `normalize_reference_embed`: the named compile step that
//! records measured per-reference embed statistics (DESIGN
//! §2.2).
//!
//! Embed magnitude is a property of an encoding run — a vision
//! encoder applied to a specific reference image. Compilation
//! cannot measure it; the step consumes measurements taken by
//! the executing harness and refuses stale or missing evidence
//! rather than assuming it. The statistics it records are
//! bookkeeping for THIS reference under THIS encoder, never a
//! claim that one normalisation suits all references: reference
//! strength is per-reference (the sweep's lesson), and this
//! step exists so the numbers exist at all — the estate's
//! Phase 0 and sweep evidence recorded no embed magnitudes.

use crate::error::GencondResult;
use crate::error::{CODE_MALFORMED, CODE_STALE_MEASUREMENT, CODE_UNMEASURED_REFERENCE, refuse};
use crate::schema::{EmbedScaling, ReferenceEmbed};
use serde::{Deserialize, Serialize};

/// One reference's embed-magnitude measurement, taken by the
/// executing harness from the vision encoding of the recorded
/// reference image.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedMeasurement {
    /// SHA-256 of the reference image measured; must equal the
    /// source's recorded reference hash.
    pub reference_sha256: String,
    /// The vision encoder the measurement was taken under;
    /// must equal the source's recorded encoder.
    pub vision_encoder: String,
    /// Image tokens in the encoding.
    pub token_count: u32,
    /// Mean embed-vector magnitude.
    pub magnitude_mean: f64,
    /// Maximum embed-vector magnitude.
    pub magnitude_max: f64,
}

/// What the step records into the compile report (and, through
/// it, provenance): the scaling mode applied, the measured
/// statistics, and whether the run sits outside the observed
/// evidence regime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalisationRecord {
    /// The source this record belongs to.
    pub source_id: String,
    /// The scaling mode's oracle string.
    pub mode: String,
    /// The reference's recorded hash.
    pub reference_sha256: String,
    /// The encoder the statistics were measured under.
    pub vision_encoder: String,
    /// Measured token count.
    pub token_count: u32,
    /// Measured mean magnitude.
    pub magnitude_mean: f64,
    /// Measured maximum magnitude.
    pub magnitude_max: f64,
    /// True when the scaling mode is outside the estate's
    /// observed evidence regime (see below).
    pub hot_reference: bool,
}

/// Runs the normalisation step for one enabled reference-embed
/// source.
///
/// The evidence regime is the estate's observed production
/// configuration: `K+V w/ C penalty`, the scaling under which
/// the cured D-802 configurations and the IPA sweep ran clean.
/// Any other scaling mode compiles — the algebra admits it —
/// but the step flags the run `hot-reference`: outside the
/// measured regime, magnitude behaviour is unobserved, and the
/// flag says so in the run's own provenance. The flag is a
/// regime marker, not a numeric threshold: no embed magnitudes
/// exist in the estate's evidence to set one from, and
/// inventing a band would claim a universality the design
/// forbids (DESIGN §2.2).
///
/// Returns the record and, when flagged, the flag string for
/// the compile report.
pub fn normalize_reference_embed(
    source_id: &str,
    embed: &ReferenceEmbed,
    measurements: &[EmbedMeasurement],
) -> GencondResult<(NormalisationRecord, Option<String>)> {
    let measurement = measurements
        .iter()
        .find(|m| m.reference_sha256 == embed.reference.sha256)
        .ok_or_else(|| {
            crate::error::GencondError::new(
                CODE_UNMEASURED_REFERENCE,
                format!(
                    "source '{source_id}' has no embed measurement for its reference; \
                     normalisation is a measured step, never an assumption"
                ),
            )
        })?;
    if measurement.vision_encoder != embed.vision_encoder {
        return refuse(
            CODE_STALE_MEASUREMENT,
            format!(
                "source '{source_id}' measurement was taken under encoder '{}' but the \
                 record declares '{}'",
                measurement.vision_encoder, embed.vision_encoder
            ),
        );
    }
    if measurement.token_count == 0
        || !measurement.magnitude_mean.is_finite()
        || !measurement.magnitude_max.is_finite()
        || measurement.magnitude_mean < 0.0
        || measurement.magnitude_max < measurement.magnitude_mean
    {
        return refuse(
            CODE_MALFORMED,
            format!("source '{source_id}' measurement statistics are incoherent"),
        );
    }
    let scaling = embed.normalisation.scaling();
    let hot = scaling != EmbedScaling::KeyValueCPenalty;
    let record = NormalisationRecord {
        source_id: source_id.to_string(),
        mode: scaling.oracle_str().to_string(),
        reference_sha256: embed.reference.sha256.clone(),
        vision_encoder: embed.vision_encoder.clone(),
        token_count: measurement.token_count,
        magnitude_mean: measurement.magnitude_mean,
        magnitude_max: measurement.magnitude_max,
        hot_reference: hot,
    };
    let flag = hot.then(|| format!("hot-reference:{source_id}"));
    Ok((record, flag))
}
