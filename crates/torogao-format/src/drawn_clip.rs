//! The `torogao-drawn-clip/1` validator and freeze
//! pipeline (drawn-animation spike, 2026-11-27; the
//! format's specification of record is this crate's
//! `FORMATS.md`).
//!
//! A drawn clip is a canonical JSON document naming a
//! stack of raster **tracks** over a matte, each track an
//! exposure table of **drawings** (full-canvas RGBA
//! payloads, stored beside the document as PNG files and
//! named by the SHA-256 of their pixels). Playback is
//! substitution only: the drawing in force at a frame is
//! the latest key at or before it (hold semantics), and
//! the stack composites source-over, bottom track first.
//! Nothing interpolates, ever — that is the drawn grammar
//! the format exists to carry (determination SS3.4-3.5).
//!
//! The freeze discipline is `torogao-motion-clip/2`'s: a
//! *working* document is fluid authoring state; a *frozen*
//! document carries a freeze block whose SHA-256 covers
//! the canonical form of the document without it. Checks
//! run in specification order and stop at the first
//! failure, so the reported error is deterministic.
//! Nothing is repaired, dropped, or clamped — invalid
//! input is a typed [`DrawnError`].

use std::collections::BTreeSet;
use std::fmt;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::jsonfmt::to_d_json;
use crate::limits::INPUT_BYTES_MAX;

/// The format stamp. This constant is the one match site
/// for the token; every comparison goes through it.
pub const DRAWN_CLIP_FORMAT: &str = "torogao-drawn-clip/1";

/// Canvas dimension cap (matches torogao-paint's
/// `CANVAS_DIM_MAX`: a clip the paint core cannot hold is
/// not a clip).
pub const DRAWN_DIM_MAX: u64 = 8192;
/// Drawings per document.
pub const DRAWN_DRAWINGS_MAX: usize = 4096;
/// Tracks per document.
pub const DRAWN_TRACKS_MAX: usize = 256;
/// Keys per track.
pub const DRAWN_KEYS_PER_TRACK_MAX: usize = 100_000;
/// Timeline span cap (`range.to - range.from`).
pub const DRAWN_RANGE_SPAN_MAX: i64 = 100_000;
/// Clip and track name byte caps.
pub const DRAWN_NAME_BYTES_MAX: usize = 64;
pub const DRAWN_TRACK_NAME_BYTES_MAX: usize = 128;

/// A typed refusal from the drawn-clip validator: a
/// stable `TG-DRAW-` code plus a human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawnError {
    code: &'static str,
    message: String,
}

impl DrawnError {
    fn new(code: &'static str, message: impl Into<String>) -> DrawnError {
        DrawnError {
            code,
            message: message.into(),
        }
    }

    /// The stable machine-readable code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The human-readable explanation.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DrawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for DrawnError {}

/// The drawn-clip result type.
pub type DrawnResult<T> = Result<T, DrawnError>;

/// What validation learned about a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawnReport {
    /// The clip's name.
    pub name: String,
    /// Canvas `(width, height)`.
    pub canvas: (u64, u64),
    /// Timeline frames per second.
    pub fps: u64,
    /// Playback range `(from, to)`, inclusive.
    pub range: (i64, i64),
    /// Drawings declared.
    pub drawings: usize,
    /// Tracks declared.
    pub tracks: usize,
    /// Keys across all tracks.
    pub keys: usize,
    /// The freeze hash, when the document is frozen.
    pub freeze_sha256: Option<String>,
}

/// The canonical byte form of a drawn-clip document: the
/// estate canonical JSON writer over the document with
/// its freeze block removed. The freeze hash covers
/// exactly these bytes.
pub fn canonical_bytes(doc: &Value) -> String {
    let mut stripped = doc.clone();
    if let Some(object) = stripped.as_object_mut() {
        object.remove("freeze");
    }
    to_d_json(&stripped)
}

/// The freeze hash of a document: lowercase hex SHA-256
/// of its canonical bytes.
pub fn freeze_hash(doc: &Value) -> String {
    let digest = Sha256::digest(canonical_bytes(doc).as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// The payload path a drawing's `file` field must name
/// in `/1`: derived from the drawing id, never free text,
/// so a document cannot point outside its own directory
/// shape.
pub fn drawing_file_name(id: &str) -> String {
    format!("drawings/{id}.png")
}

/// Validates a drawn-clip document from its bytes. A
/// document carrying a freeze block is held to the
/// frozen rules (hash reproduces); one without is held
/// to the working rules.
pub fn validate_clip(bytes: &[u8]) -> DrawnResult<DrawnReport> {
    if bytes.len() > INPUT_BYTES_MAX {
        return Err(DrawnError::new(
            "TG-DRAW-TOO-LARGE",
            format!(
                "document is {} bytes, over the {INPUT_BYTES_MAX} byte limit",
                bytes.len()
            ),
        ));
    }
    let doc: Value = serde_json::from_slice(bytes).map_err(|error| {
        DrawnError::new("TG-DRAW-NOT-JSON", format!("document is not JSON: {error}"))
    })?;
    validate_document(&doc)
}

/// Validates an already-parsed document (see
/// [`validate_clip`]).
pub fn validate_document(doc: &Value) -> DrawnResult<DrawnReport> {
    let root = doc.as_object().ok_or_else(|| {
        DrawnError::new("TG-DRAW-NOT-OBJECT", "the document root is not an object")
    })?;

    // Closed top-level shape for /1.
    const TOP_LEVEL: [&str; 11] = [
        "format",
        "name",
        "canvas",
        "fps",
        "range",
        "loop",
        "matte",
        "drawings",
        "tracks",
        "provenance",
        "freeze",
    ];
    for key in root.keys() {
        if !TOP_LEVEL.contains(&key.as_str()) {
            return Err(DrawnError::new(
                "TG-DRAW-UNKNOWN-FIELD",
                format!("top-level field {key:?} is not part of {DRAWN_CLIP_FORMAT}"),
            ));
        }
    }

    let format = required_string(root, "format", 64)?;
    if format != DRAWN_CLIP_FORMAT {
        return Err(DrawnError::new(
            "TG-DRAW-FORMAT",
            format!("format is {format:?}, not {DRAWN_CLIP_FORMAT:?}"),
        ));
    }
    let name = required_string(root, "name", DRAWN_NAME_BYTES_MAX)?;
    check_name(&name, "clip name")?;

    let canvas = required_object(root, "canvas")?;
    let width = required_u64(canvas, "width", "canvas")?;
    let height = required_u64(canvas, "height", "canvas")?;
    if width == 0 || width > DRAWN_DIM_MAX || height == 0 || height > DRAWN_DIM_MAX {
        return Err(DrawnError::new(
            "TG-DRAW-CANVAS",
            format!("canvas {width}x{height} is outside 1..={DRAWN_DIM_MAX} per side"),
        ));
    }

    let fps = required_u64(root, "fps", "document")?;
    if fps == 0 || fps > 240 {
        return Err(DrawnError::new(
            "TG-DRAW-FPS",
            format!("fps {fps} is outside 1..=240"),
        ));
    }

    let range = required_object(root, "range")?;
    let from = required_i64(range, "from", "range")?;
    let to = required_i64(range, "to", "range")?;
    if from < 0 || to < from || to - from > DRAWN_RANGE_SPAN_MAX {
        return Err(DrawnError::new(
            "TG-DRAW-RANGE",
            format!(
                "range {from}..={to} is inverted, negative, or spans over {DRAWN_RANGE_SPAN_MAX} frames"
            ),
        ));
    }

    let _loop = required_bool(root, "loop")?;

    // The matte under the stack: four channels, 0..=255.
    let matte = required_array(root, "matte")?;
    if matte.len() != 4
        || matte
            .iter()
            .any(|channel| channel.as_u64().map(|v| v > 255).unwrap_or(true))
    {
        return Err(DrawnError::new(
            "TG-DRAW-MATTE",
            "matte must be an array of four channel values in 0..=255",
        ));
    }

    if let Some(provenance) = root.get("provenance") {
        if !provenance.is_object() {
            return Err(DrawnError::new(
                "TG-DRAW-WRONG-TYPE",
                "provenance is not an object",
            ));
        }
    }

    // Drawings: the payload manifest. An id is derived
    // from its hash (`d-<sha256>`), so a renamed or
    // swapped payload cannot keep a valid name.
    let drawings = required_array(root, "drawings")?;
    if drawings.is_empty() || drawings.len() > DRAWN_DRAWINGS_MAX {
        return Err(DrawnError::new(
            "TG-DRAW-DRAWINGS",
            format!(
                "document declares {} drawings, outside 1..={DRAWN_DRAWINGS_MAX}",
                drawings.len()
            ),
        ));
    }
    let mut drawing_ids = BTreeSet::new();
    for (index, drawing) in drawings.iter().enumerate() {
        let context = format!("drawing {index}");
        let drawing = drawing.as_object().ok_or_else(|| {
            DrawnError::new("TG-DRAW-WRONG-TYPE", format!("{context} is not an object"))
        })?;
        let sha = required_string(drawing, "sha256", 64).map_err(|e| prefix(e, &context))?;
        if !is_lower_hex(&sha, 64) {
            return Err(DrawnError::new(
                "TG-DRAW-DRAWING-HASH",
                format!("{context} sha256 is not 64 lowercase hex digits"),
            ));
        }
        let id = required_string(drawing, "id", 80).map_err(|e| prefix(e, &context))?;
        if id != format!("d-{sha}") {
            return Err(DrawnError::new(
                "TG-DRAW-DRAWING-ID",
                format!("{context} id {id:?} is not derived from its sha256 (d-{sha})"),
            ));
        }
        if !drawing_ids.insert(id.clone()) {
            return Err(DrawnError::new(
                "TG-DRAW-DRAWING-ID",
                format!("{context} duplicates drawing id {id:?}"),
            ));
        }
        let dw = required_u64(drawing, "width", &context)?;
        let dh = required_u64(drawing, "height", &context)?;
        if dw != width || dh != height {
            return Err(DrawnError::new(
                "TG-DRAW-DRAWING-SIZE",
                format!("{context} is {dw}x{dh}; /1 drawings are full-canvas ({width}x{height})"),
            ));
        }
        let file = required_string(drawing, "file", 96).map_err(|e| prefix(e, &context))?;
        if file != drawing_file_name(&id) {
            return Err(DrawnError::new(
                "TG-DRAW-DRAWING-FILE",
                format!(
                    "{context} file {file:?} is not the derived path {:?}",
                    drawing_file_name(&id)
                ),
            ));
        }
    }

    // Tracks: the layer stack, bottom first, each an
    // exposure table over the drawings.
    let tracks = required_array(root, "tracks")?;
    if tracks.is_empty() || tracks.len() > DRAWN_TRACKS_MAX {
        return Err(DrawnError::new(
            "TG-DRAW-TRACKS",
            format!(
                "document declares {} tracks, outside 1..={DRAWN_TRACKS_MAX}",
                tracks.len()
            ),
        ));
    }
    let mut track_names = BTreeSet::new();
    let mut total_keys = 0_usize;
    for (index, track) in tracks.iter().enumerate() {
        let context = format!("track {index}");
        let track = track.as_object().ok_or_else(|| {
            DrawnError::new("TG-DRAW-WRONG-TYPE", format!("{context} is not an object"))
        })?;
        let track_name = required_string(track, "name", DRAWN_TRACK_NAME_BYTES_MAX)
            .map_err(|e| prefix(e, &context))?;
        if !track_names.insert(track_name.clone()) {
            return Err(DrawnError::new(
                "TG-DRAW-TRACK-NAME",
                format!("{context} duplicates track name {track_name:?}"),
            ));
        }
        let blend = required_string(track, "blend", 32).map_err(|e| prefix(e, &context))?;
        if blend != "normal" {
            return Err(DrawnError::new(
                "TG-DRAW-BLEND",
                format!(
                    "{context} ({track_name:?}) names blend {blend:?}; /1 carries normal only — other blends enter the format when their arithmetic is proven against the paint core, not before"
                ),
            ));
        }
        let _opacity = required_u64(track, "opacity", &context)?;
        if _opacity > 255 {
            return Err(DrawnError::new(
                "TG-DRAW-OPACITY",
                format!("{context} opacity {_opacity} is outside 0..=255"),
            ));
        }
        let _visible = required_bool(track, "visible").map_err(|e| prefix(e, &context))?;

        let keys = required_array(track, "keys").map_err(|e| prefix(e, &context))?;
        if keys.is_empty() || keys.len() > DRAWN_KEYS_PER_TRACK_MAX {
            return Err(DrawnError::new(
                "TG-DRAW-KEYS",
                format!(
                    "{context} declares {} keys, outside 1..={DRAWN_KEYS_PER_TRACK_MAX}",
                    keys.len()
                ),
            ));
        }
        let mut previous: Option<i64> = None;
        for (key_index, key) in keys.iter().enumerate() {
            let key_context = format!("{context} key {key_index}");
            let key = key.as_object().ok_or_else(|| {
                DrawnError::new(
                    "TG-DRAW-WRONG-TYPE",
                    format!("{key_context} is not an object"),
                )
            })?;
            let time = required_i64(key, "time", &key_context)?;
            if let Some(prev) = previous {
                if time <= prev {
                    return Err(DrawnError::new(
                        "TG-DRAW-KEY-ORDER",
                        format!(
                            "{key_context} time {time} does not follow {prev}; keys are strictly increasing"
                        ),
                    ));
                }
            }
            previous = Some(time);
            if time > to {
                return Err(DrawnError::new(
                    "TG-DRAW-KEY-RANGE",
                    format!(
                        "{key_context} time {time} is past the range end {to}; dead keys are refused, not carried"
                    ),
                ));
            }
            let drawing_ref =
                required_string(key, "drawing", 80).map_err(|e| prefix(e, &key_context))?;
            if !drawing_ids.contains(&drawing_ref) {
                return Err(DrawnError::new(
                    "TG-DRAW-KEY-DRAWING",
                    format!("{key_context} names undeclared drawing {drawing_ref:?}"),
                ));
            }
        }
        // Hold coverage: some key must be in force at the
        // range's first frame.
        let first_time = required_i64(
            keys[0].as_object().expect("checked above"),
            "time",
            &context,
        )?;
        if first_time > from {
            return Err(DrawnError::new(
                "TG-DRAW-KEY-COVERAGE",
                format!(
                    "{context} ({track_name:?}) first key is at {first_time}, after the range start {from}; frames before it would have no drawing in force"
                ),
            ));
        }
        total_keys += keys.len();
    }

    // Freeze block, when present: shape + hash.
    let mut freeze_sha256 = None;
    if let Some(freeze) = root.get("freeze") {
        let freeze = freeze
            .as_object()
            .ok_or_else(|| DrawnError::new("TG-DRAW-WRONG-TYPE", "freeze is not an object"))?;
        let _band = required_string(freeze, "band", 128).map_err(|e| prefix(e, "freeze"))?;
        let _compiler =
            required_string(freeze, "compiler", 128).map_err(|e| prefix(e, "freeze"))?;
        let sha = required_string(freeze, "sha256", 64).map_err(|e| prefix(e, "freeze"))?;
        if !is_lower_hex(&sha, 64) {
            return Err(DrawnError::new(
                "TG-DRAW-FREEZE-HASH",
                "freeze sha256 is not 64 lowercase hex digits",
            ));
        }
        let actual = freeze_hash(doc);
        if sha != actual {
            return Err(DrawnError::new(
                "TG-DRAW-FREEZE-HASH",
                format!("freeze sha256 is {sha}, the document hashes to {actual}"),
            ));
        }
        freeze_sha256 = Some(sha);
    }

    Ok(DrawnReport {
        name,
        canvas: (width, height),
        fps,
        range: (from, to),
        drawings: drawings.len(),
        tracks: tracks.len(),
        keys: total_keys,
        freeze_sha256,
    })
}

/// Freezes a working document: validates it and returns
/// the document with its freeze block inserted. The
/// input is not mutated.
pub fn freeze_document(doc: &Value, compiler: &str, band: &str) -> DrawnResult<Value> {
    let root = doc.as_object().ok_or_else(|| {
        DrawnError::new("TG-DRAW-NOT-OBJECT", "the document root is not an object")
    })?;
    if root.contains_key("freeze") {
        return Err(DrawnError::new(
            "TG-DRAW-ALREADY-FROZEN",
            "the document already carries a freeze block; edit a thawed copy",
        ));
    }
    validate_document(doc)?;
    let mut frozen_doc = doc.clone();
    let hash = freeze_hash(&frozen_doc);
    let freeze = serde_json::json!({
        "band": band,
        "compiler": compiler,
        "sha256": hash,
    });
    frozen_doc
        .as_object_mut()
        .expect("validated as an object")
        .insert("freeze".to_string(), freeze);
    // The frozen form must pass its own validation.
    validate_document(&frozen_doc)?;
    Ok(frozen_doc)
}

fn prefix(error: DrawnError, context: &str) -> DrawnError {
    DrawnError::new(error.code, format!("{context}: {}", error.message))
}

fn is_lower_hex(text: &str, digits: usize) -> bool {
    text.len() == digits
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn check_name(name: &str, what: &str) -> DrawnResult<()> {
    let ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        && name
            .bytes()
            .next()
            .map(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            .unwrap_or(false);
    if !ok {
        return Err(DrawnError::new(
            "TG-DRAW-NAME",
            format!(
                "{what} {name:?} must be lowercase ASCII letters, digits, '-' or '_', starting with a letter or digit"
            ),
        ));
    }
    Ok(())
}

fn required_object<'a>(
    parent: &'a serde_json::Map<String, Value>,
    field: &str,
) -> DrawnResult<&'a serde_json::Map<String, Value>> {
    match parent.get(field) {
        Some(Value::Object(object)) => Ok(object),
        Some(_) => Err(DrawnError::new(
            "TG-DRAW-WRONG-TYPE",
            format!("{field} is not an object"),
        )),
        None => Err(DrawnError::new(
            "TG-DRAW-MISSING-FIELD",
            format!("required field {field} is missing"),
        )),
    }
}

fn required_array<'a>(
    parent: &'a serde_json::Map<String, Value>,
    field: &str,
) -> DrawnResult<&'a Vec<Value>> {
    match parent.get(field) {
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(DrawnError::new(
            "TG-DRAW-WRONG-TYPE",
            format!("{field} is not an array"),
        )),
        None => Err(DrawnError::new(
            "TG-DRAW-MISSING-FIELD",
            format!("required field {field} is missing"),
        )),
    }
}

fn required_string(
    parent: &serde_json::Map<String, Value>,
    field: &str,
    max_bytes: usize,
) -> DrawnResult<String> {
    match parent.get(field) {
        Some(Value::String(text)) => {
            if text.is_empty() {
                Err(DrawnError::new(
                    "TG-DRAW-MISSING-FIELD",
                    format!("{field} is empty"),
                ))
            } else if text.len() > max_bytes {
                Err(DrawnError::new(
                    "TG-DRAW-FIELD-TOO-LONG",
                    format!(
                        "{field} is {} bytes, over the {max_bytes} limit",
                        text.len()
                    ),
                ))
            } else {
                Ok(text.clone())
            }
        }
        Some(_) => Err(DrawnError::new(
            "TG-DRAW-WRONG-TYPE",
            format!("{field} is not a string"),
        )),
        None => Err(DrawnError::new(
            "TG-DRAW-MISSING-FIELD",
            format!("required field {field} is missing"),
        )),
    }
}

fn required_u64(
    parent: &serde_json::Map<String, Value>,
    field: &str,
    context: &str,
) -> DrawnResult<u64> {
    match parent.get(field) {
        Some(value) => value.as_u64().ok_or_else(|| {
            DrawnError::new(
                "TG-DRAW-WRONG-TYPE",
                format!("{context} field {field} is not a non-negative integer"),
            )
        }),
        None => Err(DrawnError::new(
            "TG-DRAW-MISSING-FIELD",
            format!("{context} required field {field} is missing"),
        )),
    }
}

fn required_i64(
    parent: &serde_json::Map<String, Value>,
    field: &str,
    context: &str,
) -> DrawnResult<i64> {
    match parent.get(field) {
        Some(value) => value.as_i64().ok_or_else(|| {
            DrawnError::new(
                "TG-DRAW-WRONG-TYPE",
                format!("{context} field {field} is not an integer"),
            )
        }),
        None => Err(DrawnError::new(
            "TG-DRAW-MISSING-FIELD",
            format!("{context} required field {field} is missing"),
        )),
    }
}

fn required_bool(parent: &serde_json::Map<String, Value>, field: &str) -> DrawnResult<bool> {
    match parent.get(field) {
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(DrawnError::new(
            "TG-DRAW-WRONG-TYPE",
            format!("{field} is not a boolean"),
        )),
        None => Err(DrawnError::new(
            "TG-DRAW-MISSING-FIELD",
            format!("required field {field} is missing"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn working_doc() -> Value {
        serde_json::json!({
            "format": DRAWN_CLIP_FORMAT,
            "name": "test-blink",
            "canvas": {"width": 64, "height": 64},
            "fps": 24,
            "range": {"from": 0, "to": 3},
            "loop": true,
            "matte": [255, 255, 255, 255],
            "drawings": [
                {"id": format!("d-{SHA_A}"), "sha256": SHA_A, "width": 64, "height": 64, "file": format!("drawings/d-{SHA_A}.png")},
                {"id": format!("d-{SHA_B}"), "sha256": SHA_B, "width": 64, "height": 64, "file": format!("drawings/d-{SHA_B}.png")}
            ],
            "tracks": [
                {"name": "base", "blend": "normal", "opacity": 255, "visible": true,
                 "keys": [{"time": 0, "drawing": format!("d-{SHA_A}")}]},
                {"name": "anim/blink", "blend": "normal", "opacity": 255, "visible": true,
                 "keys": [{"time": 0, "drawing": format!("d-{SHA_B}")}, {"time": 2, "drawing": format!("d-{SHA_A}")}]}
            ]
        })
    }

    #[test]
    fn validates_and_freezes() {
        let doc = working_doc();
        let report = validate_document(&doc).expect("working doc validates");
        assert_eq!(report.keys, 3);
        assert!(report.freeze_sha256.is_none());
        let frozen = freeze_document(&doc, "test-compiler", "test-band").expect("freezes");
        let report = validate_document(&frozen).expect("frozen doc validates");
        assert!(report.freeze_sha256.is_some());
        // A tampered frozen document refuses.
        let mut tampered = frozen.clone();
        tampered["fps"] = serde_json::json!(12);
        let error = validate_document(&tampered).expect_err("tamper refuses");
        assert_eq!(error.code(), "TG-DRAW-FREEZE-HASH");
    }

    #[test]
    fn refuses_a_blend_beyond_normal() {
        let mut doc = working_doc();
        doc["tracks"][0]["blend"] = serde_json::json!("multiply");
        let error = validate_document(&doc).expect_err("blend refuses");
        assert_eq!(error.code(), "TG-DRAW-BLEND");
    }

    #[test]
    fn refuses_a_key_past_the_range() {
        let mut doc = working_doc();
        doc["tracks"][1]["keys"][1]["time"] = serde_json::json!(9);
        let error = validate_document(&doc).expect_err("dead key refuses");
        assert_eq!(error.code(), "TG-DRAW-KEY-RANGE");
    }

    #[test]
    fn refuses_uncovered_range_start() {
        let mut doc = working_doc();
        doc["tracks"][0]["keys"][0]["time"] = serde_json::json!(1);
        let error = validate_document(&doc).expect_err("coverage refuses");
        assert_eq!(error.code(), "TG-DRAW-KEY-COVERAGE");
    }

    #[test]
    fn refuses_an_underived_drawing_id() {
        let mut doc = working_doc();
        doc["drawings"][0]["id"] = serde_json::json!("d-0000");
        let error = validate_document(&doc).expect_err("id refuses");
        assert_eq!(error.code(), "TG-DRAW-DRAWING-ID");
    }
}
