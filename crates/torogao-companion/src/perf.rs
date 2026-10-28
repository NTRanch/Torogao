//! The performance instruments the Stage 1.1 gates read:
//! the evaluated-parameter trace of a fixed Daydream run
//! through the companion core, its determinism hash, the
//! loop-seam ratio (the Phase D `measure_motion.py` law,
//! re-implemented over the same parameter domain), and the
//! motion-on-every-frame fraction.
//!
//! The trace hash is a chain: FNV-1a over the per-step
//! evaluated state hashes in order, so two runs agree only
//! if every step agreed — a stronger statement than
//! comparing final hashes, and the one Gate G6 makes.

use std::path::Path;

use crate::core::{AMBIENT_ANIMATION, CompanionCore};
use crate::error::CompanionResult;

/// FNV-1a offset basis (64-bit), the estate's chain hash.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime (64-bit).
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// One fixed trace run through the companion core.
#[derive(Debug, Clone)]
pub struct TraceRun {
    /// Per-step evaluated state hashes, in order.
    pub step_hashes: Vec<u64>,
    /// Per-step parameter vectors `(x, y)` in payload
    /// order (the measure_motion domain).
    pub params: Vec<Vec<[f32; 2]>>,
    /// The chain hash over `step_hashes`.
    pub trace_hash: u64,
    /// How many of the steps changed the evaluated state
    /// versus the previous step.
    pub moving_steps: usize,
    /// Step indices at which the playhead wrapped (the
    /// loop seam steps): the step whose delta closes the
    /// loop.
    pub seam_steps: Vec<usize>,
}

/// One chain step: fold one step hash into the running
/// chain state. `chain_hash` over a slice is exactly the
/// fold of this over the slice from the offset basis, so
/// the live presentation (which cannot keep every hash)
/// and the instrumented trace (which can) produce the
/// same number for the same performance.
pub fn chain_step(state: u64, hash: u64) -> u64 {
    let mut state = state;
    for byte in hash.to_le_bytes() {
        state ^= u64::from(byte);
        state = state.wrapping_mul(FNV_PRIME);
    }
    state
}

/// Chains per-step hashes into one trace hash.
pub fn chain_hash(step_hashes: &[u64]) -> u64 {
    step_hashes
        .iter()
        .fold(FNV_OFFSET_BASIS, |state, &hash| chain_step(state, hash))
}

/// Runs `frames` fixed steps of the ambient performance
/// through a fresh companion core (no session server: the
/// trace is the presented performance's own evaluation,
/// and serving must not perturb it).
pub fn run_trace(model_path: &Path, frames: usize) -> CompanionResult<TraceRun> {
    let mut core = CompanionCore::load(model_path, None, None, "torogao-companion trace")?;
    let mut step_hashes = Vec::with_capacity(frames);
    let mut params: Vec<Vec<[f32; 2]>> = Vec::with_capacity(frames);
    let mut seam_steps = Vec::new();
    let mut previous_hframe: Option<f32> = None;
    let mut moving_steps = 0_usize;
    let mut previous_hash: Option<u64> = None;
    for _ in 0..frames {
        let hash = core.tick()?;
        if let Some(previous) = previous_hash {
            if previous != hash {
                moving_steps += 1;
            }
        }
        previous_hash = Some(hash);
        step_hashes.push(hash);
        let scene = core.scene()?;
        params.push(
            scene
                .puppet
                .params
                .iter()
                .map(|param| [param.last_internal.x, param.last_internal.y])
                .collect(),
        );
        let snapshot = core.play_snapshot()?;
        if snapshot.animation.as_deref() == Some(AMBIENT_ANIMATION) {
            if let Some(previous) = previous_hframe {
                if snapshot.hframe < previous - 0.5 {
                    // The playhead wrapped: this step's
                    // delta (params[len-1] vs params[len-2])
                    // is a seam delta.
                    seam_steps.push(step_hashes.len() - 1);
                }
            }
            previous_hframe = Some(snapshot.hframe);
        }
    }
    Ok(TraceRun {
        trace_hash: chain_hash(&step_hashes),
        step_hashes,
        params,
        moving_steps,
        seam_steps,
    })
}

/// The mean absolute per-step parameter delta between two
/// parameter vectors (the MAD of measure_motion.py: mean
/// over parameters of the L1 delta across the [x, y] pair).
pub fn param_delta(a: &[[f32; 2]], b: &[[f32; 2]]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let sum: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| f64::from((x[0] - y[0]).abs() + (x[1] - y[1]).abs()))
        .sum();
    sum / a.len() as f64
}

/// The loop-seam report over one trace, measure_motion's
/// law: the seam ratio is the seam step's delta over the
/// median per-step delta (1.0 = the seam is a typical step;
/// Gate G5's bound is 1.1).
#[derive(Debug, Clone)]
pub struct SeamReport {
    /// Median per-step delta.
    pub median_delta: f64,
    /// The seam deltas, one per wrap.
    pub seam_deltas: Vec<f64>,
    /// Worst seam ratio (max seam delta / median).
    pub seam_ratio_max: f64,
}

/// Computes the seam report for a trace.
pub fn seam_report(trace: &TraceRun) -> SeamReport {
    let mut deltas: Vec<f64> = Vec::with_capacity(trace.params.len().saturating_sub(1));
    for pair in trace.params.windows(2) {
        deltas.push(param_delta(&pair[0], &pair[1]));
    }
    let median_delta = median(&mut deltas.clone());
    let seam_deltas: Vec<f64> = trace
        .seam_steps
        .iter()
        .filter(|&&step| step >= 1 && step < trace.params.len())
        .map(|&step| param_delta(&trace.params[step - 1], &trace.params[step]))
        .collect();
    let seam_ratio_max = if median_delta > 0.0 {
        seam_deltas.iter().copied().fold(0.0_f64, f64::max) / median_delta
    } else {
        0.0
    };
    SeamReport {
        median_delta,
        seam_deltas,
        seam_ratio_max,
    }
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_hash_is_order_sensitive_and_deterministic() {
        let a = chain_hash(&[1, 2, 3]);
        assert_eq!(a, chain_hash(&[1, 2, 3]));
        assert_ne!(a, chain_hash(&[3, 2, 1]));
        assert_eq!(chain_hash(&[]), FNV_OFFSET_BASIS);
    }

    #[test]
    fn param_delta_is_l1_mean() {
        let a = vec![[0.0, 0.0], [2.0, 0.0]];
        let b = vec![[1.0, 0.0], [2.0, 3.0]];
        assert!((param_delta(&a, &b) - 2.0).abs() < 1e-12);
        assert_eq!(param_delta(&a, &a), 0.0);
    }

    #[test]
    fn median_handles_even_and_odd_counts() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(median(&mut []), 0.0);
    }
}
