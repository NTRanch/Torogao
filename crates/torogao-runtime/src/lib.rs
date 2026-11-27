//! torogao-runtime: the hosted-core runtime (runtime plan
//! Rung R1; the same rung as the engine plan's Rung 1 seen from
//! the rebuilds' side).
//!
//! The runtime owns the contract a game core implements to be
//! hosted by Torogao — [`GameCore`] — and the machinery every
//! hosted title gets for free and may not re-implement:
//!
//! - the unified [`Command`] vocabulary (pointer, key, gamepad,
//!   and system input reduced to tick-boundary commands; the
//!   D-465 rule generalized from performed props to all input);
//! - session discipline: genesis from (core, seed), the
//!   append-only op log, and the per-tick hash chain
//!   ([`Session`]);
//! - the save service: a core's canonical blob wrapped in a
//!   runtime envelope that binds core id, tick, and chain tip
//!   ([`SaveEnvelope`]);
//! - the headless replay harness: a recorded op log re-executed
//!   against a fresh core must reproduce the live hash chain
//!   exactly. The harness is the D-925 TUI-first validation
//!   surface — the runtime is validatable with no window open.
//!
//! Determinism is two-tier (plan Decision 2). Tier 1 — sim
//! state, saves, the hash chain — is integer-only by contract:
//! no float may be stored in sim state, appear in a save, or
//! influence a `step` branch. Tier 2 — the derived presentation
//! view ([`PresentationState`]) — is float-bearing, is emitted
//! per frame for shells to render, and is NEVER fed back into
//! `step`. The tier boundary is enforced, not just documented:
//! [`tier1`] scans this crate's sources (and any witness core's)
//! for the forbidden constructs, and the conformance suite in
//! [`contract`] pins golden save bytes so a float crossing into
//! a save fails a gate.
//!
//! Dependency discipline (plan SS1.1): this crate depends
//! downward only on torogao-author (the AUTH-G interpreter
//! lineage) and torogao-format (canonical serialization) —
//! never on any windowing, GPU, audio, or OS crate.
//! torogao-engine / torogao-rig evaluation joins behind a
//! feature in a later rung; R1 carries no float code outside
//! [`present`].

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::float_arithmetic)]

pub mod command;
pub mod contract;
pub mod core;
pub mod corpus;
pub mod hash;
pub mod host;
pub mod oplog;
pub mod present;
pub mod save;
pub mod session;
pub mod tier1;
pub mod tier2;

pub use command::Command;
pub use core::{GameCore, SaveBlob, StepReport, Verdict};
pub use hash::Hash;
pub use oplog::{OpLog, OpLogEntry};
pub use present::{GaugeView, MeterView, PresentationState};
pub use save::SaveEnvelope;
pub use session::{ReplayReport, Session};
pub use tier2::{Band, BandReport, RECORDED_BAND, compare_presentations, presentation_digest};
pub use torogao_author::{Refusal, RefusalClass};
