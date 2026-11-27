//! The hosted-core contract: [`GameCore`].
//!
//! This is the trait surface the three RE rebuilds re-derived
//! by hand (sim step, state hash, save/load, fixture replay)
//! and that MoonDeep's bridge contract specifies from the
//! consumer side — defined once, here, so every hosted title
//! sits behind the identical contract, session discipline,
//! save discipline, and gates.
//!
//! Two shapes of core implement it: a title authored in
//! AUTH-G implements the trait BY BEING INTERPRETED
//! ([`crate::host::author_game::InterpreterCore`]); a title
//! with its own simulation (MoonDeep's core, the Loop Service
//! rebuild) implements it directly in Rust. The runtime does
//! not prefer one.
//!
//! One deliberate deviation from the plan's sketch, recorded
//! in the R1 record: `load` is an instance method
//! (`load(&mut self, blob)`), not an associated constructor.
//! D-467 binds a save to the project revision it was written
//! under; verifying that binding requires the revision
//! context the core was constructed with (for the interpreter:
//! the project and its hash). A free constructor could only
//! re-derive that context from the blob itself — trusting the
//! artefact to name its own binding. Restore-into-instance
//! makes the check real: the core refuses any blob that does
//! not bind to ITS project/revision.

use crate::command::Command;
use crate::hash::Hash;
use crate::present::PresentationState;
use serde::{Deserialize, Serialize};
use torogao_author::Refusal;

/// A core's canonical save bytes: opaque to the runtime,
/// canonical and Tier 1 by the core's own discipline (for the
/// interpreter, the AUTH-G save document; for an own-core
/// game, its versioned save envelope). The runtime wraps this
/// in a [`crate::SaveEnvelope`] rather than inspecting it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SaveBlob(pub String);

impl SaveBlob {
    /// The blob's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the core did with one command. Every command in a
/// step receives exactly one verdict, in order; the verdicts
/// are part of the op log, so a replay must reproduce them
/// alongside the hash chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict")]
pub enum Verdict {
    /// The command was consumed and applied.
    Applied,
    /// The command was refused (unknown id, unmet
    /// precondition, malformed arguments). A refused command
    /// changes nothing.
    Refused {
        /// Why, in the core's own words.
        reason: String,
    },
    /// The command belongs to a vocabulary layer this core
    /// does not consume (host-reduced pointer/key/pad input
    /// reaching an interpreter core; lifecycle notices). The
    /// tick still advances; the command has no sim effect.
    Ignored {
        /// Why, in the core's own words.
        reason: String,
    },
}

/// The outcome of one [`GameCore::step`]: the tick that was
/// executed, one verdict per command, and the core's event
/// trace for the tick (human-readable, in order — the same
/// trace class the AUTH-G interpreter emits).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepReport {
    /// The tick this step executed (the core's tick AFTER the
    /// step, 1-based).
    pub tick: u64,
    /// One verdict per command, in command order.
    pub verdicts: Vec<Verdict>,
    /// Runtime event lines produced by the tick, in order.
    pub events: Vec<String>,
}

/// The contract every hosted game core implements.
pub trait GameCore {
    /// The core's stable identity, bound into every save
    /// envelope (e.g. `auth-g:<game id>` for an interpreted
    /// title, `loop-service/own-core` for the Loop rebuild).
    /// A session refuses a save whose core id differs.
    fn core_id(&self) -> &str;

    /// The current tick (0 at genesis).
    fn tick(&self) -> u64;

    /// Advances exactly one fixed tick. Commands are the ONLY
    /// input; wall-clock time is never an argument. Tier 1
    /// discipline binds this method: no float may be stored,
    /// saved, or branched on along any path it takes.
    fn step(&mut self, commands: &[Command]) -> StepReport;

    /// The sim state hash, computed over integer/canonical
    /// state only. This hash is the cross-platform identity of
    /// the run; twin gates compare it bit-for-bit.
    fn state_hash(&self) -> Hash;

    /// Canonical save bytes for the current tick boundary.
    /// Refusals are typed, never partial writes.
    fn save(&self) -> Result<SaveBlob, Refusal>;

    /// Restores the state carried by `blob` into this core,
    /// verifying the blob's revision binding (D-467) against
    /// the project/revision this core was constructed with.
    /// A failed load mutates nothing.
    fn load(&mut self, blob: &SaveBlob) -> Result<(), Refusal>;

    /// The derived, float-bearing presentation view of the
    /// current state. Never fed back into `step`; its values
    /// are Tier 2 (see [`crate::present`]).
    fn present(&self) -> PresentationState;
}
