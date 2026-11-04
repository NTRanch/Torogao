//! `torogao-gencond` — Torogao's experimental
//! generation-conditioning seam (D-822, DESIGN
//! `torogao-conditioning-seam-20261103/DESIGN.md`).
//!
//! The seam owns: the `Conditioning` authoring record (RON in
//! `project.ron`, JSON on the verb surface), canon compilation
//! (canon record -> conditioning record), and the compiler
//! from authoring records into `torogao-gen-graph/1` JSON
//! graphs — the estate's S-1 graph form, natively, with the
//! false off refused by the type system (no blank-map value)
//! and by the compiler (uniform maps refuse by name).
//!
//! Placement is the experiment's containment (DESIGN §3.3):
//! this crate is an authoring-surface dependency only, reached
//! through torogao-author's `gen-conditioning` feature. No
//! core crate (engine, format, session, vmc, osc, voice,
//! viseme, vrm, companion, app, tui) may depend on it, and it
//! depends on no estate crate itself — an architecture gate in
//! torogao-author's test suite enforces both directions, and
//! the removal drill is rehearsed in the stage report.
//! Deleting this crate and the feature leaves saved
//! conditioning records as inert data — present in the file,
//! round-tripping untouched, refusing compilation with a typed
//! error that names this family.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod canon;
pub mod compile;
pub mod error;
pub mod graph;
pub mod limits;
pub mod measure;
pub mod plan;
pub mod ron_convert;
pub mod ron_tree;
pub mod schema;

pub use canon::{CanonPose, CanonProfile, CanonRecord, D802_PROFILE, compile_canon};
pub use compile::{AssetBundle, CompileOutput, CompileReport, compile, sha256_hex};
pub use error::{GencondError, GencondResult};
pub use graph::GenGraph;
pub use measure::{EmbedMeasurement, NormalisationRecord, normalize_reference_embed};
pub use plan::{GenerationPlan, LatentSpec, ModelBinding, SamplerSpec};
pub use ron_tree::Ron;
pub use schema::Conditioning;
