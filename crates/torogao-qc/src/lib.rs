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
//! Three asset-level verbs sit beside them (the B1–B3 patch
//! set, adapted 2026-11-26 from the wave-2 Blender review's
//! BlenderLens/ellmos patterns; records in
//! `~/workspace/blender-patterns-renderqc-20261126/RECORDS.md`).
//! All three read GLBs through [`glb`], the estate's existing
//! reader in `torogao-vrm`:
//!
//! - [`asset::check_assets`] — B1: a project-owned rules file
//!   (palette, budgets, anchor/envelope) evaluated over GLB
//!   assets; codes `QC_ASSET_PALETTE`, `QC_ASSET_BUDGET`,
//!   `QC_ASSET_ORIGIN`. The rules file's hash is recorded in
//!   the report's `extra`.
//! - [`export_check::export_check`] — B2: an exported GLB
//!   re-opened and audited against a pinned expectation
//!   manifest for semantic losses (dropped shape keys, missing
//!   UV channels, missing tangents); one code,
//!   `QC_EXPORT_LOSS`, with the loss named in the finding's
//!   `data.kind` from a closed vocabulary.
//! - [`roundtrip::roundtrip`] — B3, the second tier of B2:
//!   the exported GLB is reimported by a pinned headless
//!   Blender and its structural summary diffed against the
//!   source GLB's; differences join the same report as
//!   `QC_EXPORT_LOSS` findings whose `data.stage` is
//!   `on_reimport` (B2's own carry `in_file`).
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

pub mod asset;
pub mod diff;
pub mod error;
pub mod export_check;
pub mod glb;
pub mod inspect;
pub mod limits;
pub mod options;
pub mod prescan;
pub mod report;
pub mod roundtrip;

use crate::error::QcError;
use std::io::Read;
use std::path::Path;

/// Reads a file with a hard size bound: the metadata length is
/// checked first, and the read itself is capped, so a file
/// that grows between the two cannot exceed the bound either.
/// Shared by the asset-level verbs; `kind` names the error
/// family (`QC-E-<kind>-READ` / `QC-E-<kind>-TOO-LARGE`).
pub(crate) fn read_bounded(path: &Path, max: u64, kind: &str) -> Result<Vec<u8>, QcError> {
    let meta = std::fs::metadata(path).map_err(|err| {
        QcError::new(
            format!("QC-E-{kind}-READ"),
            format!("cannot stat {}: {err}", path.display()),
        )
    })?;
    if meta.len() > max {
        return Err(QcError::new(
            format!("QC-E-{kind}-TOO-LARGE"),
            format!(
                "{} is {} bytes, over the {max}-byte bound",
                path.display(),
                meta.len()
            ),
        ));
    }
    let file = std::fs::File::open(path).map_err(|err| {
        QcError::new(
            format!("QC-E-{kind}-READ"),
            format!("cannot read {}: {err}", path.display()),
        )
    })?;
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes).map_err(|err| {
        QcError::new(
            format!("QC-E-{kind}-READ"),
            format!("cannot read {}: {err}", path.display()),
        )
    })?;
    if bytes.len() as u64 > max {
        return Err(QcError::new(
            format!("QC-E-{kind}-TOO-LARGE"),
            format!(
                "{} grew past the {max}-byte bound while reading",
                path.display()
            ),
        ));
    }
    Ok(bytes)
}
