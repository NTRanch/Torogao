//! torogao-mobile-rig — the rig lane of Torogao Mobile.
//!
//! Owns the character as data and the rules around it:
//!
//! - [`schema`]: the `torogao-character/1` record — one text
//!   document per character (identity lock, prompt identity,
//!   body/face parameters, expressions, outfit, rig, art), the
//!   ONLY thing the phone edits. Canonical serialisation and a
//!   SHA-256 over it make every revision hashable and diffable.
//! - [`params`]: the named body/face parameter layer over the
//!   frozen builder spec, and the one deterministic function
//!   that applies it (`derive_spec`). Knob application lives
//!   exactly here, client-side; the primo adapter builds the
//!   spec it was handed and verifies its hash, never re-derives.
//! - [`morphs`]: the expression morph table — every morph the
//!   fleet builder emits, with its moving mechanism named from
//!   the builder's own `compute_morphs` (Rule 10: a morph's
//!   name is never evidence of what it moves). A record naming
//!   a morph outside this table does not validate.
//! - [`gate`]: the Step 0 derive gate (no identity lock, no
//!   derivation — art origination is unaffected), the A10.1
//!   bend-battery and A10.2 export-census verdict logic over
//!   the estate instruments' raw output, and the lock check
//!   comparing a derivation's measured proportions against the
//!   locked ones.
//! - [`jobs`]: the hash-pinned rig job documents (derive,
//!   pose-test, preview-render) the facade accepts, and the
//!   result documents it files.
//! - [`client`]: the blocking facade client for those jobs,
//!   with the same bounded discipline as the core's gen client.
//! - [`cast_records`]: the compile-time cast table's three
//!   records (Prima, Lana, Sirena) migrated into
//!   `torogao-character/1` records verbatim, citations carried.
//!
//! Derivation is a JOB, not a slider drag (the parametric body
//! layer is design-only; ARCHITECTURE §5 route B1): the phone
//! submits a record revision, primo's builder service builds
//! it under the official Blender build, the gates run
//! primo-side, and the gated GLB plus preview stills come back
//! through the facade. This crate never solves and never
//! gates on-device; it carries the record and the verdict
//! logic both sides share.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod cast_records;
pub mod client;
pub mod gate;
pub mod jobs;
pub mod morphs;
pub mod params;
pub mod schema;

pub use client::{RigClient, RigError};
pub use gate::{GateResults, Verdict, derive_eligibility};
pub use jobs::{RigJobDocument, RigJobKind, RigJobResult};
pub use schema::{CharacterRecord, SCHEMA_ID};
