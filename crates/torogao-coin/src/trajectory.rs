//! The canonical cross-platform trajectory: one fixture,
//! run identically on every platform, and the FNV-1a digest
//! of its per-step record.
//!
//! The fixture is the run `coin-smoke` has always made —
//! preparation actions 13 and 8, then a triangle-wave swing
//! stream against the level-0 gauge — moved here from the
//! binary so the Linux smoke and the Android spike shell
//! execute *the same code*, not two transcriptions of it.
//!
//! The digest discipline is the estate's state-hash
//! discipline (`torogao-app/src/state_hash.rs`), re-implemented
//! here so this crate gains no dependency on the app crate:
//! FNV-1a 64-bit (offset basis `0xcbf29ce484222325`, prime
//! `0x100000001b3`) over the little-endian byte stream of
//! each record's words, in order. The record itself is
//! defined by `Session::run_attempt_traced`: after each
//! executed sample step, the accumulator's banked points
//! and the gauge they make, as f64 bit patterns. Equal
//! digests on two platforms mean the two runs banked the
//! same points at the same steps, bit for bit — the S7
//! cross-platform determinism gate (PIPELINE.md, D-212).

use crate::dynamics::{CoinError, Dynamics};
use crate::measured_dynamics;
use crate::session::{AttemptOutcome, Session};

/// FNV-1a offset basis (64-bit).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime (64-bit).
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Day-clock start of the fixture session, in minutes
/// (09:00, the measured day frame's morning).
pub const FIXTURE_DAY_START_MINUTE: u32 = 9 * 60;
/// The fixture's preparation actions: the two broadest
/// measured actions (indices 13 and 8 in the source switch
/// table), in the order `coin-smoke` has always applied
/// them.
pub const FIXTURE_PREP_ACTIONS: [usize; 2] = [13, 8];
/// Sample count of the fixture stream: 20 s at 60 steps
/// per second.
pub const FIXTURE_STEPS: usize = 20 * 60;
/// Step duration of the fixture attempt, in seconds.
pub const FIXTURE_DT_SECONDS: f64 = 1.0 / 60.0;

/// An incremental FNV-1a hasher over little-endian u64
/// words, byte at a time — the `state_hash` discipline.
struct TrajectoryHasher {
    state: u64,
}

impl TrajectoryHasher {
    fn new() -> TrajectoryHasher {
        TrajectoryHasher {
            state: FNV_OFFSET_BASIS,
        }
    }

    fn write_u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.state ^= u64::from(byte);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }

    fn finish(&self) -> u64 {
        self.state
    }
}

/// The fixture's sample stream: a triangle wave, amplitude
/// 10 world units, centred on the start point (x = 0 at
/// step 0), one sample per step — the construction
/// `coin-smoke` has always run, unchanged.
pub fn fixture_samples() -> Vec<f64> {
    let mut samples = Vec::with_capacity(FIXTURE_STEPS);
    for i in 0..FIXTURE_STEPS {
        let phase = ((i + 10) % 40) as f64;
        let x = if phase < 20.0 {
            phase - 10.0
        } else {
            30.0 - phase
        };
        samples.push(x);
    }
    samples
}

/// The result of one fixture run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixtureRun {
    /// The attempt's outcome, as `Session::run_attempt`
    /// reports it.
    pub outcome: AttemptOutcome,
    /// The session's day clock after preparation and the
    /// attempt, in minutes.
    pub clock_minute: u32,
    /// Whether the session may save after the run (day
    /// clock at or past the leaving minute).
    pub can_save: bool,
    /// FNV-1a digest of the run's per-step trajectory
    /// record.
    pub trajectory_hash: u64,
    /// Sample steps actually executed (the attempt breaks
    /// on success or timeout, so this can be far below
    /// `FIXTURE_STEPS`).
    pub steps_executed: usize,
}

/// Run the canonical fixture over `dynamics`: a fresh
/// session at the fixture day start, the fixture
/// preparation actions, then the fixture stream as one
/// traced attempt.
pub fn run_fixture(dynamics: Dynamics) -> Result<FixtureRun, CoinError> {
    let mut session = Session::new(dynamics, FIXTURE_DAY_START_MINUTE)?;
    for index in FIXTURE_PREP_ACTIONS {
        session.apply_prep_action(index)?;
    }
    let samples = fixture_samples();
    let mut trace: Vec<(u64, u64)> = Vec::with_capacity(samples.len());
    let outcome = session.run_attempt_traced(&samples, FIXTURE_DT_SECONDS, &mut trace)?;
    let mut hasher = TrajectoryHasher::new();
    for &(points_bits, gauge_bits) in &trace {
        hasher.write_u64(points_bits);
        hasher.write_u64(gauge_bits);
    }
    Ok(FixtureRun {
        outcome,
        clock_minute: session.clock_minute(),
        can_save: session.can_save(),
        trajectory_hash: hasher.finish(),
        steps_executed: trace.len(),
    })
}

/// The fixture over the measured dynamics table — the one
/// run both `coin-smoke` (Linux) and the Android spike
/// shell execute.
pub fn run_measured_fixture() -> Result<FixtureRun, CoinError> {
    run_fixture(measured_dynamics()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stream_hashes_to_the_offset_basis() {
        assert_eq!(TrajectoryHasher::new().finish(), FNV_OFFSET_BASIS);
    }

    #[test]
    fn fixture_matches_the_recorded_smoke_values() {
        // The values PIPELINE.md §5 records for coin-smoke:
        // the fixture must reproduce them exactly, or the
        // move from the binary changed the run.
        let run = run_measured_fixture().expect("measured fixture runs");
        assert!(run.outcome.success);
        assert_eq!(format!("{:.4}", run.outcome.gauge), "219.5479");
        assert_eq!(format!("{:.4}", run.outcome.points), "172.5000");
        assert_eq!(format!("{:.2}", run.outcome.elapsed_seconds), "0.37");
        assert_eq!(run.outcome.level_after, 1);
        assert_eq!(run.clock_minute, 595);
        assert!(!run.can_save);
    }

    #[test]
    fn fixture_hash_is_deterministic_across_runs() {
        let a = run_measured_fixture().expect("first run");
        let b = run_measured_fixture().expect("second run");
        assert_eq!(a.trajectory_hash, b.trajectory_hash);
        assert_eq!(a.steps_executed, b.steps_executed);
        // The first stroke banks on the pointer's first
        // return crossing, far inside the 20 s stream.
        assert!(a.steps_executed < FIXTURE_STEPS);
    }

    #[test]
    fn traced_and_untraced_attempts_agree() {
        // Adversarial to the trace itself: recording must
        // not perturb the run it records.
        let dynamics = measured_dynamics().expect("dynamics load");
        let mut plain = Session::new(dynamics.clone(), FIXTURE_DAY_START_MINUTE)
            .expect("plain session");
        let mut traced =
            Session::new(dynamics, FIXTURE_DAY_START_MINUTE).expect("traced session");
        for index in FIXTURE_PREP_ACTIONS {
            plain.apply_prep_action(index).expect("plain prep");
            traced.apply_prep_action(index).expect("traced prep");
        }
        let samples = fixture_samples();
        let plain_outcome = plain.run_attempt(&samples, FIXTURE_DT_SECONDS).expect("plain run");
        let mut trace = Vec::new();
        let traced_outcome = traced
            .run_attempt_traced(&samples, FIXTURE_DT_SECONDS, &mut trace)
            .expect("traced run");
        assert_eq!(plain_outcome, traced_outcome);
        assert!(!trace.is_empty());
    }

    #[test]
    fn hash_is_sensitive_to_the_stream() {
        // Adversarial: deepen one excursion and the digest
        // must move — a hash that cannot see the stream is
        // not evidence of anything.
        let dynamics = measured_dynamics().expect("dynamics load");
        let mut session =
            Session::new(dynamics, FIXTURE_DAY_START_MINUTE).expect("session");
        for index in FIXTURE_PREP_ACTIONS {
            session.apply_prep_action(index).expect("prep");
        }
        let mut altered = fixture_samples();
        altered[10] += 1.0;
        let digest_of = |samples: &[f64]| {
            let mut s = session.clone();
            let mut trace = Vec::new();
            s.run_attempt_traced(samples, FIXTURE_DT_SECONDS, &mut trace)
                .expect("run");
            let mut hasher = TrajectoryHasher::new();
            for &(p, g) in &trace {
                hasher.write_u64(p);
                hasher.write_u64(g);
            }
            hasher.finish()
        };
        assert_ne!(digest_of(&fixture_samples()), digest_of(&altered));
    }

    #[test]
    fn non_finite_sample_is_a_typed_refusal() {
        // Adversarial input: the traced path refuses exactly
        // where the untraced path does — a typed error,
        // never a panic or a silent zero.
        let dynamics = measured_dynamics().expect("dynamics load");
        let mut session =
            Session::new(dynamics, FIXTURE_DAY_START_MINUTE).expect("session");
        let mut trace = Vec::new();
        let bad = [0.0, f64::NAN];
        assert!(session
            .run_attempt_traced(&bad, FIXTURE_DT_SECONDS, &mut trace)
            .is_err());
    }
}
