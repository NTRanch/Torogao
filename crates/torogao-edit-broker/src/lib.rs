//! torogao-edit-broker — the contract core of the edit-route
//! job broker (Torogao desktop tranche T2).
//!
//! The broker is one primo service that owns the ComfyUI edit
//! queue. Lanes submit edit jobs to the broker and poll them;
//! the broker runs them FIFO, one at a time, by driving
//! `edit-local.py` (the primo-local edit route: Qwen-Image-2.1
//! instruction edits on ComfyUI :8189) as a subprocess per job.
//! This replaces the de facto protocol the route's record
//! documents from 2026-11-27: lanes running waiter scripts that
//! poll for an empty ComfyUI queue and race to fire into idle
//! gaps.
//!
//! The broker implements the estate's facade engine contract —
//! the same surface the torogao-gen seat implements and the
//! governed facade (`torogao-mobile-facade`) speaks:
//! `GET /health`, `POST /jobs`, `GET /jobs/<id>`, `GET /jobs`,
//! `POST /jobs/<id>/interrupt`. The facade-side registration of
//! this engine (a second engine behind the facade, with the
//! edit recipe bounds added to its governance list) is a
//! separate extension; this crate is the engine side of the
//! seam, built to be registered without changes.
//!
//! This library is the pure half: the job document's validation,
//! the driver argv construction, and artefact collection. All
//! of it is testable without a GPU or a ComfyUI server. The
//! binary (`src/main.rs`) owns the sockets, the queue, and the
//! driver subprocesses.
//!
//! Job document (the `POST /jobs` body):
//!
//! ```json
//! {
//!   "canvas_png_base64": "<the canvas to edit, PNG>",
//!   "prompt": "<the instruction, as text>",
//!   "config": "faithful" | "turbo",
//!   "seed": 2701,
//!   "mask": { "kind": "explicit", "mask_png_base64": "<PNG>",
//!             "feather": 8 }
//!         | { "kind": "auto", "feather": 8,
//!             "threshold": 28, "dilate": 6 }
//! }
//! ```
//!
//! - `config` defaults to `faithful` — no LoRA, 20 steps,
//!   cfg 4.0; the proof-gated, chain-safe configuration of
//!   record. `turbo` is offered only under its label (fast,
//!   NOT chain-safe; see [`EditConfig::label`]). `omni` is
//!   refused by name: the Omni LoRA's proof verdict
//!   (primo-local-edit-route RECORD.md, 2026-11-27) is that it
//!   re-composes the frame instead of editing it, so this
//!   engine does not offer it at any strength.
//! - `mask` is optional. Its presence asks the driver to
//!   composite: outside the mask the delivered frame is the
//!   canvas's pixels exactly. `explicit` takes the caller's
//!   mask image (white = take the edit); `auto` derives the
//!   region from the canvas/edit pixel diff. The two kinds'
//!   fields are mutually exclusive, mirroring the driver's own
//!   refusal of `--mask` with `--auto-mask`.
//! - Unknown fields are refused, at the top level and inside
//!   `mask`: the contract is explicit, and a silently ignored
//!   field is a silent no-op the caller cannot see.
//!
//! Bounds are named constants below; every one is enforced at
//! submit, before any file is written or any job is queued.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Largest `POST /jobs` body accepted, in bytes. Edit jobs
/// carry a PNG canvas (and optionally a mask) inline as
/// base64, so the gen seat's 256 KiB cap cannot apply here;
/// 64 MiB bounds two 32 MiB images plus the prompt with room
/// to spare. Beyond → 413.
pub const BODY_BYTES_MAX: usize = 64 * 1024 * 1024;

/// Largest decoded canvas or mask image accepted, in bytes.
pub const IMAGE_BYTES_MAX: usize = 32 * 1024 * 1024;

/// Largest instruction prompt accepted, in characters. An
/// engine-side sanity bound; the facade's own prompt cap is
/// facade governance and applies upstream, unchanged.
pub const PROMPT_CHARS_MAX: usize = 16_384;

/// The seed used when a job does not name one: the driver's
/// own default (2701), named here so the broker never invents
/// a seed silently. The resolved seed is always reported in
/// the job's state and always passed to the driver explicitly.
pub const SEED_DEFAULT: u64 = 2701;

/// Compositing defaults and bounds — the driver's defaults,
/// restated so the broker pins them in argv instead of
/// depending on the driver's defaults never drifting.
pub const FEATHER_PX_DEFAULT: u64 = 8;
pub const FEATHER_PX_MAX: u64 = 64;
pub const MASK_THRESHOLD_DEFAULT: u64 = 28;
pub const MASK_THRESHOLD_MAX: u64 = 255;
pub const MASK_DILATE_PX_DEFAULT: u64 = 6;
pub const MASK_DILATE_PX_MAX: u64 = 64;

/// A submit-time refusal: the job document is invalid and no
/// job was created. The message is part of the contract —
/// callers (and tests) read the named reason out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refusal {}

fn refuse(message: impl Into<String>) -> Refusal {
    Refusal(message.into())
}

/// The named edit configurations this engine offers. The
/// driver's third configuration, Omni, is deliberately not a
/// variant: it is refused at parse time with its verdict
/// named, so it cannot be selected by accident or by typo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditConfig {
    /// No LoRA, 20 steps, cfg 4.0. The default, and the only
    /// chain-safe configuration.
    Faithful,
    /// chatrlabs omni-turbo LoRA, 8 steps, cfg 1.0. Fast, and
    /// labelled not-chain-safe wherever it appears.
    Turbo,
}

impl EditConfig {
    /// The wire name, as submitted in `config` and reported
    /// back in the job's state.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Faithful => "faithful",
            Self::Turbo => "turbo",
        }
    }

    /// The label that travels with the config everywhere it
    /// is reported. Turbo's label carries both its standing
    /// warning and the licence fence, so a turbo output can
    /// never be mistaken for a faithful one downstream.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Faithful => {
                "faithful — no LoRA, 20 steps, cfg 4.0; \
                 the chain-safe default of record"
            }
            Self::Turbo => {
                "turbo — 8 steps, cfg 1.0, NOT chain-safe \
                 (small props drift; never chain from its \
                 outputs without a verdict read); Qwen \
                 Research License fence: non-commercial \
                 internal use only"
            }
        }
    }
}

/// A validated compositing request. The variants carry
/// exactly the fields their kind uses; fields belonging to
/// the other kind are refused at parse time rather than
/// silently ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskDocument {
    /// The caller's mask image (white = take the edit).
    Explicit {
        /// The decoded mask PNG bytes.
        mask_png: Vec<u8>,
        /// Feather radius in pixels at the region boundary.
        feather_px: u64,
    },
    /// The region derived from the canvas/edit pixel diff.
    Auto {
        /// Feather radius in pixels at the region boundary.
        feather_px: u64,
        /// Max-channel |canvas − edit| above which a pixel is
        /// in the region, 0–255.
        threshold: u64,
        /// Dilation of the derived region, in pixels.
        dilate_px: u64,
    },
}

/// A compositing request with the image bytes set aside:
/// the shape the driver argv is built from once the job's
/// files are staged on disk. Produced by
/// [`MaskDocument::shape`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskShape {
    /// The caller's mask image, staged beside the canvas.
    Explicit {
        /// Feather radius in pixels at the region boundary.
        feather_px: u64,
    },
    /// The region derived from the canvas/edit pixel diff.
    Auto {
        /// Feather radius in pixels at the region boundary.
        feather_px: u64,
        /// Max-channel |canvas − edit| above which a pixel is
        /// in the region, 0–255.
        threshold: u64,
        /// Dilation of the derived region, in pixels.
        dilate_px: u64,
    },
}

impl MaskDocument {
    /// This mask with its image bytes set aside.
    pub fn shape(&self) -> MaskShape {
        match self {
            Self::Explicit { feather_px, .. } => MaskShape::Explicit {
                feather_px: *feather_px,
            },
            Self::Auto {
                feather_px,
                threshold,
                dilate_px,
            } => MaskShape::Auto {
                feather_px: *feather_px,
                threshold: *threshold,
                dilate_px: *dilate_px,
            },
        }
    }
}

/// A validated edit job, ready to be written to disk and
/// queued. Produced only by [`parse_job_document`]; the
/// fields are the whole contract, so a `JobDocument` in hand
/// is a job the driver can be asked to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobDocument {
    /// The decoded canvas PNG bytes.
    pub canvas_png: Vec<u8>,
    /// The instruction text, exactly as submitted.
    pub prompt: String,
    /// The named configuration.
    pub config: EditConfig,
    /// The resolved seed (submitted, or [`SEED_DEFAULT`]).
    pub seed: u64,
    /// The compositing request, if any.
    pub mask: Option<MaskDocument>,
}

/// Parse and validate a `POST /jobs` body into a
/// [`JobDocument`]. Rejection leaves no trace: no job, no
/// files, no queue entry.
///
/// Accepts exactly the fields documented at the crate root.
/// Refuses, by name: a non-object body, unknown fields,
/// missing or mistyped fields, undecodable base64, non-PNG
/// images, oversized images or prompt, out-of-bound mask
/// numbers, fields belonging to the other mask kind, and
/// `config: "omni"`.
pub fn parse_job_document(body: &Value) -> Result<JobDocument, Refusal> {
    let object = body
        .as_object()
        .ok_or_else(|| refuse("job body must be a JSON object"))?;
    refuse_unknown_fields(
        object.keys().map(String::as_str),
        &["canvas_png_base64", "prompt", "config", "seed", "mask"],
        "job",
    )?;

    let canvas_png = parse_required_image_field(object, "canvas_png_base64")?;
    let prompt = match object.get("prompt") {
        Some(Value::String(text)) if !text.trim().is_empty() => text.clone(),
        Some(Value::String(_)) => return Err(refuse("prompt must not be empty")),
        Some(_) => return Err(refuse("prompt must be a string")),
        None => return Err(refuse("missing string field prompt")),
    };
    if prompt.chars().count() > PROMPT_CHARS_MAX {
        return Err(refuse(format!(
            "prompt is {} characters, over the {PROMPT_CHARS_MAX} character bound",
            prompt.chars().count()
        )));
    }
    let config = parse_config(object.get("config"))?;
    let seed = match object.get("seed") {
        None => SEED_DEFAULT,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| refuse("seed must be a non-negative integer"))?,
    };
    let mask = match object.get("mask") {
        None => None,
        Some(value) => Some(parse_mask(value)?),
    };
    Ok(JobDocument {
        canvas_png,
        prompt,
        config,
        seed,
        mask,
    })
}

fn parse_config(value: Option<&Value>) -> Result<EditConfig, Refusal> {
    match value {
        None => Ok(EditConfig::Faithful),
        Some(Value::String(name)) => match name.as_str() {
            "faithful" => Ok(EditConfig::Faithful),
            "turbo" => Ok(EditConfig::Turbo),
            "omni" => Err(refuse(
                "config \"omni\" is not offered by this engine: its proof \
                 verdict (primo-local-edit-route RECORD.md, 2026-11-27) is \
                 that it re-composes the frame instead of editing it — \
                 identity, pose, and props re-rolled. It remains in the \
                 driver for hand use; the broker refuses it.",
            )),
            other => Err(refuse(format!(
                "unknown config {other:?}: this engine offers \"faithful\" \
                 (default) and \"turbo\" (labelled not-chain-safe)"
            ))),
        },
        Some(_) => Err(refuse("config must be a string")),
    }
}

fn parse_mask(value: &Value) -> Result<MaskDocument, Refusal> {
    let object = value
        .as_object()
        .ok_or_else(|| refuse("mask must be a JSON object"))?;
    refuse_unknown_fields(
        object.keys().map(String::as_str),
        &["kind", "mask_png_base64", "feather", "threshold", "dilate"],
        "mask",
    )?;
    let kind = match object.get("kind") {
        Some(Value::String(kind)) => kind.as_str(),
        Some(_) => return Err(refuse("mask.kind must be a string")),
        None => return Err(refuse("missing string field mask.kind")),
    };
    let feather_px = parse_bounded_u64(object, "feather", FEATHER_PX_DEFAULT, FEATHER_PX_MAX)?;
    match kind {
        "explicit" => {
            refuse_present(object, "threshold", "explicit masks take no threshold")?;
            refuse_present(object, "dilate", "explicit masks take no dilate")?;
            let mask_png = parse_required_image_field(object, "mask_png_base64")?;
            Ok(MaskDocument::Explicit {
                mask_png,
                feather_px,
            })
        }
        "auto" => {
            refuse_present(
                object,
                "mask_png_base64",
                "auto masks derive the region from the edit; a supplied \
                 mask image belongs to kind \"explicit\"",
            )?;
            let threshold = parse_bounded_u64(
                object,
                "threshold",
                MASK_THRESHOLD_DEFAULT,
                MASK_THRESHOLD_MAX,
            )?;
            let dilate_px =
                parse_bounded_u64(object, "dilate", MASK_DILATE_PX_DEFAULT, MASK_DILATE_PX_MAX)?;
            Ok(MaskDocument::Auto {
                feather_px,
                threshold,
                dilate_px,
            })
        }
        other => Err(refuse(format!(
            "unknown mask.kind {other:?}: expected \"explicit\" or \"auto\""
        ))),
    }
}

/// Refuse a field that is present but belongs to another
/// variant of the contract: accepting it would make it a
/// silent no-op.
fn refuse_present(
    object: &serde_json::Map<String, Value>,
    field: &str,
    why: &str,
) -> Result<(), Refusal> {
    if object.contains_key(field) {
        return Err(refuse(format!("mask.{field} refused: {why}")));
    }
    Ok(())
}

fn refuse_unknown_fields<'a>(
    fields: impl Iterator<Item = &'a str>,
    allowed: &[&str],
    scope: &str,
) -> Result<(), Refusal> {
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    for field in fields {
        if !allowed.contains(field) {
            return Err(refuse(format!(
                "unknown field {field:?} in {scope}: the contract is \
                 explicit and a silently ignored field is a silent no-op"
            )));
        }
    }
    Ok(())
}

/// Parse a required base64 PNG image field: refuses when
/// absent; the decoded bytes are size-bounded and must carry
/// the PNG signature.
fn parse_required_image_field(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Vec<u8>, Refusal> {
    let Some(value) = object.get(field) else {
        return Err(refuse(format!("missing string field {field}")));
    };
    let Some(text) = value.as_str() else {
        return Err(refuse(format!("{field} must be a base64 string")));
    };
    let bytes =
        base64_decode(text).map_err(|e| refuse(format!("{field} is not valid base64: {e}")))?;
    if bytes.len() > IMAGE_BYTES_MAX {
        return Err(refuse(format!(
            "{field} decodes to {} bytes, over the {IMAGE_BYTES_MAX} byte bound",
            bytes.len()
        )));
    }
    if !is_png(&bytes) {
        return Err(refuse(format!(
            "{field} does not start with the PNG signature"
        )));
    }
    Ok(bytes)
}

fn parse_bounded_u64(
    object: &serde_json::Map<String, Value>,
    field: &str,
    default: u64,
    max: u64,
) -> Result<u64, Refusal> {
    match object.get(field) {
        None => Ok(default),
        Some(value) => {
            let number = value
                .as_u64()
                .ok_or_else(|| refuse(format!("mask.{field} must be a non-negative integer")))?;
            if number > max {
                return Err(refuse(format!(
                    "mask.{field} is {number}, over the bound of {max}"
                )));
            }
            Ok(number)
        }
    }
}

/// The PNG file signature: the one format check the broker
/// performs on image bytes. Full decode is the driver's job;
/// this refuses the common wrong-artefact submission (a JPEG,
/// a text file, a path) at submit, with a named reason.
pub fn is_png(bytes: &[u8]) -> bool {
    const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.len() >= PNG_SIGNATURE.len() && bytes[..PNG_SIGNATURE.len()] == PNG_SIGNATURE
}

/// Build the driver's argv for one job (excluding the python
/// interpreter and the driver path, which the caller
/// prepends). The resolved seed and the resolved mask numbers
/// are always passed explicitly, so a job's behaviour never
/// depends on the driver's defaults drifting. Turbo passes
/// exactly `--turbo`; `--omni` can never appear, because
/// [`EditConfig`] has no such variant.
pub fn edit_argv(
    canvas: &Path,
    prompt_file: &Path,
    prefix: &str,
    config: EditConfig,
    seed: u64,
    mask: Option<&MaskShape>,
    mask_file: Option<&Path>,
) -> Vec<OsString> {
    let mut argv: Vec<OsString> = vec![
        OsString::from("edit"),
        canvas.into(),
        prompt_file.into(),
        OsString::from(prefix),
    ];
    if config == EditConfig::Turbo {
        argv.push(OsString::from("--turbo"));
    }
    argv.push(OsString::from("--seed"));
    argv.push(OsString::from(seed.to_string()));
    match mask {
        Some(MaskShape::Explicit { feather_px }) => {
            let mask_path = mask_file.expect("an explicit mask job carries a mask file");
            argv.push(OsString::from("--mask"));
            argv.push(mask_path.into());
            argv.push(OsString::from("--feather"));
            argv.push(OsString::from(feather_px.to_string()));
        }
        Some(MaskShape::Auto {
            feather_px,
            threshold,
            dilate_px,
        }) => {
            argv.push(OsString::from("--auto-mask"));
            argv.push(OsString::from("--feather"));
            argv.push(OsString::from(feather_px.to_string()));
            argv.push(OsString::from("--mask-threshold"));
            argv.push(OsString::from(threshold.to_string()));
            argv.push(OsString::from("--mask-dilate"));
            argv.push(OsString::from(dilate_px.to_string()));
        }
        None => {}
    }
    argv
}

/// The artefact kinds a finished edit job produces. The raw
/// edit is always produced; the composited frame and its
/// alpha exist exactly when the job requested compositing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtefactKind {
    /// The edit exactly as the model returned it.
    RawEdit,
    /// The edit composited onto the canvas under the mask:
    /// outside the mask, the canvas's pixels exactly.
    Composited,
    /// The final feathered alpha the composite used.
    MaskAlpha,
}

impl ArtefactKind {
    /// The wire name reported in the job's artefact list.
    pub fn name(&self) -> &'static str {
        match self {
            Self::RawEdit => "raw_edit",
            Self::Composited => "composited",
            Self::MaskAlpha => "mask_alpha",
        }
    }

    /// Deterministic report order: raw, composited, mask.
    fn rank(&self) -> u8 {
        match self {
            Self::RawEdit => 0,
            Self::Composited => 1,
            Self::MaskAlpha => 2,
        }
    }
}

/// One output file of a finished job, with the hash a caller
/// verifies it by. Artefacts are hash-addressed in the
/// estate's discipline: the sha256 is computed at collection
/// time, from the bytes on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artefact {
    /// The file name within the route's output directory.
    pub name: String,
    /// Which output this is.
    pub kind: ArtefactKind,
    /// The full path the broker reads the bytes from.
    pub path: PathBuf,
    /// Lowercase hex sha256 of the file's bytes.
    pub sha256: String,
    /// The file's length in bytes.
    pub bytes: u64,
}

/// Collect a job's artefacts from the route's output
/// directory by its unique prefix. The driver writes
/// `<prefix>_NNNNN_.png` (raw), `<prefix>_comp.png`
/// (composited), and `<prefix>_mask.png` (alpha). Files not
/// matching the prefix's own names are ignored; a file is
/// classified by exact name for comp/mask and by the
/// `<prefix>_` stem for the raw counter-suffixed output.
pub fn collect_artefacts(output_dir: &Path, prefix: &str) -> std::io::Result<Vec<Artefact>> {
    let comp_name = format!("{prefix}_comp.png");
    let mask_name = format!("{prefix}_mask.png");
    let raw_stem = format!("{prefix}_");
    let mut artefacts = Vec::new();
    for entry in std::fs::read_dir(output_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = if name == comp_name {
            ArtefactKind::Composited
        } else if name == mask_name {
            ArtefactKind::MaskAlpha
        } else if name.starts_with(&raw_stem) && name.ends_with(".png") {
            ArtefactKind::RawEdit
        } else {
            continue;
        };
        let bytes = std::fs::read(entry.path())?;
        artefacts.push(Artefact {
            name,
            kind,
            path: entry.path(),
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
        });
    }
    artefacts.sort_by(|a, b| (a.kind.rank(), &a.name).cmp(&(b.kind.rank(), &b.name)));
    Ok(artefacts)
}

/// The artefact a poll reports as the job's delivered frame:
/// the composited frame when compositing ran, else the raw
/// edit. `None` when the job produced no frame at all.
pub fn delivered_artefact(artefacts: &[Artefact]) -> Option<&Artefact> {
    artefacts
        .iter()
        .find(|a| a.kind == ArtefactKind::Composited)
        .or_else(|| artefacts.iter().find(|a| a.kind == ArtefactKind::RawEdit))
}

/// Lowercase hex sha256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Minimal base64 encoder (standard alphabet, padded) — the
/// loopback contract carries PNGs inline, as the gen seat's
/// does.
pub fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Strict base64 decoder: standard alphabet, canonical
/// padding, no whitespace. Anything else is refused with the
/// position of the offending character, so a malformed
/// submission fails at submit with a named reason.
pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    fn value_of(byte: u8, position: usize) -> Result<u32, String> {
        match byte {
            b'A'..=b'Z' => Ok(u32::from(byte - b'A')),
            b'a'..=b'z' => Ok(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Ok(u32::from(byte - b'0') + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!(
                "character {:?} at position {position} is not in the base64 alphabet",
                byte as char
            )),
        }
    }
    let bytes = input.as_bytes();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return Err(format!(
            "length {} is not a positive multiple of 4",
            bytes.len()
        ));
    }
    let padding = match bytes.len() {
        len if bytes[len - 2] == b'=' && bytes[len - 1] == b'=' => 2,
        len if bytes[len - 1] == b'=' => 1,
        _ => 0,
    };
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3 - padding);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let is_last = (index + 1) * 4 == bytes.len();
        let mut values = [0u32; 4];
        for (slot, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                if !is_last || slot < 4 - padding {
                    return Err(format!(
                        "'=' at position {} is not trailing padding",
                        index * 4 + slot
                    ));
                }
                values[slot] = 0;
            } else {
                values[slot] = value_of(*byte, index * 4 + slot)?;
            }
        }
        let n = (values[0] << 18) | (values[1] << 12) | (values[2] << 6) | values[3];
        out.push((n >> 16) as u8);
        if !(is_last && padding >= 2) {
            out.push((n >> 8) as u8);
        }
        if !(is_last && padding >= 1) {
            out.push(n as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A minimal "PNG" for validation purposes: the broker
    /// checks the signature only, so the fixture is the
    /// signature plus a payload tag.
    fn png(tag: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(tag);
        bytes
    }

    fn minimal_body() -> Value {
        json!({
            "canvas_png_base64": base64_encode(&png(b"canvas")),
            "prompt": "widen the straps",
        })
    }

    // -- Validation: the happy paths -------------------------------------

    #[test]
    fn minimal_document_takes_the_faithful_defaults() {
        let document = parse_job_document(&minimal_body()).expect("minimal body parses");
        assert_eq!(document.config, EditConfig::Faithful);
        assert_eq!(document.seed, SEED_DEFAULT);
        assert_eq!(document.mask, None);
        assert_eq!(document.prompt, "widen the straps");
    }

    #[test]
    fn turbo_and_explicit_seed_are_accepted() {
        let mut body = minimal_body();
        body["config"] = json!("turbo");
        body["seed"] = json!(42);
        let document = parse_job_document(&body).expect("turbo body parses");
        assert_eq!(document.config, EditConfig::Turbo);
        assert_eq!(document.seed, 42);
    }

    #[test]
    fn explicit_mask_takes_its_own_defaults() {
        let mut body = minimal_body();
        body["mask"] = json!({
            "kind": "explicit",
            "mask_png_base64": base64_encode(&png(b"mask")),
        });
        let document = parse_job_document(&body).expect("explicit mask parses");
        assert_eq!(
            document.mask,
            Some(MaskDocument::Explicit {
                mask_png: png(b"mask"),
                feather_px: FEATHER_PX_DEFAULT,
            })
        );
    }

    #[test]
    fn auto_mask_takes_its_own_defaults() {
        let mut body = minimal_body();
        body["mask"] = json!({ "kind": "auto" });
        let document = parse_job_document(&body).expect("auto mask parses");
        assert_eq!(
            document.mask,
            Some(MaskDocument::Auto {
                feather_px: FEATHER_PX_DEFAULT,
                threshold: MASK_THRESHOLD_DEFAULT,
                dilate_px: MASK_DILATE_PX_DEFAULT,
            })
        );
    }

    // -- Adversarial: the refusals ---------------------------------------

    #[test]
    fn omni_is_refused_by_name_with_its_verdict() {
        let mut body = minimal_body();
        body["config"] = json!("omni");
        let refusal = parse_job_document(&body).expect_err("omni must be refused");
        assert!(refusal.0.contains("not offered"), "{refusal}");
        assert!(refusal.0.contains("re-composes"), "{refusal}");
    }

    #[test]
    fn unknown_config_is_refused() {
        let mut body = minimal_body();
        body["config"] = json!("faithfull");
        let refusal = parse_job_document(&body).expect_err("typo config must be refused");
        assert!(refusal.0.contains("unknown config"), "{refusal}");
    }

    #[test]
    fn unknown_top_level_field_is_refused() {
        let mut body = minimal_body();
        body["strength"] = json!(0.5);
        let refusal = parse_job_document(&body).expect_err("unknown field must be refused");
        assert!(refusal.0.contains("unknown field"), "{refusal}");
    }

    #[test]
    fn missing_canvas_is_refused() {
        let body = json!({ "prompt": "widen the straps" });
        let refusal = parse_job_document(&body).expect_err("missing canvas must be refused");
        assert!(refusal.0.contains("canvas_png_base64"), "{refusal}");
    }

    #[test]
    fn non_png_canvas_is_refused() {
        let body = json!({
            "canvas_png_base64": base64_encode(b"this is a jpeg, probably"),
            "prompt": "widen the straps",
        });
        let refusal = parse_job_document(&body).expect_err("non-PNG must be refused");
        assert!(refusal.0.contains("PNG signature"), "{refusal}");
    }

    #[test]
    fn undecodable_canvas_is_refused() {
        let body = json!({
            "canvas_png_base64": "not!base64",
            "prompt": "widen the straps",
        });
        assert!(parse_job_document(&body).is_err());
    }

    #[test]
    fn empty_and_oversized_prompts_are_refused() {
        let mut body = minimal_body();
        body["prompt"] = json!("   ");
        assert!(parse_job_document(&body).is_err());
        body["prompt"] = json!("x".repeat(PROMPT_CHARS_MAX + 1));
        assert!(parse_job_document(&body).is_err());
    }

    #[test]
    fn negative_seed_is_refused() {
        let mut body = minimal_body();
        body["seed"] = json!(-1);
        assert!(parse_job_document(&body).is_err());
    }

    #[test]
    fn auto_mask_with_a_supplied_image_is_refused() {
        let mut body = minimal_body();
        body["mask"] = json!({
            "kind": "auto",
            "mask_png_base64": base64_encode(&png(b"mask")),
        });
        let refusal = parse_job_document(&body).expect_err("mixed mask must be refused");
        assert!(refusal.0.contains("mask_png_base64"), "{refusal}");
    }

    #[test]
    fn explicit_mask_without_an_image_is_refused() {
        let mut body = minimal_body();
        body["mask"] = json!({ "kind": "explicit" });
        assert!(parse_job_document(&body).is_err());
    }

    #[test]
    fn explicit_mask_with_auto_only_fields_is_refused() {
        let mut body = minimal_body();
        body["mask"] = json!({
            "kind": "explicit",
            "mask_png_base64": base64_encode(&png(b"mask")),
            "threshold": 10,
        });
        let refusal = parse_job_document(&body).expect_err("threshold on explicit refused");
        assert!(refusal.0.contains("threshold"), "{refusal}");
    }

    #[test]
    fn out_of_bound_mask_numbers_are_refused() {
        let mut body = minimal_body();
        body["mask"] = json!({ "kind": "auto", "feather": FEATHER_PX_MAX + 1 });
        assert!(parse_job_document(&body).is_err());
        body["mask"] = json!({ "kind": "auto", "threshold": MASK_THRESHOLD_MAX + 1 });
        assert!(parse_job_document(&body).is_err());
        body["mask"] = json!({ "kind": "auto", "dilate": MASK_DILATE_PX_MAX + 1 });
        assert!(parse_job_document(&body).is_err());
    }

    // -- Driver argv ------------------------------------------------------

    fn argv_strings(document: &JobDocument, mask_file: Option<&Path>) -> Vec<String> {
        edit_argv(
            Path::new("/jobs/job-1/canvas.png"),
            Path::new("/jobs/job-1/prompt.txt"),
            "job-1",
            document.config,
            document.seed,
            document.mask.as_ref().map(MaskDocument::shape).as_ref(),
            mask_file,
        )
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
    }

    #[test]
    fn faithful_argv_has_no_lora_flag_and_an_explicit_seed() {
        let document = parse_job_document(&minimal_body()).expect("parses");
        let argv = argv_strings(&document, None);
        assert_eq!(argv[0], "edit");
        assert_eq!(argv[3], "job-1");
        assert!(!argv.iter().any(|a| a == "--turbo" || a == "--omni"));
        let seed_at = argv.iter().position(|a| a == "--seed").expect("seed flag");
        assert_eq!(argv[seed_at + 1], SEED_DEFAULT.to_string());
    }

    #[test]
    fn turbo_argv_carries_exactly_the_turbo_flag() {
        let mut body = minimal_body();
        body["config"] = json!("turbo");
        let document = parse_job_document(&body).expect("parses");
        let argv = argv_strings(&document, None);
        assert_eq!(argv.iter().filter(|a| *a == "--turbo").count(), 1);
        assert!(!argv.iter().any(|a| a == "--omni"));
    }

    #[test]
    fn explicit_mask_argv_names_the_mask_and_feather() {
        let mut body = minimal_body();
        body["mask"] = json!({
            "kind": "explicit",
            "mask_png_base64": base64_encode(&png(b"mask")),
            "feather": 14,
        });
        let document = parse_job_document(&body).expect("parses");
        let argv = argv_strings(&document, Some(Path::new("/jobs/job-1/mask.png")));
        let mask_at = argv.iter().position(|a| a == "--mask").expect("mask flag");
        assert_eq!(argv[mask_at + 1], "/jobs/job-1/mask.png");
        let feather_at = argv.iter().position(|a| a == "--feather").expect("feather");
        assert_eq!(argv[feather_at + 1], "14");
        assert!(!argv.iter().any(|a| a == "--auto-mask"));
    }

    #[test]
    fn auto_mask_argv_names_threshold_and_dilate() {
        let mut body = minimal_body();
        body["mask"] = json!({ "kind": "auto", "threshold": 40, "dilate": 3 });
        let document = parse_job_document(&body).expect("parses");
        let argv = argv_strings(&document, None);
        assert!(argv.iter().any(|a| a == "--auto-mask"));
        let threshold_at = argv
            .iter()
            .position(|a| a == "--mask-threshold")
            .expect("threshold flag");
        assert_eq!(argv[threshold_at + 1], "40");
        let dilate_at = argv
            .iter()
            .position(|a| a == "--mask-dilate")
            .expect("dilate flag");
        assert_eq!(argv[dilate_at + 1], "3");
    }

    // -- Base64 -----------------------------------------------------------

    #[test]
    fn base64_round_trips() {
        for sample in [
            &b""[..],
            b"a",
            b"ab",
            b"abc",
            b"abcd",
            &[0x89, 0x50, 0xFF, 0x00],
        ] {
            if sample.is_empty() {
                continue; // the empty string is refused by design (no empty images)
            }
            let encoded = base64_encode(sample);
            assert_eq!(base64_decode(&encoded).expect("decodes"), sample);
        }
    }

    #[test]
    fn base64_refuses_malformed_input() {
        assert!(base64_decode("").is_err());
        assert!(base64_decode("A").is_err());
        assert!(base64_decode("AB$=").is_err());
        assert!(base64_decode("AB D").is_err());
        assert!(base64_decode("A=AA").is_err());
        assert!(base64_decode("AB=C").is_err());
    }

    // -- Artefacts --------------------------------------------------------

    #[test]
    fn artefacts_are_classified_ordered_and_hashed() {
        let dir = std::env::temp_dir().join(format!(
            "torogao-edit-broker-lib-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        // sha256("abc") is a published test vector.
        std::fs::write(dir.join("job-7_00001_.png"), b"abc").expect("raw");
        std::fs::write(dir.join("job-7_comp.png"), b"composited").expect("comp");
        std::fs::write(dir.join("job-7_mask.png"), b"alpha").expect("mask");
        std::fs::write(dir.join("job-8_00001_.png"), b"another job").expect("other");
        std::fs::write(dir.join("unrelated.txt"), b"noise").expect("noise");

        let artefacts = collect_artefacts(&dir, "job-7").expect("collects");
        assert_eq!(artefacts.len(), 3);
        assert_eq!(artefacts[0].kind, ArtefactKind::RawEdit);
        assert_eq!(
            artefacts[0].sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(artefacts[1].kind, ArtefactKind::Composited);
        assert_eq!(artefacts[2].kind, ArtefactKind::MaskAlpha);
        assert_eq!(
            delivered_artefact(&artefacts).expect("delivered").kind,
            ArtefactKind::Composited
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn delivered_falls_back_to_the_raw_edit() {
        let dir = std::env::temp_dir().join(format!(
            "torogao-edit-broker-lib-test-raw-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("job-9_00001_.png"), b"abc").expect("raw");
        let artefacts = collect_artefacts(&dir, "job-9").expect("collects");
        assert_eq!(
            delivered_artefact(&artefacts).expect("delivered").kind,
            ArtefactKind::RawEdit
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
