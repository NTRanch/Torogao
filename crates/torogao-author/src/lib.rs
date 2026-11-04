//! The Torogao authoring core.
//!
//! A project is text: `project.ron` (one canonical serializer,
//! floats through `torogao-format`'s canonical writer) plus
//! `ops.jsonl` (an append-only log of every admitted op and every
//! denial). Every mutating op runs one pipeline:
//! load → [`decide::decide`] → [`apply::step`] → canonical write.
//! A denial is final: no verb, input, or face carries an override,
//! force, or skip-checks parameter.
//!
//! Faces (the `torogao author` CLI now, MCP in slice A4) call
//! [`session::run_verb`] / [`session::run_requests`] and hold no
//! validation logic of their own.
//!
//! Slice A1 implements the project, art intake, parts & meshes,
//! deformers, parameters (without `param_link_define`), and
//! expressions groups: 30 of the manifest's 49 verbs
//! ([`op::MANIFEST`]). The G-10 completion adds nine more — the
//! A2 physics records ([`physics`]), the A3 motion and audio
//! records ([`motion`], [`audio`]), and `export_manifest`
//! ([`export`]) — and the Q1 hypnosis build (D-634) adds two:
//! the binding verbs ([`hypnosis`]) — for 41 of 49; the
//! remaining eight are descoped by recorded ruling
//! ([`op::DESCOPED`]), so every manifest verb is implemented
//! here or refused by name with its ruling and its
//! capability's home.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod apply;
pub mod audio;
pub mod census;
pub mod codec;
#[cfg(feature = "gen-conditioning")]
pub mod conditioning;
pub mod decide;
pub mod diff;
pub mod export;
pub mod game;
pub mod hypnosis;
pub mod import;
pub mod intake;
pub mod limits;
pub mod model;
pub mod motion;
pub mod op;
pub mod physics;
pub mod refusal;
pub mod ron;
pub mod session;
pub mod store;
pub mod validate;
pub mod voice;

pub use refusal::{Refusal, RefusalClass};
pub use session::{Answer, Status, parse_script, run_requests, run_verb, verbs_doc};

/// Whether this build carries the generation-conditioning
/// seam (the `gen-conditioning` feature). The architecture
/// gate reads this constant because an integration test
/// cannot cfg on the library's own features.
pub const GENCOND_ENABLED: bool = cfg!(feature = "gen-conditioning");
