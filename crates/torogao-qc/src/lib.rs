//! torogao-qc: the render-QC checker — step 1 of the estate
//! RENDER-QC-CONTRACT (adapted 2026-11-26 from Onimite's
//! agent-checking contract; the contract document lives in
//! `~/workspace/onimite-adapt-20261126/RENDER-QC-CONTRACT.md`).
//!
//! Two verbs:
//!
//! - [`inspect::inspect`] — a static pass over a frozen scene
//!   spec (D-1397) and its optional asset manifest. It decides
//!   every contract section-1 code that spec + manifest can
//!   decide, and emits the section-4 JSON report: findings with
//!   stable codes, each carrying a `spec_ref` (an RFC 6901 JSON
//!   Pointer to the spec node that produced it), plus a
//!   `summary {pass, fail, findings_by_code}`.
//! - [`diff::diff_dirs`] — the section-3 snapshot diff: two
//!   directories of PNG frames compared per frame against a
//!   named pixel threshold, reported as
//!   `{frames_compared, frames_over, worst_frames}` with a
//!   `QC_SNAPSHOT_DIFF` finding per frame over threshold.
//!
//! The checker is deterministic and offline: the same inputs
//! produce a byte-identical report. No timestamps, no absolute
//! paths, no hash-map iteration order anywhere in the output.
//!
//! The inspect input is a JSON object; every section is
//! optional and a check family whose inputs are absent is
//! skipped silently (the report never invents data):
//!
//! - `assets`: `[{id?, path|file, sha256?}]`, plus any object
//!   anywhere in the document carrying a string `file` field
//!   (the shape `torogao-scene/1` plates use). Paths resolve
//!   under the spec file's directory; absolute paths and paths
//!   escaping that root are a typed error (fail-closed).
//! - `options`: an object whose keys are checked against the
//!   known-option set (see [`options`]).
//! - `frame {width, height}` + `elements`: `[{id, kind?, text?,
//!   panel?, exit?, bounds {x, y, w, h}}]`, with an optional
//!   top-level `overlap_whitelist: [[id, id], ...]`.
//! - `fps` + `cues`: `[{id, frame|at_frame, ms|at_ms}]`.
//! - identity: spec `characters: [{id, identity: {...}}]`
//!   compared against a sidecar lock record named
//!   `<spec-stem>.lock.json` beside the spec
//!   (`{characters: {<id>: {<param>: <value>}}}`). Where no
//!   lock record exists the check emits nothing.
//!
//! Hard failures (unreadable or malformed input, path escape,
//! bound violations) are typed [`error::QcError`] values with
//! stable `QC-E-*` codes — never panics, never findings.

#![forbid(unsafe_code)]

pub mod error;
pub mod inspect;
pub mod limits;
pub mod options;
pub mod prescan;
pub mod report;
