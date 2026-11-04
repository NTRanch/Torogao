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
//!
//! The crate also carries the LIFT layer (D-838, Stage 1):
//! the session record (`torogao-scene-record/1`, the
//! `scene`/`custody`/`freeze` modules) — the record a
//! generation session emits, whose renders are legally
//! renders *of the record*, with typed medium-extension
//! blocks, a custody map freezing human-touched nodes
//! against regeneration, and the freeze operations that turn
//! dense proposals into deterministic keyforms and palette
//! snaps under stated contracts. The lift layer shares the
//! seam's containment exactly: same feature, same dependency
//! rule, same removal drill.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod canon;
pub mod compile;
pub mod custody;
pub mod error;
pub mod freeze;
pub mod graph;
pub mod limits;
pub mod measure;
pub mod plan;
pub mod ron_convert;
pub mod ron_tree;
pub mod scene;
pub mod scene_convert;
pub mod schema;

pub use canon::{CanonPose, CanonProfile, CanonRecord, D802_PROFILE, compile_canon};
pub use compile::{AssetBundle, CompileOutput, CompileReport, compile, sha256_hex};
pub use custody::{CustodyEntry, CustodyMap, CustodyState};
pub use error::{GencondError, GencondResult};
pub use freeze::{
    FrozenTrack, Keyform, PalettePair, PaletteSnap, ParameterTrack, TrackSample, freeze_palette,
    freeze_track, track_noise_epsilon,
};
pub use graph::GenGraph;
pub use measure::{EmbedMeasurement, NormalisationRecord, normalize_reference_embed};
pub use plan::{GenerationPlan, LatentSpec, ModelBinding, SamplerSpec};
pub use ron_tree::Ron;
pub use scene::{
    EpsilonOrigin, ExtensionState, FreezeOp, FreezeProvenance, LUMEN_RIG_MODEL_FORMAT,
    LUMEN_RIG_MODEL_FORMAT_VERSION, MediumExtension, MediumKind, Puppet2dRef, RenderRecipe,
    RerenderGuarantee, SCENE_FORMAT, SCENE_FORMAT_VERSION, SceneRecord, conditioning_sha256,
};
pub use schema::Conditioning;
