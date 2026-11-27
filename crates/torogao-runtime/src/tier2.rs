//! Tier 2 measurement: the presentation digest and the
//! cross-architecture tolerance band (plan SS2.1, landed as a
//! gate in Rung R2).
//!
//! Tier 2 state — the derived presentation view — is float-
//! bearing and is never an identity. This module is the
//! measurement instrument the twin gate uses to keep that
//! honest:
//!
//! - [`presentation_digest`] hashes one [`PresentationState`]
//!   over the exact bit patterns of its gauge fractions (plus
//!   every integer and string field). Two runs **on one
//!   architecture** must produce identical digests — that is
//!   the within-arch byte-identity check, the D-1397 twin
//!   criterion applied to presentation.
//! - [`compare_presentations`] walks two runs tick by tick and
//!   measures the numeric distance between their gauges: the
//!   largest absolute and relative differences observed, and
//!   how many tick digests differed at all. **Across
//!   architectures** the digests may legitimately differ in
//!   last bits; the gate compares the measured distances
//!   against [`RECORDED_BAND`] instead.
//!
//! This module is Tier 2 code, like [`crate::present`]: it
//! reads floats, it never feeds a sim, and the Tier 1 lint
//! ([`crate::tier1`]) exempts it from the float rules only —
//! the ordering/time/thread/platform rules bind it too.

use crate::hash::Hash;
use crate::present::PresentationState;
use sha2::{Digest, Sha256};

/// A tolerance band for cross-architecture Tier 2 comparison:
/// every compared gauge must sit within `max_abs` absolute
/// difference, or within `max_rel` relative difference, of its
/// counterpart (either leg passing is enough — the standard
/// combined tolerance for values that range from 0 to 1 and
/// beyond).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    /// Largest permitted absolute difference.
    pub max_abs: f64,
    /// Largest permitted relative difference (against the
    /// larger magnitude of the pair).
    pub max_rel: f64,
}

/// The band of record for the runtime's Tier 2 gate (Rung R2).
///
/// First measurement, 2026-11-26: across Linux x86_64 native,
/// Linux aarch64 (qemu-user), and the Android-target build
/// (qemu-user), both witness corpora's gauges — integer ratios
/// evaluated in f64, the only float shape the runtime's own
/// witnesses produce — measured a maximum absolute difference
/// of exactly 0.0. The recorded band is therefore set at the
/// floor this constant names, not at the measurement: engine
/// and rig evaluation (transcendentals, fused multiply-add)
/// joins Tier 2 in later rungs and will need real headroom,
/// and a band of 0 would make the gate a second identity
/// check in disguise. The band is tightened or loosened only
/// by evidence, recorded in the gate record (plan SS4.1).
pub const RECORDED_BAND: Band = Band {
    max_abs: 1e-6,
    max_rel: 1e-6,
};

/// The measured distance between two presentation runs.
#[derive(Clone, Debug, PartialEq)]
pub struct BandReport {
    /// How many gauge values were compared.
    pub gauges_compared: usize,
    /// How many ticks' presentation digests differed at all
    /// (the within-arch failure count; informational across
    /// architectures, where last-bit digest differences are
    /// the expected shape of a passing band comparison).
    pub digest_mismatches: usize,
    /// The largest absolute gauge difference observed.
    pub max_abs_diff: f64,
    /// The largest relative gauge difference observed.
    pub max_rel_diff: f64,
}

impl BandReport {
    /// Whether every compared gauge sits inside `band`.
    #[must_use]
    #[allow(clippy::float_arithmetic)]
    pub fn within(&self, band: Band) -> bool {
        self.max_abs_diff <= band.max_abs || self.max_rel_diff <= band.max_rel
    }
}

fn push_text(hasher: &mut Sha256, text: &str) {
    hasher.update((text.len() as u64).to_be_bytes());
    hasher.update(text.as_bytes());
}

/// The Tier 2 digest of one presentation state: SHA-256 over a
/// canonical byte encoding of every field, with gauge
/// fractions hashed as their exact bit patterns. A digest is
/// a per-architecture diagnostic, never an identity (plan
/// SS2.1): equal digests across architectures are welcome but
/// not required — the band is the cross-arch instrument.
#[must_use]
pub fn presentation_digest(state: &PresentationState) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(state.tick.to_be_bytes());
    push_text(&mut hasher, &state.headline);
    match &state.detail {
        Some(detail) => {
            hasher.update([1u8]);
            push_text(&mut hasher, detail);
        }
        None => hasher.update([0u8]),
    }
    hasher.update((state.meters.len() as u64).to_be_bytes());
    for meter in &state.meters {
        push_text(&mut hasher, &meter.id);
        hasher.update(meter.value.to_be_bytes());
    }
    hasher.update((state.gauges.len() as u64).to_be_bytes());
    for gauge in &state.gauges {
        push_text(&mut hasher, &gauge.id);
        hasher.update(gauge.fraction.to_bits().to_be_bytes());
    }
    let digest = hasher.finalize();
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    Hash(text)
}

/// Compares two presentation runs (one state per tick, in
/// tick order) and measures their distance. Gauges are matched
/// by position within their tick; a tick whose gauge ids or
/// counts differ counts as a digest mismatch and its gauges
/// are not compared numerically (the shapes diverged, which
/// the band is not the instrument for).
#[must_use]
#[allow(clippy::float_arithmetic)]
pub fn compare_presentations(a: &[PresentationState], b: &[PresentationState]) -> BandReport {
    let mut report = BandReport {
        gauges_compared: 0,
        digest_mismatches: 0,
        max_abs_diff: 0.0,
        max_rel_diff: 0.0,
    };
    for (state_a, state_b) in a.iter().zip(b.iter()) {
        if presentation_digest(state_a) != presentation_digest(state_b) {
            report.digest_mismatches += 1;
        }
        let same_shape = state_a.gauges.len() == state_b.gauges.len()
            && state_a
                .gauges
                .iter()
                .zip(state_b.gauges.iter())
                .all(|(ga, gb)| ga.id == gb.id);
        if !same_shape {
            continue;
        }
        for (ga, gb) in state_a.gauges.iter().zip(state_b.gauges.iter()) {
            let abs_diff = (ga.fraction - gb.fraction).abs();
            let scale = ga.fraction.abs().max(gb.fraction.abs());
            let rel_diff = if scale > 0.0 { abs_diff / scale } else { 0.0 };
            report.gauges_compared += 1;
            if abs_diff > report.max_abs_diff {
                report.max_abs_diff = abs_diff;
            }
            if rel_diff > report.max_rel_diff {
                report.max_rel_diff = rel_diff;
            }
        }
    }
    report
}
