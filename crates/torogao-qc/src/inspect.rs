//! The inspect pass: contract section 1 over a frozen scene
//! spec and its manifest. Every check family runs only when
//! its inputs are present in the spec; absent inputs are
//! skipped silently, never synthesised.

use crate::error::QcError;
use crate::limits::*;
use crate::report::{Finding, Report, Severity, round2};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Runs the inspect pass over the spec at `spec_path`.
///
/// `manifest_path` optionally names a manifest JSON
/// (`{assets: {<id>: {path, sha256} | "<sha256>"}}`) whose
/// pinned hashes and extra asset entries merge with the
/// spec's own. `known_options` is the `QC_UNUSED_OPTION` set.
///
/// # Errors
///
/// Typed `QC-E-*` on unreadable/malformed input, on any asset
/// path that is absolute or escapes the spec's directory, and
/// on bound violations. Findings are never errors.
pub fn inspect(
    spec_path: &Path,
    manifest_path: Option<&Path>,
    known_options: &BTreeSet<String>,
) -> Result<Report, QcError> {
    let bytes = read_bounded(spec_path, SPEC_BYTES_MAX, "SPEC")?;
    // Non-finite numbers (1e999, NaN, Infinity) are a
    // QC_NAN_VALUE verdict, not a parse failure: the prescan
    // records them and sanitizes them out of the parsed copy.
    let prescan = crate::prescan::prescan(&bytes);
    let spec: Value = serde_json::from_slice(&prescan.text).map_err(|err| {
        QcError::new(
            "QC-E-SPEC-PARSE",
            format!("spec {} is not valid JSON: {err}", spec_path.display()),
        )
    })?;
    let root_obj = spec
        .as_object()
        .ok_or_else(|| QcError::new("QC-E-SPEC-SHAPE", "spec root must be a JSON object"))?;
    let root = spec_root(spec_path)?;
    let scene = root_obj
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| file_stem(spec_path));
    let mut report = Report::new("inspect", scene);
    report
        .extra
        .push(("spec", Value::String(file_name(spec_path))));
    for hit in &prescan.findings {
        report.push(Finding {
            code: "QC_NAN_VALUE",
            severity: Severity::Fail,
            element: None,
            frame: None,
            detail: format!(
                "value at {} is not finite (written as {})",
                hit.pointer, hit.token
            ),
            spec_ref: hit.pointer.clone(),
        });
    }
    walk_nan(&spec, &mut Vec::new(), &mut report);
    check_assets(&spec, manifest_path, &root, &mut report)?;
    check_options(root_obj, known_options, &mut report);
    check_geometry(root_obj, &mut report);
    check_sync(root_obj, &mut report);
    check_identity(&spec, root_obj, spec_path, &root, &mut report)?;
    report.sort_findings();
    Ok(report)
}

/// Reads a file with a hard size bound: the metadata length is
/// checked first, and the read itself is capped, so a file
/// that grows between the two cannot exceed the bound either.
fn read_bounded(path: &Path, max: u64, kind: &str) -> Result<Vec<u8>, QcError> {
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

/// The directory a spec's relative asset paths resolve under:
/// the spec file's own directory, canonicalised.
fn spec_root(spec_path: &Path) -> Result<PathBuf, QcError> {
    let parent = spec_path.parent().unwrap_or_else(|| Path::new("."));
    parent.canonicalize().map_err(|err| {
        QcError::new(
            "QC-E-SPEC-READ",
            format!("cannot resolve spec directory {}: {err}", parent.display()),
        )
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Escapes one JSON Pointer segment (RFC 6901).
fn escape_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Builds a JSON Pointer from path segments.
fn pointer(segments: &[String]) -> String {
    let mut out = String::new();
    for segment in segments {
        out.push('/');
        out.push_str(&escape_segment(segment));
    }
    out
}

/// QC_NAN_VALUE: walks every number in the document; any
/// non-finite float (JSON text such as `1e999`, which
/// serde_json reads as infinity) is a finding at its pointer.
fn walk_nan(value: &Value, segments: &mut Vec<String>, report: &mut Report) {
    match value {
        Value::Number(number) => {
            if let Some(f) = number.as_f64() {
                if !f.is_finite() {
                    let at = pointer(segments);
                    report.push(Finding {
                        code: "QC_NAN_VALUE",
                        severity: Severity::Fail,
                        element: None,
                        frame: None,
                        detail: format!("value at {at} is not finite"),
                        spec_ref: at,
                    });
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                segments.push(index.to_string());
                walk_nan(item, segments, report);
                segments.pop();
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                segments.push(key.clone());
                walk_nan(item, segments, report);
                segments.pop();
            }
        }
        _ => {}
    }
}

/// One asset reference to verify.
struct AssetRef {
    id: Option<String>,
    rel: String,
    sha256: Option<String>,
    spec_ref: String,
}

/// Collects asset references: any object with a string `file`
/// field anywhere in the spec, plus `assets[]` entries using
/// `path`. Carries the entry's own `id`/`sha256` when present.
fn collect_spec_refs(value: &Value, segments: &mut Vec<String>, out: &mut Vec<AssetRef>) {
    match value {
        Value::Object(map) => {
            let rel = map.get("file").and_then(Value::as_str).map(str::to_string);
            let rel = rel.or_else(|| {
                if segments.first().map(String::as_str) == Some("assets") {
                    map.get("path").and_then(Value::as_str).map(str::to_string)
                } else {
                    None
                }
            });
            if let Some(rel) = rel {
                out.push(AssetRef {
                    id: map.get("id").and_then(Value::as_str).map(str::to_string),
                    rel,
                    sha256: map
                        .get("sha256")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    spec_ref: pointer(segments),
                });
            }
            for (key, item) in map {
                segments.push(key.clone());
                collect_spec_refs(item, segments, out);
                segments.pop();
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                segments.push(index.to_string());
                collect_spec_refs(item, segments, out);
                segments.pop();
            }
        }
        _ => {}
    }
}

/// Loads the manifest: a hash map (asset id or relative path
/// -> pinned sha256) plus standalone refs for manifest entries
/// whose path the spec does not already reference.
fn load_manifest(
    path: &Path,
    spec_refs: &[AssetRef],
) -> Result<(BTreeMap<String, String>, Vec<AssetRef>), QcError> {
    let bytes = read_bounded(path, MANIFEST_BYTES_MAX, "MANIFEST")?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|err| {
        QcError::new(
            "QC-E-MANIFEST-PARSE",
            format!("manifest {} is not valid JSON: {err}", path.display()),
        )
    })?;
    let assets = value
        .get("assets")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            QcError::new(
                "QC-E-MANIFEST-SHAPE",
                "manifest must be an object with an `assets` object",
            )
        })?;
    let spec_paths: BTreeSet<&str> = spec_refs.iter().map(|r| r.rel.as_str()).collect();
    let mut hashes = BTreeMap::new();
    let mut extra = Vec::new();
    for (key, entry) in assets {
        let (rel, sha) = match entry {
            Value::String(sha) => (None, Some(sha.clone())),
            Value::Object(map) => (
                map.get("path").and_then(Value::as_str).map(str::to_string),
                map.get("sha256")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            ),
            _ => (None, None),
        };
        if let Some(sha) = &sha {
            hashes.insert(key.clone(), sha.clone());
            if let Some(rel) = &rel {
                hashes.insert(rel.clone(), sha.clone());
            }
        }
        if let Some(rel) = rel {
            if !spec_paths.contains(rel.as_str()) {
                extra.push(AssetRef {
                    id: Some(key.clone()),
                    rel,
                    sha256: sha,
                    spec_ref: format!("manifest:/assets/{key}"),
                });
            }
        }
    }
    Ok((hashes, extra))
}

/// Resolves a spec-relative asset path under the root,
/// rejecting absolute paths and any `..` that escapes —
/// fail-closed, as a typed error, before any file is touched.
fn resolve_contained(root: &Path, rel: &str) -> Result<PathBuf, QcError> {
    let escape = || {
        QcError::new(
            "QC-E-PATH-ESCAPE",
            format!("asset path {rel:?} is absolute or escapes the spec root"),
        )
    };
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for component in Path::new(rel).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(escape());
                }
            }
            Component::RootDir | Component::Prefix(_) => return Err(escape()),
        }
    }
    if parts.is_empty() {
        return Err(QcError::new(
            "QC-E-ASSET-PATH",
            format!("asset path {rel:?} names no file"),
        ));
    }
    let mut resolved = root.to_path_buf();
    for part in parts {
        resolved.push(part);
    }
    Ok(resolved)
}

/// Streams a file's SHA-256 as lowercase hex, size-bounded.
fn hash_file(path: &Path) -> Result<String, QcError> {
    let meta = std::fs::metadata(path).map_err(|err| {
        QcError::new(
            "QC-E-ASSET-READ",
            format!("cannot stat asset {}: {err}", path.display()),
        )
    })?;
    if meta.len() > HASH_BYTES_MAX {
        return Err(QcError::new(
            "QC-E-ASSET-TOO-LARGE",
            format!(
                "asset {} is {} bytes, over the {HASH_BYTES_MAX}-byte hash bound",
                path.display(),
                meta.len()
            ),
        ));
    }
    let mut file = std::fs::File::open(path).map_err(|err| {
        QcError::new(
            "QC-E-ASSET-READ",
            format!("cannot read asset {}: {err}", path.display()),
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; HASH_BUFFER_BYTES];
    let mut total: u64 = 0;
    loop {
        let read = file.read(&mut buffer).map_err(|err| {
            QcError::new(
                "QC-E-ASSET-READ",
                format!("cannot read asset {}: {err}", path.display()),
            )
        })?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > HASH_BYTES_MAX {
            return Err(QcError::new(
                "QC-E-ASSET-TOO-LARGE",
                format!(
                    "asset {} grew past the hash bound while reading",
                    path.display()
                ),
            ));
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// QC_MISSING_MEDIA: every referenced asset must exist under
/// the root and, where a hash is pinned (by the spec entry or
/// the manifest), match it.
fn check_assets(
    spec: &Value,
    manifest_path: Option<&Path>,
    root: &Path,
    report: &mut Report,
) -> Result<(), QcError> {
    let mut refs = Vec::new();
    collect_spec_refs(spec, &mut Vec::new(), &mut refs);
    let mut hashes = BTreeMap::new();
    if let Some(path) = manifest_path {
        let (map, extra) = load_manifest(path, &refs)?;
        hashes = map;
        refs.extend(extra);
    }
    if refs.len() > ASSETS_MAX {
        return Err(QcError::new(
            "QC-E-TOO-MANY-ASSETS",
            format!(
                "{} asset references, over the {ASSETS_MAX} bound",
                refs.len()
            ),
        ));
    }
    for asset in &refs {
        let sha = asset.sha256.clone().or_else(|| {
            asset
                .id
                .as_ref()
                .and_then(|id| hashes.get(id).cloned())
                .or_else(|| hashes.get(&asset.rel).cloned())
        });
        check_one_asset(asset, sha.as_deref(), root, report)?;
    }
    Ok(())
}

fn check_one_asset(
    asset: &AssetRef,
    sha: Option<&str>,
    root: &Path,
    report: &mut Report,
) -> Result<(), QcError> {
    let resolved = resolve_contained(root, &asset.rel)?;
    if !resolved.is_file() {
        report.push(Finding {
            code: "QC_MISSING_MEDIA",
            severity: Severity::Fail,
            element: asset.id.clone(),
            frame: None,
            detail: format!("asset not found: {}", asset.rel),
            spec_ref: asset.spec_ref.clone(),
        });
        return Ok(());
    }
    if let Some(pinned) = sha {
        let actual = hash_file(&resolved)?;
        if !actual.eq_ignore_ascii_case(pinned.trim()) {
            report.push(Finding {
                code: "QC_MISSING_MEDIA",
                severity: Severity::Fail,
                element: asset.id.clone(),
                frame: None,
                detail: format!(
                    "sha256 mismatch for {}: pinned {}, actual {}",
                    asset.rel, pinned, actual
                ),
                spec_ref: asset.spec_ref.clone(),
            });
        }
    }
    Ok(())
}

/// QC_UNUSED_OPTION: keys of the spec's `options` object that
/// the known set does not consume.
fn check_options(
    spec: &serde_json::Map<String, Value>,
    known_options: &BTreeSet<String>,
    report: &mut Report,
) {
    let Some(options) = spec.get("options").and_then(Value::as_object) else {
        return;
    };
    for key in options.keys() {
        if !known_options.contains(key) {
            report.push(Finding {
                code: "QC_UNUSED_OPTION",
                severity: Severity::Warning,
                element: None,
                frame: None,
                detail: format!("option {key:?} is not in the known option set"),
                spec_ref: format!("/options/{}", escape_segment(key)),
            });
        }
    }
}

/// An axis-aligned rectangle from an element's `bounds`.
#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn contains(self, inner: Rect) -> bool {
        inner.x >= self.x
            && inner.y >= self.y
            && inner.x + inner.w <= self.x + self.w
            && inner.y + inner.h <= self.y + self.h
    }

    fn intersects(self, other: Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }
}

fn rect_of(value: &Value) -> Option<Rect> {
    let get = |key: &str| value.get(key).and_then(Value::as_f64);
    let rect = Rect {
        x: get("x")?,
        y: get("y")?,
        w: get("w")?,
        h: get("h")?,
    };
    if rect.w < 0.0 || rect.h < 0.0 {
        return None;
    }
    Some(rect)
}

/// One element of the spec's `elements` array, pre-digested.
struct Element {
    index: usize,
    id: String,
    is_text: bool,
    bounds: Option<Rect>,
    panel: Option<String>,
    exit: bool,
}

fn elements_of(spec: &serde_json::Map<String, Value>) -> Vec<Element> {
    let Some(items) = spec.get("elements").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| Element {
            index,
            id: item
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("#{index}")),
            is_text: item.get("kind").and_then(Value::as_str) == Some("text")
                || item.get("text").is_some(),
            bounds: item.get("bounds").and_then(rect_of),
            panel: item
                .get("panel")
                .and_then(Value::as_str)
                .map(str::to_string),
            exit: item.get("exit").and_then(Value::as_bool) == Some(true),
        })
        .collect()
}

/// QC_TEXT_CLIPPED, QC_TEXT_OVERLAP, QC_OFF_FRAME — only
/// where the spec carries both a frame and element bounds.
fn check_geometry(spec: &serde_json::Map<String, Value>, report: &mut Report) {
    let elements = elements_of(spec);
    if elements.len() > ELEMENTS_MAX {
        // Bounded by construction: the array itself was parsed
        // under SPEC_BYTES_MAX; past the element bound the
        // geometry families are skipped, not sampled.
        return;
    }
    let frame = spec.get("frame").and_then(|f| {
        let w = f.get("width").and_then(Value::as_f64)?;
        let h = f.get("height").and_then(Value::as_f64)?;
        Some(Rect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        })
    });
    let by_id: BTreeMap<&str, &Element> = elements.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut clipped_by_frame: BTreeSet<usize> = BTreeSet::new();
    for element in &elements {
        let Some(bounds) = element.bounds else {
            continue;
        };
        if element.is_text {
            let container = element
                .panel
                .as_ref()
                .and_then(|id| by_id.get(id.as_str()))
                .and_then(|panel| panel.bounds)
                .or(frame);
            if let Some(container) = container {
                if !container.contains(bounds) {
                    if element.panel.is_none() {
                        clipped_by_frame.insert(element.index);
                    }
                    report.push(Finding {
                        code: "QC_TEXT_CLIPPED",
                        severity: Severity::Finding,
                        element: Some(element.id.clone()),
                        frame: None,
                        detail: format!("text element {:?} bounds leave its container", element.id),
                        spec_ref: format!("/elements/{}", element.index),
                    });
                }
            }
        }
        if let Some(frame) = frame {
            if !element.exit
                && !frame.contains(bounds)
                && !clipped_by_frame.contains(&element.index)
            {
                report.push(Finding {
                    code: "QC_OFF_FRAME",
                    severity: Severity::Finding,
                    element: Some(element.id.clone()),
                    frame: None,
                    detail: format!("element {:?} bounds leave the frame", element.id),
                    spec_ref: format!("/elements/{}", element.index),
                });
            }
        }
    }
    check_overlaps(spec, &elements, report);
}

fn check_overlaps(
    spec: &serde_json::Map<String, Value>,
    elements: &[Element],
    report: &mut Report,
) {
    let mut whitelist: BTreeSet<(String, String)> = BTreeSet::new();
    if let Some(pairs) = spec.get("overlap_whitelist").and_then(Value::as_array) {
        for pair in pairs {
            if let Some(pair) = pair.as_array() {
                if let (Some(a), Some(b)) = (
                    pair.first().and_then(Value::as_str),
                    pair.get(1).and_then(Value::as_str),
                ) {
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    whitelist.insert((lo.to_string(), hi.to_string()));
                }
            }
        }
    }
    for (i, a) in elements.iter().enumerate() {
        for b in &elements[i + 1..] {
            if !a.is_text && !b.is_text {
                continue;
            }
            // A text element and the panel that contains it
            // intersect by construction; that pair is the
            // clipped check's business, not an overlap.
            if a.panel.as_deref() == Some(b.id.as_str())
                || b.panel.as_deref() == Some(a.id.as_str())
            {
                continue;
            }
            let (Some(ab), Some(bb)) = (a.bounds, b.bounds) else {
                continue;
            };
            if !ab.intersects(bb) {
                continue;
            }
            let (lo, hi) = if a.id <= b.id {
                (a.id.clone(), b.id.clone())
            } else {
                (b.id.clone(), a.id.clone())
            };
            if whitelist.contains(&(lo, hi)) {
                continue;
            }
            report.push(Finding {
                code: "QC_TEXT_OVERLAP",
                severity: Severity::Finding,
                element: Some(if a.is_text {
                    a.id.clone()
                } else {
                    b.id.clone()
                }),
                frame: None,
                detail: format!("elements {:?} and {:?} overlap", a.id, b.id),
                spec_ref: format!("/elements/{}", b.index),
            });
        }
    }
}

/// QC_SYNC_DRIFT: a cue carrying both a frame and a time
/// whose disagreement exceeds one frame at the spec's fps.
fn check_sync(spec: &serde_json::Map<String, Value>, report: &mut Report) {
    let Some(fps) = spec.get("fps").and_then(Value::as_f64) else {
        return;
    };
    if fps <= 0.0 || !fps.is_finite() {
        return;
    }
    let Some(cues) = spec.get("cues").and_then(Value::as_array) else {
        return;
    };
    if cues.len() > CUES_MAX {
        return;
    }
    let frame_ms = 1000.0 / fps;
    for (index, cue) in cues.iter().enumerate() {
        let frame = cue
            .get("frame")
            .or_else(|| cue.get("at_frame"))
            .and_then(Value::as_f64);
        let ms = cue
            .get("ms")
            .or_else(|| cue.get("at_ms"))
            .and_then(Value::as_f64);
        let (Some(frame), Some(ms)) = (frame, ms) else {
            continue;
        };
        let drift_frames = (ms - frame * frame_ms).abs() / frame_ms;
        if drift_frames > 1.0 {
            report.push(Finding {
                code: "QC_SYNC_DRIFT",
                severity: Severity::Finding,
                element: cue.get("id").and_then(Value::as_str).map(str::to_string),
                frame: Some(frame),
                detail: format!(
                    "cue at frame {frame} is timed at {ms} ms, {} frames from its frame time",
                    round2(drift_frames)
                ),
                spec_ref: format!("/cues/{index}"),
            });
        }
    }
}

/// QC_IDENTITY_DRIFT: spec character identity parameters
/// against the sidecar lock record `<spec-stem>.lock.json`.
/// No lock record beside the spec -> no findings, by design.
fn check_identity(
    _spec: &Value,
    spec_obj: &serde_json::Map<String, Value>,
    spec_path: &Path,
    root: &Path,
    report: &mut Report,
) -> Result<(), QcError> {
    let lock_name = format!("{}.lock.json", file_stem(spec_path));
    let lock_path = root.join(&lock_name);
    if !lock_path.is_file() {
        return Ok(());
    }
    let bytes = read_bounded(&lock_path, LOCK_BYTES_MAX, "LOCK")?;
    let lock: Value = serde_json::from_slice(&bytes).map_err(|err| {
        QcError::new(
            "QC-E-LOCK-PARSE",
            format!(
                "identity lock {} is not valid JSON: {err}",
                lock_path.display()
            ),
        )
    })?;
    let locked = lock_characters(&lock);
    let Some(characters) = spec_obj.get("characters").and_then(Value::as_array) else {
        return Ok(());
    };
    for (index, character) in characters.iter().enumerate() {
        let id = character
            .get("id")
            .or_else(|| character.get("name"))
            .and_then(Value::as_str);
        let Some(id) = id else { continue };
        let Some(params) = locked.get(id) else {
            continue;
        };
        let identity = character.get("identity");
        for (param, locked_value) in params {
            let spec_value = identity.and_then(|i| i.get(param));
            match spec_value {
                None => report.push(Finding {
                    code: "QC_IDENTITY_DRIFT",
                    severity: Severity::Fail,
                    element: Some(id.to_string()),
                    frame: None,
                    detail: format!(
                        "locked identity parameter {param:?} of {id:?} is absent from the spec"
                    ),
                    spec_ref: format!("/characters/{index}"),
                }),
                Some(value) if !json_eq(value, locked_value) => {
                    report.push(Finding {
                        code: "QC_IDENTITY_DRIFT",
                        severity: Severity::Fail,
                        element: Some(id.to_string()),
                        frame: None,
                        detail: format!(
                            "identity parameter {param:?} of {id:?} differs from the lock: \
                             locked {locked_value}, spec {value}"
                        ),
                        spec_ref: format!("/characters/{index}/identity"),
                    });
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The lock record's character map: `characters` object when
/// present, else the record itself treated as the map.
fn lock_characters(lock: &Value) -> BTreeMap<String, BTreeMap<String, Value>> {
    let source = lock
        .get("characters")
        .and_then(Value::as_object)
        .or_else(|| lock.as_object());
    let mut out = BTreeMap::new();
    if let Some(map) = source {
        for (id, params) in map {
            if let Some(params) = params.as_object() {
                out.insert(
                    id.clone(),
                    params.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                );
            }
        }
    }
    out
}

/// JSON equality with numbers compared by value (a lock's `1`
/// and a spec's `1.0` are the same parameter value).
fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(fx), Some(fy)) => fx == fy,
            _ => x == y,
        },
        (Value::Array(xs), Value::Array(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| json_eq(x, y))
        }
        (Value::Object(xs), Value::Object(ys)) => {
            xs.len() == ys.len()
                && xs
                    .iter()
                    .all(|(k, x)| ys.get(k).is_some_and(|y| json_eq(x, y)))
        }
        _ => a == b,
    }
}
