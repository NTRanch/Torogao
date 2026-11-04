//! torogao-motion-clip: the mocap programme's P1 record —
//! parser, validator, and layer replay evaluator for
//! `torogao-motion-clip/1` documents.
//!
//! The format is specified by `torogao-motion-clip/1`
//! (SPEC.md in `~/workspace/torogao-motion-clip-spec-20261103/`);
//! this crate is its implementation. A motion clip is a
//! skeletal master record: a base clip (bone TRS tracks and
//! scalar channel tracks, keyed by frame at a canonical
//! 60 Hz) plus an ordered stack of additive layers —
//! scoped, weighted delta tracks that compose over the
//! base at evaluation time and can never reference it
//! (the D-736 empty-by-construction law).
//!
//! The pipeline is three stages, mirroring the spec:
//!
//! 1. [`parse_document`] — document validation (SPEC.md
//!    section 9, steps 1–10): field-strict JSON, every
//!    invalidity the spec names rejected with its typed
//!    [`ClipError`] code, first failure in spec order.
//! 2. [`BoundClip::bind`] — binding validation (steps
//!    11–13) against a [`SkeletonDef`] and the rig's
//!    [`ChannelDecls`].
//! 3. Evaluation ([`BoundClip::evaluate_frame`]) — the
//!    exact arithmetic of section 7: a pure function of
//!    the document, the def, the declarations, and the
//!    frame index; provenance is never an input.
//!
//! The SkeletonDef record itself is a separate record
//! family's specification (SPEC.md section 14, item 2);
//! this crate consumes it through the [`SkeletonDef`]
//! trait and invents none of its shape. Intake converters
//! (BVH, VMC, VMD import) are later mocap phases and are
//! deliberately absent: they will *emit* this format, and
//! nothing here depends on them.

#![forbid(unsafe_code)]

pub mod bind;
pub mod error;
pub mod eval;
pub mod limits;
pub mod model;
pub mod raw;
pub mod uclip;
pub mod validate;

pub use bind::{BoneDef, BoundClip, ChannelDecls, ChannelDef, SkeletonDef};
pub use error::ClipError;
pub use eval::{
    BonePose, EvalError, EvaluatedFrame, frame_map_source_at, frame_map_take_frames_for_source,
};
pub use model::{
    BoneTrack, CameraMode, ChannelTrack, Clip, DeltaQuatKey, DeltaScalarKey, DeltaVecKey,
    FORMAT_TOKEN, FrameMap, FrameRate, Group, GroupKind, GroupProvenance, Interpolation, Layer,
    LayerBoneTrack, LayerChannelTrack, LayerTrack, PROVENANCE_TOKEN, Provenance, QuatKey, Region,
    ScalarKey, Scope, SkeletonBinding, SourceClass, VecKey,
};
pub use uclip::sha256_hex;
pub use validate::parse_document;
