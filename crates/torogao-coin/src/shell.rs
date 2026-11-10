//! The shell binding: torogao-coin wired to a host shell's
//! input stream.
//!
//! The systems crate (`session`, `swing`) speaks in whole
//! attempts: a caller hands `Session::run_attempt` a finished
//! sample stream. A shell — the torogao-app window loop, the
//! Android host, a headless harness — speaks in single pointer
//! events: one sample arrives per frame, preparation is a menu
//! selection between attempts, and the attempt must resolve on
//! the exact frame the gauge fills or the timer dies. This
//! module is the adapter between those two contracts.
//!
//! The wiring, stated plainly:
//!
//! - `CoinShell::prepare` / `message_stats` are the shell's
//!   between-attempt inputs (menu selections on the day
//!   clock). Both refuse while an attempt is in progress, so
//!   the preparation product is constant within an attempt —
//!   the invariant the systems layer's composition assumes.
//! - `CoinShell::begin_attempt` opens the timed window;
//!   `pointer_sample` feeds one pointer-x sample, in the
//!   swing's relative world-x units, per call. On the
//!   resolving sample the shell returns an `AttemptVerdict`:
//!   `win` is the whole contract the caller's core consumes
//!   (COIN-DYNAMIC.md §3 — the core receives
//!   `Attempt { win }` and never sees the pointer); `outcome`
//!   carries the full systems result for the shell's own
//!   presentation and logging.
//! - The driver predicts resolution with its own accumulator
//!   and timer, in the same step order `Session::attempt`
//!   documents (feed, success check, timer decrement, timeout
//!   check), but the authoritative outcome — level rise
//!   included — is produced by handing the buffered stream to
//!   `Session::run_attempt` at resolution. The prediction and
//!   the authoritative outcome must agree bit for bit; the
//!   binding asserts that invariant rather than letting two
//!   implementations drift apart silently.
//!
//! What this binding deliberately does not do: convert shell
//! pixels to world units. The runtime calibration of pointer
//! displacement to world displacement is REVIEW
//! (COIN-DYNAMIC.md §5) — callers feed world-x samples, and
//! no scale is invented here. (`Dynamics::
//! pointer_animation_scale` is a presentation scale in the
//! source, not that calibration.)

use crate::dynamics::{CoinError, Dynamics, StatTriple};
use crate::session::{AttemptOutcome, DT_SECONDS_MAX, Session};
use crate::swing::{SAMPLES_MAX, SwingAccumulator};
use crate::trajectory::{FIXTURE_DAY_START_MINUTE, FIXTURE_DT_SECONDS, fixture_samples};

/// FNV-1a offset basis (64-bit) — the estate's state-hash
/// discipline, as in `trajectory` and
/// `torogao-app/src/state_hash.rs`.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime (64-bit).
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Bound on preparation steps in the day-frame skeleton
/// session (the longest measured action costs 35 minutes and
/// the day frame spans 540, so 16 steps suffice; the bound
/// exists so the script's loop is finite by contract).
const DAY_FRAME_STEPS_MAX: usize = 64;

/// An incremental FNV-1a hasher over little-endian u64 words.
struct ShellHasher {
    state: u64,
}

impl ShellHasher {
    fn new() -> ShellHasher {
        ShellHasher {
            state: FNV_OFFSET_BASIS,
        }
    }

    fn write_u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.state ^= u64::from(byte);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }

    fn write_f64(&mut self, value: f64) {
        self.write_u64(value.to_bits());
    }

    fn finish(&self) -> u64 {
        self.state
    }
}

/// FNV-1a digest of one attempt's per-step trajectory trace,
/// under the same discipline as `trajectory`'s fixture hash:
/// each record's points bits then gauge bits, in order. A
/// streamed fixture attempt therefore hashes identically to
/// the canonical fixture run — the binding is transparent to
/// the S7 cross-platform comparison.
fn hash_trace(trace: &[(u64, u64)]) -> u64 {
    let mut hasher = ShellHasher::new();
    for &(points_bits, gauge_bits) in trace {
        hasher.write_u64(points_bits);
        hasher.write_u64(gauge_bits);
    }
    hasher.finish()
}

/// The resolution of one streamed attempt, as the shell
/// hands it to the game's core.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttemptVerdict {
    /// The core contract value: the gauge filled before the
    /// timer died. The core consumes this and nothing else
    /// about the pointer (COIN-DYNAMIC.md §3).
    pub win: bool,
    /// The full systems outcome (gauge, points, level after)
    /// for the shell's presentation and logging.
    pub outcome: AttemptOutcome,
    /// FNV-1a digest of the attempt's per-step trajectory
    /// trace (see `hash_trace`).
    pub trajectory_hash: u64,
    /// Sample steps the attempt executed, resolving step
    /// included.
    pub steps_executed: usize,
}

/// One in-progress attempt: the streaming half of
/// `Session::attempt`. Owns no session state; at resolution
/// the buffered stream is handed to `Session::run_attempt`,
/// which remains the single authority on outcomes.
struct AttemptDriver {
    dynamics: Dynamics,
    swing: SwingAccumulator,
    buffer: Vec<f64>,
    trace: Vec<(u64, u64)>,
    timer: f64,
    elapsed_seconds: f64,
    dt_seconds: f64,
    target: f64,
    multiplier: f64,
}

impl AttemptDriver {
    fn new(dynamics: Dynamics, dt_seconds: f64, target: f64, multiplier: f64) -> AttemptDriver {
        AttemptDriver {
            timer: dynamics.timer_start(),
            dynamics,
            swing: SwingAccumulator::new(),
            buffer: Vec::new(),
            trace: Vec::new(),
            elapsed_seconds: 0.0,
            dt_seconds,
            target,
            multiplier,
        }
    }

    /// Feed one sample. Returns the predicted verdict
    /// (`Some(win)`) on the resolving sample, `None` while
    /// the attempt runs. A refused sample (non-finite, or
    /// past `SAMPLES_MAX`) changes nothing.
    fn feed(&mut self, x: f64) -> Result<Option<bool>, CoinError> {
        if self.buffer.len() >= SAMPLES_MAX {
            return Err(CoinError::InvalidStep {
                reason: "sample stream exceeds SAMPLES_MAX",
            });
        }
        self.swing.feed(x, &self.dynamics)?;
        self.buffer.push(x);
        let points = self.swing.points();
        self.trace
            .push((points.to_bits(), (points * self.multiplier).to_bits()));
        self.elapsed_seconds += self.dt_seconds;
        if points * self.multiplier >= self.target {
            return Ok(Some(true));
        }
        self.timer -= self.dt_seconds / self.dynamics.timer_divisor();
        if self.timer <= 0.0 {
            return Ok(Some(false));
        }
        Ok(None)
    }

    fn points(&self) -> f64 {
        self.swing.points()
    }

    fn gauge(&self) -> f64 {
        self.swing.points() * self.multiplier
    }
}

/// The bound coin session: one `Session` plus the shell's
/// input state machine around it. States are exclusive —
/// preparation inputs and attempt inputs are never both
/// admissible — and every refusal leaves the state untouched.
pub struct CoinShell {
    dynamics: Dynamics,
    session: Session,
    attempt: Option<AttemptDriver>,
}

impl CoinShell {
    /// A fresh bound session at `day_start_minute` on the
    /// day clock (validated by `Session::new`).
    pub fn new(dynamics: Dynamics, day_start_minute: u32) -> Result<CoinShell, CoinError> {
        let session = Session::new(dynamics.clone(), day_start_minute)?;
        Ok(CoinShell {
            dynamics,
            session,
            attempt: None,
        })
    }

    /// The session's current level.
    pub fn level(&self) -> usize {
        self.session.level()
    }

    /// The session's preparation stats.
    pub fn prep(&self) -> StatTriple {
        self.session.prep()
    }

    /// The session's day clock, in minutes since midnight.
    pub fn clock_minute(&self) -> u32 {
        self.session.clock_minute()
    }

    /// Whether the day has ended (the save gate).
    pub fn can_save(&self) -> bool {
        self.session.can_save()
    }

    /// Whether an attempt window is currently open.
    pub fn attempt_active(&self) -> bool {
        self.attempt.is_some()
    }

    /// The open attempt's current gauge, or `None` between
    /// attempts — the shell's per-frame HUD read.
    pub fn attempt_gauge(&self) -> Option<f64> {
        self.attempt.as_ref().map(AttemptDriver::gauge)
    }

    /// Shell input: one preparation-menu selection. Refused
    /// during an attempt (the preparation product is constant
    /// within an attempt by contract).
    pub fn prepare(&mut self, action_index: usize) -> Result<u32, CoinError> {
        if self.attempt.is_some() {
            return Err(CoinError::InvalidStep {
                reason: "preparation is refused during an attempt",
            });
        }
        self.session.apply_prep_action(action_index)
    }

    /// Shell input: the message-driven stat path. Same
    /// attempt guard as `prepare`.
    pub fn message_stats(&mut self, raw: StatTriple) -> Result<(), CoinError> {
        if self.attempt.is_some() {
            return Err(CoinError::InvalidStep {
                reason: "message stats are refused during an attempt",
            });
        }
        self.session.apply_message_stats(raw)
    }

    /// Shell input: open the timed attempt window with step
    /// `dt_seconds`. The dt contract is the systems layer's
    /// own: finite, in (0, `DT_SECONDS_MAX`].
    pub fn begin_attempt(&mut self, dt_seconds: f64) -> Result<(), CoinError> {
        if self.attempt.is_some() {
            return Err(CoinError::InvalidStep {
                reason: "an attempt is already in progress",
            });
        }
        if !dt_seconds.is_finite() || dt_seconds <= 0.0 || dt_seconds > DT_SECONDS_MAX {
            return Err(CoinError::InvalidStep {
                reason: "dt outside (0, 1] seconds",
            });
        }
        let target = self.dynamics.gauge_maximum(self.session.level())?;
        let multiplier = self.session.gauge_multiplier();
        self.attempt = Some(AttemptDriver::new(
            self.dynamics.clone(),
            dt_seconds,
            target,
            multiplier,
        ));
        Ok(())
    }

    /// Shell input: one pointer-x sample (relative world-x
    /// units; see the module docs on calibration). Returns
    /// the verdict on the resolving sample, `None` while the
    /// attempt runs. Refused with no attempt open; a refused
    /// sample leaves the attempt exactly as it was.
    pub fn pointer_sample(&mut self, x_world: f64) -> Result<Option<AttemptVerdict>, CoinError> {
        let Some(driver) = &mut self.attempt else {
            return Err(CoinError::InvalidStep {
                reason: "pointer sample with no attempt in progress",
            });
        };
        let Some(predicted_win) = driver.feed(x_world)? else {
            return Ok(None);
        };
        let driver = self.attempt.take().expect("attempt present at resolution");
        let outcome = self
            .session
            .run_attempt(&driver.buffer, driver.dt_seconds)?;
        // The driver predicts; the session decides. Agreement
        // is an invariant of the binding (same step order,
        // same inputs), so a disagreement is a defect in this
        // module, never a runtime condition to paper over.
        assert_eq!(
            outcome.success, predicted_win,
            "driver/session verdict drift"
        );
        assert_eq!(
            outcome.points.to_bits(),
            driver.points().to_bits(),
            "driver/session points drift"
        );
        assert_eq!(
            outcome.elapsed_seconds.to_bits(),
            driver.elapsed_seconds.to_bits(),
            "driver/session elapsed drift"
        );
        Ok(Some(AttemptVerdict {
            win: outcome.success,
            outcome,
            trajectory_hash: hash_trace(&driver.trace),
            steps_executed: driver.trace.len(),
        }))
    }

    /// Abandon the open attempt, if any. An attempt has no
    /// side effects before resolution (the systems layer is
    /// the same), so cancelling returns the session exactly
    /// as `begin_attempt` found it. Returns whether an
    /// attempt was open.
    pub fn cancel_attempt(&mut self) -> bool {
        self.attempt.take().is_some()
    }

    /// Deterministic digest of the bound state: level, day
    /// clock, preparation stats, and the open attempt's
    /// progress when one is open. The smoke pattern's state
    /// hash — identical input scripts produce identical
    /// hashes on every platform.
    pub fn state_hash(&self) -> u64 {
        let mut hasher = ShellHasher::new();
        hasher.write_u64(self.session.level() as u64);
        hasher.write_u64(u64::from(self.session.clock_minute()));
        let prep = self.session.prep();
        hasher.write_f64(prep.hp);
        hasher.write_f64(prep.mp);
        hasher.write_f64(prep.sp);
        match &self.attempt {
            None => hasher.write_u64(0),
            Some(driver) => {
                hasher.write_u64(1);
                hasher.write_f64(driver.points());
                hasher.write_f64(driver.gauge());
                hasher.write_f64(driver.timer);
                hasher.write_f64(driver.elapsed_seconds);
                hasher.write_u64(driver.buffer.len() as u64);
            }
        }
        hasher.finish()
    }
}

/// One TF Jam skeleton session, run end to end through the
/// binding: a named script, its headline results, and its
/// final state hash.
#[derive(Clone, Debug, PartialEq)]
pub struct SkeletonSession {
    /// Script name (structural token, not player-facing
    /// content).
    pub name: &'static str,
    /// The session's attempt verdict, or `None` for the
    /// script that makes no attempt.
    pub win: Option<bool>,
    /// Session level at script end.
    pub level_after: usize,
    /// Day clock at script end, in minutes.
    pub clock_minute: u32,
    /// Save gate at script end.
    pub can_save: bool,
    /// `CoinShell::state_hash` at script end.
    pub state_hash: u64,
    /// The attempt's trajectory hash, when the script made
    /// an attempt.
    pub trajectory_hash: Option<u64>,
}

/// Feed a whole stream through the shell, returning the
/// verdict of the attempt it must resolve. A stream that
/// ends mid-attempt breaks the script's own construction —
/// an invariant, asserted, not a runtime refusal.
fn feed_stream(shell: &mut CoinShell, samples: &[f64]) -> Result<AttemptVerdict, CoinError> {
    let mut verdict = None;
    for &x in samples {
        if let Some(v) = shell.pointer_sample(x)? {
            verdict = Some(v);
            break;
        }
    }
    let verdict = verdict.expect("skeleton stream resolves its attempt");
    assert!(!shell.attempt_active(), "resolution closes the attempt");
    Ok(verdict)
}

fn summarize(
    shell: &CoinShell,
    name: &'static str,
    verdict: Option<&AttemptVerdict>,
) -> SkeletonSession {
    SkeletonSession {
        name,
        win: verdict.map(|v| v.win),
        level_after: shell.level(),
        clock_minute: shell.clock_minute(),
        can_save: shell.can_save(),
        state_hash: shell.state_hash(),
        trajectory_hash: verdict.map(|v| v.trajectory_hash),
    }
}

/// The fixture script: the canonical preparation pair and
/// swing stream (the `trajectory` fixture), fed sample by
/// sample through the binding instead of as one batch.
fn fixture_swing_session(dynamics: Dynamics) -> Result<SkeletonSession, CoinError> {
    let mut shell = CoinShell::new(dynamics, FIXTURE_DAY_START_MINUTE)?;
    for index in crate::trajectory::FIXTURE_PREP_ACTIONS {
        shell.prepare(index)?;
    }
    shell.begin_attempt(FIXTURE_DT_SECONDS)?;
    let verdict = feed_stream(&mut shell, &fixture_samples())?;
    Ok(summarize(&shell, "fixture-swing", Some(&verdict)))
}

/// The timeout script: a one-sided pointer drift banks
/// nothing (no direction crossing ever happens), so the
/// derived 20-second window expires. dt is chosen exact in
/// binary (0.3125 / 20 = 0.015625), so the timer reaches
/// exactly 0 at step 64 — the same construction the systems
/// tests pin.
fn one_sided_timeout_session(dynamics: Dynamics) -> Result<SkeletonSession, CoinError> {
    let mut shell = CoinShell::new(dynamics, FIXTURE_DAY_START_MINUTE)?;
    shell.begin_attempt(0.3125)?;
    let samples: Vec<f64> = (0..64).map(|i| 0.1 + 0.1 * f64::from(i % 7)).collect();
    let verdict = feed_stream(&mut shell, &samples)?;
    assert_eq!(verdict.outcome.elapsed_seconds, 20.0);
    Ok(summarize(&shell, "one-sided-timeout", Some(&verdict)))
}

/// The day-frame script: preparation selections alone carry
/// the clock to the leaving minute, and the save gate opens
/// — no attempt is made.
fn day_frame_save_session(dynamics: Dynamics) -> Result<SkeletonSession, CoinError> {
    let mut shell = CoinShell::new(dynamics, FIXTURE_DAY_START_MINUTE)?;
    let mut steps = 0_usize;
    while !shell.can_save() {
        assert!(steps < DAY_FRAME_STEPS_MAX, "day-frame script is bounded");
        // Action 8 is the longest measured action (35
        // minutes); any fixed choice reaches the gate, this
        // one in the fewest steps.
        shell.prepare(8)?;
        steps += 1;
    }
    Ok(summarize(&shell, "day-frame-save", None))
}

/// Run the TF Jam skeleton sessions on the bound crate: the
/// three scripts the skeleton programme exercises (a winning
/// swing, a timed-out swing, the day frame), each through
/// the shell input surface a live shell would drive.
pub fn run_skeleton_sessions(dynamics: Dynamics) -> Result<Vec<SkeletonSession>, CoinError> {
    Ok(vec![
        fixture_swing_session(dynamics.clone())?,
        one_sided_timeout_session(dynamics.clone())?,
        day_frame_save_session(dynamics)?,
    ])
}
