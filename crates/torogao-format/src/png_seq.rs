//! PNG-sequence reader for the drawn-clip importer
//! (torogao-drawn-clip spike follow-on, 2026-11-27).
//!
//! The `.kra` bridge (`crate::kra`) ties drawn-clip
//! import to one authoring tool. This reader is the
//! tool-agnostic bridge the Owner commissioned in its
//! place of honour beside it: any tool that can export
//! PNG frames — OpenToonz, Clip Studio, Blender, Krita
//! itself — feeds `torogao-drawn-clip/1` through here,
//! so the authoring tool is a free choice, never a
//! pipeline marriage.
//!
//! Two source shapes are mapped:
//!
//! - **Manifest**: a `torogao-drawn-seq/1` JSON document
//!   naming, per track, an ordered list of exposures —
//!   each a drawing file plus its hold in ticks. A file
//!   may be referenced by many exposures and many
//!   tracks; it is decoded once and carried by content
//!   identity, exactly the format's substitution
//!   discipline (reference, never duplication).
//! - **Numbered sequence**: a directory of
//!   `<prefix><digits>.png` frames in the shape of a raw
//!   render dump. Consecutive frames with identical
//!   pixels collapse into holds by content hash, so a
//!   dump imports as an exposure table with no
//!   hand-written manifest.
//!
//! Identity follows pixels, never names: a drawing's
//! identity is the SHA-256 of its decoded RGBA bytes
//! (the drawn-clip law), so a file named to impersonate
//! another drawing still imports under its own content.
//! Anything the reader does not map is a typed refusal
//! (`TG-PNGSEQ-*`), never a silent approximation:
//! missing files, zero holds, escaping paths, numbering
//! gaps or duplicate frame indices, dimension drift
//! between drawings, and undecodable PNGs all refuse by
//! name.
//!
//! The PNG decode law mirrors `torogao-paint`'s
//! `png_io::decode_png_rgba` exactly (the `png` crate,
//! `EXPAND | STRIP_16`, colour types expanded to RGBA8),
//! so the importer and the player — which decodes the
//! payloads it verifies through the paint core — agree
//! on the pixels byte for byte.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::drawn_clip::{DRAWN_DIM_MAX, DRAWN_KEYS_PER_TRACK_MAX, DRAWN_RANGE_SPAN_MAX};
use crate::kra::KraRaster;
use crate::limits::INPUT_BYTES_MAX;

/// The manifest format stamp. This constant is the one
/// match site for the token.
pub const PNG_SEQ_FORMAT: &str = "torogao-drawn-seq/1";

/// Ceiling on one source PNG's byte length at import
/// (mirrors torogao-paint's `PNG_FILE_BYTES_MAX`).
pub const PNG_FILE_BYTES_MAX: usize = 512 * 1024 * 1024;
/// Tracks per manifest.
pub const SEQ_TRACKS_MAX: usize = 256;
/// Exposures per track.
pub const SEQ_EXPOSURES_PER_TRACK_MAX: usize = 100_000;
/// Frames in one numbered sequence.
pub const SEQ_FRAMES_MAX: usize = 100_000;

/// A typed refusal from the PNG-sequence reader: a
/// stable `TG-PNGSEQ-` code plus a human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeqError {
    code: &'static str,
    message: String,
}

impl SeqError {
    fn new(code: &'static str, message: impl Into<String>) -> SeqError {
        SeqError {
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

impl fmt::Display for SeqError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SeqError {}

/// The reader's result type.
pub type SeqResult<T> = Result<T, SeqError>;

/// Which source shape produced a document (the importer
/// records it in the clip's provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqMode {
    /// A `torogao-drawn-seq/1` manifest.
    Manifest,
    /// A numbered render dump.
    Sequence,
}

impl SeqMode {
    /// The provenance token for this mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            SeqMode::Manifest => "manifest",
            SeqMode::Sequence => "sequence",
        }
    }
}

/// One key of an imported track: the drawing in force
/// from `time` until the track's next key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeqKey {
    /// Timeline position (frame index).
    pub time: i64,
    /// The drawing shown from `time` on.
    pub raster: KraRaster,
}

/// One imported track: an exposure table over the
/// source drawings, bottom-first stack order preserved
/// from the manifest (a numbered sequence yields one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeqTrack {
    /// Track name.
    pub name: String,
    /// Track opacity, 0..=255.
    pub opacity: u8,
    /// Visibility as declared.
    pub visible: bool,
    /// Keys sorted by time, holds already collapsed.
    pub keys: Vec<SeqKey>,
}

/// What the source actually used, counted at read time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeqCensus {
    /// Distinct source files decoded (a file referenced
    /// by many exposures is read once).
    pub files_read: usize,
    /// Exposure entries consumed (manifest entries, or
    /// frames in a numbered sequence).
    pub exposures: usize,
    /// Distinct drawings by pixel hash.
    pub distinct_drawings: usize,
    /// Numbered-sequence frames folded into a previous
    /// frame's hold (always 0 for a manifest, whose
    /// holds are explicit).
    pub collapsed_frames: usize,
}

/// A decoded PNG-sequence source, shaped to feed the
/// drawn-clip document builder exactly as
/// `crate::kra::KraDocument` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngSeqDocument {
    /// Clip name.
    pub name: String,
    /// Canvas width.
    pub width: u32,
    /// Canvas height.
    pub height: u32,
    /// Timeline frames per second.
    pub fps: i64,
    /// Playback range start (inclusive).
    pub range_from: i64,
    /// Playback range end (inclusive).
    pub range_to: i64,
    /// Whether playback loops.
    pub loop_playback: bool,
    /// The matte under the stack, `[r, g, b, a]`.
    pub matte: [u8; 4],
    /// The source shape that produced this document.
    pub mode: SeqMode,
    /// Tracks, bottom first.
    pub tracks: Vec<SeqTrack>,
    /// The read-time census.
    pub census: SeqCensus,
}

/// Parameters for a numbered-sequence import: the facts
/// a render dump does not carry about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceParams {
    /// Clip name (drawn-clip name rules).
    pub name: String,
    /// Timeline frames per second, 1..=240.
    pub fps: i64,
    /// Whether playback loops.
    pub loop_playback: bool,
    /// The matte under the stack, `[r, g, b, a]`.
    pub matte: [u8; 4],
}

/// Reads a `torogao-drawn-seq/1` manifest and every
/// drawing it references. Exposure files resolve
/// relative to the manifest's own directory and may not
/// escape it.
pub fn read_png_manifest(manifest_path: &Path) -> SeqResult<PngSeqDocument> {
    let bytes = std::fs::read(manifest_path).map_err(|error| {
        SeqError::new(
            "TG-PNGSEQ-READ",
            format!(
                "manifest {} is not readable: {error}",
                manifest_path.display()
            ),
        )
    })?;
    if bytes.len() > INPUT_BYTES_MAX {
        return Err(SeqError::new(
            "TG-PNGSEQ-TOO-LARGE",
            format!(
                "manifest is {} bytes, over the {INPUT_BYTES_MAX} byte limit",
                bytes.len()
            ),
        ));
    }
    let doc: Value = serde_json::from_slice(&bytes).map_err(|error| {
        SeqError::new(
            "TG-PNGSEQ-NOT-JSON",
            format!("manifest is not JSON: {error}"),
        )
    })?;
    let root = doc.as_object().ok_or_else(|| {
        SeqError::new("TG-PNGSEQ-NOT-OBJECT", "the manifest root is not an object")
    })?;

    const TOP_LEVEL: [&str; 7] = ["format", "name", "fps", "loop", "matte", "range", "tracks"];
    for key in root.keys() {
        if !TOP_LEVEL.contains(&key.as_str()) {
            return Err(SeqError::new(
                "TG-PNGSEQ-UNKNOWN-FIELD",
                format!("top-level field {key:?} is not part of {PNG_SEQ_FORMAT}"),
            ));
        }
    }
    let format = required_string(root, "format")?;
    if format != PNG_SEQ_FORMAT {
        return Err(SeqError::new(
            "TG-PNGSEQ-FORMAT",
            format!("format is {format:?}, not {PNG_SEQ_FORMAT:?}"),
        ));
    }
    let name = required_string(root, "name")?;
    check_clip_name(&name)?;
    let fps = required_i64(root, "fps", "manifest")?;
    check_fps(fps)?;
    let loop_playback = optional_bool(root, "loop", true)?;
    let matte = optional_matte(root)?;
    let declared_range = optional_range(root)?;

    let base_dir = manifest_path
        .parent()
        .map(|parent| {
            if parent.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                parent.to_path_buf()
            }
        })
        .unwrap_or_else(|| PathBuf::from("."));

    let tracks_json = required_array(root, "tracks")?;
    if tracks_json.is_empty() || tracks_json.len() > SEQ_TRACKS_MAX {
        return Err(SeqError::new(
            "TG-PNGSEQ-EMPTY",
            format!(
                "manifest declares {} tracks, outside 1..={SEQ_TRACKS_MAX}",
                tracks_json.len()
            ),
        ));
    }

    let mut canvas: Option<(u32, u32)> = None;
    let mut cache: BTreeMap<PathBuf, KraRaster> = BTreeMap::new();
    let mut track_names = BTreeSet::new();
    let mut total_exposures = 0_usize;
    let mut tracks: Vec<SeqTrack> = Vec::with_capacity(tracks_json.len());
    let mut max_track_end: Option<i64> = None;

    for (index, track_json) in tracks_json.iter().enumerate() {
        let context = format!("track {index}");
        let track = track_json.as_object().ok_or_else(|| {
            SeqError::new(
                "TG-PNGSEQ-WRONG-TYPE",
                format!("{context} is not an object"),
            )
        })?;
        const TRACK_FIELDS: [&str; 4] = ["name", "opacity", "visible", "exposures"];
        for key in track.keys() {
            if !TRACK_FIELDS.contains(&key.as_str()) {
                return Err(SeqError::new(
                    "TG-PNGSEQ-UNKNOWN-FIELD",
                    format!("{context} field {key:?} is not part of {PNG_SEQ_FORMAT}"),
                ));
            }
        }
        let track_name = required_string(track, "name").map_err(|e| prefix(e, &context))?;
        if track_name.len() > 128 {
            return Err(SeqError::new(
                "TG-PNGSEQ-TRACK-NAME",
                format!(
                    "{context} name is {} bytes, over the 128 limit",
                    track_name.len()
                ),
            ));
        }
        if !track_names.insert(track_name.clone()) {
            return Err(SeqError::new(
                "TG-PNGSEQ-TRACK-NAME",
                format!("{context} duplicates track name {track_name:?}"),
            ));
        }
        let opacity = optional_i64(track, "opacity", 255).map_err(|e| prefix(e, &context))?;
        if !(0..=255).contains(&opacity) {
            return Err(SeqError::new(
                "TG-PNGSEQ-OPACITY",
                format!("{context} opacity {opacity} is outside 0..=255"),
            ));
        }
        let visible = optional_bool(track, "visible", true).map_err(|e| prefix(e, &context))?;

        let exposures = required_array(track, "exposures").map_err(|e| prefix(e, &context))?;
        if exposures.is_empty() || exposures.len() > SEQ_EXPOSURES_PER_TRACK_MAX {
            return Err(SeqError::new(
                "TG-PNGSEQ-EMPTY",
                format!(
                    "{context} declares {} exposures, outside 1..={SEQ_EXPOSURES_PER_TRACK_MAX}",
                    exposures.len()
                ),
            ));
        }
        total_exposures += exposures.len();

        // Exposure keys accumulate from the range start:
        // each exposure's drawing is in force from its
        // start tick for `hold` ticks.
        let mut time = declared_range.map(|(from, _)| from).unwrap_or(0);
        let mut keys: Vec<SeqKey> = Vec::with_capacity(exposures.len());
        for (exposure_index, exposure_json) in exposures.iter().enumerate() {
            let exposure_context = format!("{context} exposure {exposure_index}");
            let exposure = exposure_json.as_object().ok_or_else(|| {
                SeqError::new(
                    "TG-PNGSEQ-WRONG-TYPE",
                    format!("{exposure_context} is not an object"),
                )
            })?;
            const EXPOSURE_FIELDS: [&str; 2] = ["file", "hold"];
            for key in exposure.keys() {
                if !EXPOSURE_FIELDS.contains(&key.as_str()) {
                    return Err(SeqError::new(
                        "TG-PNGSEQ-UNKNOWN-FIELD",
                        format!("{exposure_context} field {key:?} is not part of {PNG_SEQ_FORMAT}"),
                    ));
                }
            }
            let file =
                required_string(exposure, "file").map_err(|e| prefix(e, &exposure_context))?;
            let hold = required_i64(exposure, "hold", &exposure_context)?;
            if hold < 1 {
                return Err(SeqError::new(
                    "TG-PNGSEQ-HOLD",
                    format!(
                        "{exposure_context} hold is {hold}; an exposure holds at least one tick (a zero hold would name a drawing no frame ever shows)"
                    ),
                ));
            }
            if let Some((_, to)) = declared_range {
                if time > to {
                    return Err(SeqError::new(
                        "TG-PNGSEQ-EXPOSURE-RANGE",
                        format!(
                            "{exposure_context} starts at tick {time}, past the range end {to}; dead exposures are refused, not carried"
                        ),
                    ));
                }
            }
            let raster = load_drawing(&base_dir, &file, &mut cache, &mut canvas)
                .map_err(|e| prefix(e, &exposure_context))?;
            keys.push(SeqKey { time, raster });
            time += hold;
        }
        if keys.len() > DRAWN_KEYS_PER_TRACK_MAX {
            return Err(SeqError::new(
                "TG-PNGSEQ-TOO-LARGE",
                format!(
                    "{context} resolves to {} keys, over the {DRAWN_KEYS_PER_TRACK_MAX} per-track limit",
                    keys.len()
                ),
            ));
        }
        let track_end = time - 1;
        max_track_end = Some(max_track_end.map_or(track_end, |end: i64| end.max(track_end)));
        tracks.push(SeqTrack {
            name: track_name,
            opacity: opacity as u8,
            visible,
            keys,
        });
    }

    let (range_from, range_to) = match declared_range {
        Some(range) => range,
        None => (0, max_track_end.expect("at least one track ran")),
    };
    if range_to - range_from > DRAWN_RANGE_SPAN_MAX {
        return Err(SeqError::new(
            "TG-PNGSEQ-RANGE",
            format!("range {range_from}..={range_to} spans over {DRAWN_RANGE_SPAN_MAX} frames"),
        ));
    }
    let (width, height) = canvas.expect("at least one drawing was decoded");
    let census = SeqCensus {
        files_read: cache.len(),
        exposures: total_exposures,
        distinct_drawings: distinct_drawings(&tracks),
        collapsed_frames: 0,
    };
    Ok(PngSeqDocument {
        name,
        width,
        height,
        fps,
        range_from,
        range_to,
        loop_playback,
        matte,
        mode: SeqMode::Manifest,
        tracks,
        census,
    })
}

/// Reads a numbered render dump: `<prefix><digits>.png`
/// files whose numeric parts form one contiguous run.
/// Consecutive pixel-identical frames collapse into
/// holds by content hash.
pub fn read_png_sequence(dir: &Path, params: &SequenceParams) -> SeqResult<PngSeqDocument> {
    check_clip_name(&params.name)?;
    check_fps(params.fps)?;

    let mut entries: Vec<(u64, String, PathBuf)> = Vec::new();
    let mut prefix_seen: Option<String> = None;
    let read_dir = std::fs::read_dir(dir).map_err(|error| {
        SeqError::new(
            "TG-PNGSEQ-READ",
            format!(
                "sequence directory {} is not readable: {error}",
                dir.display()
            ),
        )
    })?;
    for entry in read_dir {
        let entry = entry.map_err(|error| {
            SeqError::new(
                "TG-PNGSEQ-READ",
                format!("a directory entry is unreadable: {error}"),
            )
        })?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !file_name.ends_with(".png") {
            continue;
        }
        let path = entry.path();
        if !path.is_file() {
            return Err(SeqError::new(
                "TG-PNGSEQ-SEQUENCE-NAME",
                format!("{file_name:?} ends in .png but is not a regular file"),
            ));
        }
        let stem = &file_name[..file_name.len() - ".png".len()];
        let digits_len = stem
            .bytes()
            .rev()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits_len == 0 {
            return Err(SeqError::new(
                "TG-PNGSEQ-SEQUENCE-NAME",
                format!(
                    "{file_name:?} carries no frame number; a numbered sequence names every frame <prefix><digits>.png (use a manifest for arbitrary names)"
                ),
            ));
        }
        let (prefix, digits) = stem.split_at(stem.len() - digits_len);
        match &prefix_seen {
            None => prefix_seen = Some(prefix.to_string()),
            Some(seen) if seen != prefix => {
                return Err(SeqError::new(
                    "TG-PNGSEQ-SEQUENCE-NAME",
                    format!(
                        "{file_name:?} has prefix {prefix:?}, the sequence began with {seen:?}; one dump, one prefix"
                    ),
                ));
            }
            _ => {}
        }
        let index: u64 = digits.parse().map_err(|_| {
            SeqError::new(
                "TG-PNGSEQ-SEQUENCE-NAME",
                format!("{file_name:?} frame number {digits:?} is not a readable index"),
            )
        })?;
        entries.push((index, file_name.to_string(), path));
    }
    if entries.is_empty() {
        return Err(SeqError::new(
            "TG-PNGSEQ-EMPTY",
            format!("{} holds no numbered .png frames", dir.display()),
        ));
    }
    if entries.len() > SEQ_FRAMES_MAX {
        return Err(SeqError::new(
            "TG-PNGSEQ-TOO-LARGE",
            format!(
                "sequence holds {} frames, over the {SEQ_FRAMES_MAX} limit",
                entries.len()
            ),
        ));
    }
    entries.sort_by_key(|(index, _, _)| *index);
    for pair in entries.windows(2) {
        let (prev_index, prev_name, _) = &pair[0];
        let (index, name, _) = &pair[1];
        if index == prev_index {
            return Err(SeqError::new(
                "TG-PNGSEQ-SEQUENCE-DUPLICATE",
                format!(
                    "{prev_name:?} and {name:?} both claim frame {index}; two names for one frame is a collision, not a sequence"
                ),
            ));
        }
        if *index != prev_index + 1 {
            return Err(SeqError::new(
                "TG-PNGSEQ-SEQUENCE-GAP",
                format!(
                    "frame {} is missing between {prev_name:?} and {name:?}; a gap would silently retime every later frame",
                    prev_index + 1
                ),
            ));
        }
    }

    let mut canvas: Option<(u32, u32)> = None;
    let mut keys: Vec<SeqKey> = Vec::new();
    for (position, (_, file_name, path)) in entries.iter().enumerate() {
        let raster = decode_drawing_file(path, &mut canvas)
            .map_err(|e| SeqError::new(e.code, format!("frame {file_name:?}: {}", e.message)))?;
        let sha = raster.sha256_hex();
        match keys.last() {
            Some(last) if last.raster.sha256_hex() == sha => {
                // Identical to the frame in force: the
                // hold extends, no new key.
            }
            _ => keys.push(SeqKey {
                time: position as i64,
                raster,
            }),
        }
    }
    let frame_count = entries.len();
    let census = SeqCensus {
        files_read: frame_count,
        exposures: frame_count,
        distinct_drawings: distinct_in_keys(&keys),
        collapsed_frames: frame_count - keys.len(),
    };
    let (width, height) = canvas.expect("at least one frame was decoded");
    Ok(PngSeqDocument {
        name: params.name.clone(),
        width,
        height,
        fps: params.fps,
        range_from: 0,
        range_to: frame_count as i64 - 1,
        loop_playback: params.loop_playback,
        matte: params.matte,
        mode: SeqMode::Sequence,
        tracks: vec![SeqTrack {
            name: "sequence".to_string(),
            opacity: 255,
            visible: true,
            keys,
        }],
        census,
    })
}

fn distinct_drawings(tracks: &[SeqTrack]) -> usize {
    let mut seen = BTreeSet::new();
    for track in tracks {
        for key in &track.keys {
            seen.insert(key.raster.sha256_hex());
        }
    }
    seen.len()
}

fn distinct_in_keys(keys: &[SeqKey]) -> usize {
    let mut seen = BTreeSet::new();
    for key in keys {
        seen.insert(key.raster.sha256_hex());
    }
    seen.len()
}

fn check_clip_name(name: &str) -> SeqResult<()> {
    crate::drawn_clip::check_name(name, "clip name")
        .map_err(|error| SeqError::new("TG-PNGSEQ-NAME", error.message().to_string()))
}

fn check_fps(fps: i64) -> SeqResult<()> {
    if !(1..=240).contains(&fps) {
        return Err(SeqError::new(
            "TG-PNGSEQ-FPS",
            format!("fps {fps} is outside 1..=240"),
        ));
    }
    Ok(())
}

/// Loads one exposure's drawing: resolves `file`
/// against `base_dir` (refusing any escape), decodes it
/// once per path, and holds every drawing to the first
/// drawing's canvas.
fn load_drawing(
    base_dir: &Path,
    file: &str,
    cache: &mut BTreeMap<PathBuf, KraRaster>,
    canvas: &mut Option<(u32, u32)>,
) -> SeqResult<KraRaster> {
    let relative = Path::new(file);
    let escapes = relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::RootDir));
    if escapes {
        return Err(SeqError::new(
            "TG-PNGSEQ-PATH",
            format!(
                "exposure file {file:?} is absolute or escapes the manifest's directory; drawings live beside the manifest"
            ),
        ));
    }
    let path = base_dir.join(relative);
    if let Some(raster) = cache.get(&path) {
        return Ok(raster.clone());
    }
    let raster = decode_drawing_file(&path, canvas)?;
    cache.insert(path, raster.clone());
    Ok(raster)
}

/// Reads and decodes one PNG file, holding it to the
/// running canvas (the first drawing sets it).
fn decode_drawing_file(path: &Path, canvas: &mut Option<(u32, u32)>) -> SeqResult<KraRaster> {
    let bytes = std::fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            SeqError::new(
                "TG-PNGSEQ-MISSING",
                format!("drawing file {} does not exist", path.display()),
            )
        } else {
            SeqError::new(
                "TG-PNGSEQ-READ",
                format!("drawing file {} is not readable: {error}", path.display()),
            )
        }
    })?;
    let raster = decode_png_rgba(&bytes)?;
    match canvas {
        None => *canvas = Some((raster.width, raster.height)),
        Some((width, height)) if (*width, *height) != (raster.width, raster.height) => {
            return Err(SeqError::new(
                "TG-PNGSEQ-DIMENSIONS",
                format!(
                    "{} decodes to {}x{}; the clip's drawings are full-canvas {}x{}",
                    path.display(),
                    raster.width,
                    raster.height,
                    width,
                    height
                ),
            ));
        }
        _ => {}
    }
    Ok(raster)
}

/// Decodes PNG bytes to a canvas raster. The decode law
/// mirrors torogao-paint's `png_io::decode_png_rgba`
/// (see the module header), so importer and player
/// agree on the pixels byte for byte.
fn decode_png_rgba(bytes: &[u8]) -> SeqResult<KraRaster> {
    if bytes.len() > PNG_FILE_BYTES_MAX {
        return Err(SeqError::new(
            "TG-PNGSEQ-TOO-LARGE",
            format!(
                "a drawing is {} bytes, over the {PNG_FILE_BYTES_MAX} byte limit",
                bytes.len()
            ),
        ));
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|error| {
        SeqError::new("TG-PNGSEQ-DECODE", format!("PNG does not decode: {error}"))
    })?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    if width == 0 || width > DRAWN_DIM_MAX as u32 || height == 0 || height > DRAWN_DIM_MAX as u32 {
        return Err(SeqError::new(
            "TG-PNGSEQ-DIMENSIONS",
            format!("drawing canvas {width}x{height} is outside 1..={DRAWN_DIM_MAX} per side"),
        ));
    }
    // output_buffer_size is None for interlaced images;
    // the EXPAND|STRIP_16 output is at most 4 bytes per
    // pixel, so the canvas size is the honest bound.
    let buffer_size = reader
        .output_buffer_size()
        .unwrap_or((width as usize) * (height as usize) * 4);
    let mut buf = vec![0_u8; buffer_size];
    let frame = reader.next_frame(&mut buf).map_err(|error| {
        SeqError::new("TG-PNGSEQ-DECODE", format!("PNG does not decode: {error}"))
    })?;
    buf.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => expand_alpha(buf, 255),
        png::ColorType::GrayscaleAlpha => expand_gray_alpha(buf),
        png::ColorType::Grayscale => expand_gray(buf),
        png::ColorType::Indexed => {
            return Err(SeqError::new(
                "TG-PNGSEQ-DECODE",
                "indexed colour survived the EXPAND transform",
            ));
        }
    };
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(SeqError::new(
            "TG-PNGSEQ-DECODE",
            format!("decoded {} bytes, canvas needs {expected}", rgba.len()),
        ));
    }
    Ok(KraRaster {
        width,
        height,
        rgba,
    })
}

/// RGB -> RGBA with a constant alpha (mirrors
/// torogao-paint's `png_io`).
fn expand_alpha(rgb: Vec<u8>, alpha: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity((rgb.len() / 3) * 4);
    for px in rgb.chunks_exact(3) {
        out.extend_from_slice(&[px[0], px[1], px[2], alpha]);
    }
    out
}

/// Gray+alpha -> RGBA (mirrors torogao-paint's
/// `png_io`).
fn expand_gray_alpha(ga: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity((ga.len() / 2) * 4);
    for px in ga.chunks_exact(2) {
        out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
    }
    out
}

/// Gray -> opaque RGBA (mirrors torogao-paint's
/// `png_io`).
fn expand_gray(gray: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(gray.len() * 4);
    for value in gray {
        out.extend_from_slice(&[value, value, value, 255]);
    }
    out
}

fn prefix(error: SeqError, context: &str) -> SeqError {
    SeqError::new(error.code, format!("{context}: {}", error.message))
}

fn required_string(parent: &serde_json::Map<String, Value>, field: &str) -> SeqResult<String> {
    match parent.get(field) {
        Some(Value::String(text)) => {
            if text.is_empty() {
                Err(SeqError::new(
                    "TG-PNGSEQ-MISSING-FIELD",
                    format!("{field} is empty"),
                ))
            } else {
                Ok(text.clone())
            }
        }
        Some(_) => Err(SeqError::new(
            "TG-PNGSEQ-WRONG-TYPE",
            format!("{field} is not a string"),
        )),
        None => Err(SeqError::new(
            "TG-PNGSEQ-MISSING-FIELD",
            format!("required field {field} is missing"),
        )),
    }
}

fn required_array<'a>(
    parent: &'a serde_json::Map<String, Value>,
    field: &str,
) -> SeqResult<&'a Vec<Value>> {
    match parent.get(field) {
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(SeqError::new(
            "TG-PNGSEQ-WRONG-TYPE",
            format!("{field} is not an array"),
        )),
        None => Err(SeqError::new(
            "TG-PNGSEQ-MISSING-FIELD",
            format!("required field {field} is missing"),
        )),
    }
}

fn required_i64(
    parent: &serde_json::Map<String, Value>,
    field: &str,
    context: &str,
) -> SeqResult<i64> {
    match parent.get(field) {
        Some(value) => value.as_i64().ok_or_else(|| {
            SeqError::new(
                "TG-PNGSEQ-WRONG-TYPE",
                format!("{context} field {field} is not an integer"),
            )
        }),
        None => Err(SeqError::new(
            "TG-PNGSEQ-MISSING-FIELD",
            format!("{context} required field {field} is missing"),
        )),
    }
}

fn optional_i64(
    parent: &serde_json::Map<String, Value>,
    field: &str,
    default: i64,
) -> SeqResult<i64> {
    match parent.get(field) {
        None => Ok(default),
        Some(value) => value.as_i64().ok_or_else(|| {
            SeqError::new(
                "TG-PNGSEQ-WRONG-TYPE",
                format!("field {field} is not an integer"),
            )
        }),
    }
}

fn optional_bool(
    parent: &serde_json::Map<String, Value>,
    field: &str,
    default: bool,
) -> SeqResult<bool> {
    match parent.get(field) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(SeqError::new(
            "TG-PNGSEQ-WRONG-TYPE",
            format!("field {field} is not a boolean"),
        )),
    }
}

fn optional_matte(parent: &serde_json::Map<String, Value>) -> SeqResult<[u8; 4]> {
    match parent.get("matte") {
        None => Ok([0, 0, 0, 0]),
        Some(Value::Array(channels)) => {
            if channels.len() != 4
                || channels
                    .iter()
                    .any(|channel| channel.as_u64().map(|v| v > 255).unwrap_or(true))
            {
                return Err(SeqError::new(
                    "TG-PNGSEQ-MATTE",
                    "matte must be an array of four channel values in 0..=255",
                ));
            }
            Ok([
                channels[0].as_u64().expect("checked") as u8,
                channels[1].as_u64().expect("checked") as u8,
                channels[2].as_u64().expect("checked") as u8,
                channels[3].as_u64().expect("checked") as u8,
            ])
        }
        Some(_) => Err(SeqError::new(
            "TG-PNGSEQ-WRONG-TYPE",
            "matte is not an array",
        )),
    }
}

fn optional_range(parent: &serde_json::Map<String, Value>) -> SeqResult<Option<(i64, i64)>> {
    match parent.get("range") {
        None => Ok(None),
        Some(Value::Object(range)) => {
            let from = required_i64(range, "from", "range")?;
            let to = required_i64(range, "to", "range")?;
            if from < 0 || to < from {
                return Err(SeqError::new(
                    "TG-PNGSEQ-RANGE",
                    format!("range {from}..={to} is negative or inverted"),
                ));
            }
            Ok(Some((from, to)))
        }
        Some(_) => Err(SeqError::new(
            "TG-PNGSEQ-WRONG-TYPE",
            "range is not an object",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use sha2::{Digest, Sha256};

    const W: u32 = 8;
    const H: u32 = 6;

    fn solid(width: u32, height: u32, px: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            out.extend_from_slice(&px);
        }
        out
    }

    fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(rgba).unwrap();
    }

    /// A scratch source directory unique to one test.
    fn temp_source(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("torogao-pngseq-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    fn manifest(dir: &Path, text: &str) -> PathBuf {
        let path = dir.join("drawn-seq.json");
        std::fs::write(&path, text).unwrap();
        path
    }

    const RED: [u8; 4] = [200, 30, 30, 255];
    const BLUE: [u8; 4] = [30, 30, 200, 255];

    // --- Validation -------------------------------------

    #[test]
    fn manifest_builds_exposure_keys_and_references_repeats() {
        let dir = temp_source("manifest-keys");
        write_png(&dir.join("open.png"), W, H, &solid(W, H, RED));
        write_png(&dir.join("shut.png"), W, H, &solid(W, H, BLUE));
        let path = manifest(
            &dir,
            r#"{"format":"torogao-drawn-seq/1","name":"blink-test","fps":24,
                "tracks":[{"name":"anim/blink","exposures":[
                    {"file":"open.png","hold":4},
                    {"file":"shut.png","hold":1},
                    {"file":"open.png","hold":5}]}]}"#,
        );
        let doc = read_png_manifest(&path).expect("manifest reads");
        assert_eq!(doc.mode, SeqMode::Manifest);
        assert_eq!((doc.range_from, doc.range_to), (0, 9));
        assert_eq!(doc.tracks.len(), 1);
        let keys = &doc.tracks[0].keys;
        assert_eq!(
            keys.iter().map(|key| key.time).collect::<Vec<_>>(),
            vec![0, 4, 5]
        );
        // The repeat references the same drawing by
        // content, decoded once.
        assert_eq!(keys[0].raster.sha256_hex(), keys[2].raster.sha256_hex());
        assert_ne!(keys[0].raster.sha256_hex(), keys[1].raster.sha256_hex());
        assert_eq!(doc.census.files_read, 2);
        assert_eq!(doc.census.exposures, 3);
        assert_eq!(doc.census.distinct_drawings, 2);
        cleanup(&dir);
    }

    #[test]
    fn sequence_collapses_consecutive_identical_frames() {
        let dir = temp_source("sequence-collapse");
        let frames = [RED, RED, RED, BLUE, BLUE, RED];
        for (index, px) in frames.iter().enumerate() {
            write_png(
                &dir.join(format!("frame_{index:04}.png")),
                W,
                H,
                &solid(W, H, *px),
            );
        }
        let doc = read_png_sequence(
            &dir,
            &SequenceParams {
                name: "dump-test".to_string(),
                fps: 24,
                loop_playback: true,
                matte: [0, 0, 0, 0],
            },
        )
        .expect("sequence reads");
        assert_eq!(doc.mode, SeqMode::Sequence);
        assert_eq!((doc.range_from, doc.range_to), (0, 5));
        assert_eq!(doc.tracks.len(), 1);
        assert_eq!(doc.tracks[0].name, "sequence");
        assert_eq!(
            doc.tracks[0]
                .keys
                .iter()
                .map(|key| key.time)
                .collect::<Vec<_>>(),
            vec![0, 3, 5]
        );
        assert_eq!(doc.census.files_read, 6);
        assert_eq!(doc.census.collapsed_frames, 3);
        assert_eq!(doc.census.distinct_drawings, 2);
        cleanup(&dir);
    }

    #[test]
    fn manifest_range_is_respected_and_short_tracks_hold() {
        let dir = temp_source("manifest-range");
        write_png(&dir.join("only.png"), W, H, &solid(W, H, RED));
        let path = manifest(
            &dir,
            r#"{"format":"torogao-drawn-seq/1","name":"held-test","fps":12,
                "range":{"from":0,"to":9},"loop":false,"matte":[9,8,7,255],
                "tracks":[{"name":"base","exposures":[{"file":"only.png","hold":4}]}]}"#,
        );
        let doc = read_png_manifest(&path).expect("manifest reads");
        // The declared range stands even though the one
        // track's exposures end at tick 3: the drawing
        // holds, per the format's hold semantics.
        assert_eq!((doc.range_from, doc.range_to), (0, 9));
        assert!(!doc.loop_playback);
        assert_eq!(doc.matte, [9, 8, 7, 255]);
        assert_eq!(doc.tracks[0].keys.len(), 1);
        cleanup(&dir);
    }

    #[test]
    fn manifest_two_tracks_share_a_drawing_file() {
        let dir = temp_source("manifest-share");
        write_png(&dir.join("shared.png"), W, H, &solid(W, H, RED));
        let path = manifest(
            &dir,
            r#"{"format":"torogao-drawn-seq/1","name":"share-test","fps":24,
                "tracks":[
                    {"name":"base","exposures":[{"file":"shared.png","hold":2}]},
                    {"name":"top","opacity":128,"visible":false,
                     "exposures":[{"file":"shared.png","hold":2}]}]}"#,
        );
        let doc = read_png_manifest(&path).expect("manifest reads");
        assert_eq!(doc.census.files_read, 1);
        assert_eq!(doc.census.distinct_drawings, 1);
        assert_eq!(doc.tracks[1].opacity, 128);
        assert!(!doc.tracks[1].visible);
        cleanup(&dir);
    }

    // --- Adversarial ------------------------------------

    #[test]
    fn manifest_refuses_a_missing_file() {
        let dir = temp_source("missing");
        let path = manifest(
            &dir,
            r#"{"format":"torogao-drawn-seq/1","name":"missing-test","fps":24,
                "tracks":[{"name":"base","exposures":[{"file":"ghost.png","hold":1}]}]}"#,
        );
        let error = read_png_manifest(&path).expect_err("missing file refuses");
        assert_eq!(error.code(), "TG-PNGSEQ-MISSING");
        cleanup(&dir);
    }

    #[test]
    fn manifest_refuses_a_zero_hold() {
        let dir = temp_source("zero-hold");
        write_png(&dir.join("a.png"), W, H, &solid(W, H, RED));
        let path = manifest(
            &dir,
            r#"{"format":"torogao-drawn-seq/1","name":"zero-test","fps":24,
                "tracks":[{"name":"base","exposures":[{"file":"a.png","hold":0}]}]}"#,
        );
        let error = read_png_manifest(&path).expect_err("zero hold refuses");
        assert_eq!(error.code(), "TG-PNGSEQ-HOLD");
        cleanup(&dir);
    }

    #[test]
    fn manifest_refuses_duplicate_track_names() {
        let dir = temp_source("dup-tracks");
        write_png(&dir.join("a.png"), W, H, &solid(W, H, RED));
        let path = manifest(
            &dir,
            r#"{"format":"torogao-drawn-seq/1","name":"dup-test","fps":24,
                "tracks":[
                    {"name":"base","exposures":[{"file":"a.png","hold":1}]},
                    {"name":"base","exposures":[{"file":"a.png","hold":1}]}]}"#,
        );
        let error = read_png_manifest(&path).expect_err("duplicate names refuse");
        assert_eq!(error.code(), "TG-PNGSEQ-TRACK-NAME");
        cleanup(&dir);
    }

    #[test]
    fn manifest_refuses_an_exposure_overrunning_the_range() {
        let dir = temp_source("overrun");
        write_png(&dir.join("a.png"), W, H, &solid(W, H, RED));
        let path = manifest(
            &dir,
            r#"{"format":"torogao-drawn-seq/1","name":"overrun-test","fps":24,
                "range":{"from":0,"to":3},
                "tracks":[{"name":"base","exposures":[
                    {"file":"a.png","hold":4},
                    {"file":"a.png","hold":1}]}]}"#,
        );
        let error = read_png_manifest(&path).expect_err("overrun refuses");
        assert_eq!(error.code(), "TG-PNGSEQ-EXPOSURE-RANGE");
        cleanup(&dir);
    }

    #[test]
    fn manifest_refuses_a_path_escaping_the_source_directory() {
        let dir = temp_source("escape");
        let path = manifest(
            &dir,
            r#"{"format":"torogao-drawn-seq/1","name":"escape-test","fps":24,
                "tracks":[{"name":"base","exposures":[{"file":"../outside.png","hold":1}]}]}"#,
        );
        let error = read_png_manifest(&path).expect_err("escape refuses");
        assert_eq!(error.code(), "TG-PNGSEQ-PATH");
        cleanup(&dir);
    }

    #[test]
    fn sequence_refuses_a_duplicate_frame_index() {
        let dir = temp_source("dup-index");
        write_png(&dir.join("frame_1.png"), W, H, &solid(W, H, RED));
        write_png(&dir.join("frame_01.png"), W, H, &solid(W, H, BLUE));
        let error = read_png_sequence(
            &dir,
            &SequenceParams {
                name: "dup-test".to_string(),
                fps: 24,
                loop_playback: true,
                matte: [0, 0, 0, 0],
            },
        )
        .expect_err("duplicate index refuses");
        assert_eq!(error.code(), "TG-PNGSEQ-SEQUENCE-DUPLICATE");
        cleanup(&dir);
    }

    #[test]
    fn sequence_refuses_a_numbering_gap() {
        let dir = temp_source("gap");
        write_png(&dir.join("frame_0000.png"), W, H, &solid(W, H, RED));
        write_png(&dir.join("frame_0002.png"), W, H, &solid(W, H, BLUE));
        let error = read_png_sequence(
            &dir,
            &SequenceParams {
                name: "gap-test".to_string(),
                fps: 24,
                loop_playback: true,
                matte: [0, 0, 0, 0],
            },
        )
        .expect_err("gap refuses");
        assert_eq!(error.code(), "TG-PNGSEQ-SEQUENCE-GAP");
        assert!(error.message().contains("frame 1 is missing"));
        cleanup(&dir);
    }

    #[test]
    fn sequence_refuses_an_empty_directory() {
        let dir = temp_source("empty");
        let error = read_png_sequence(
            &dir,
            &SequenceParams {
                name: "empty-test".to_string(),
                fps: 24,
                loop_playback: true,
                matte: [0, 0, 0, 0],
            },
        )
        .expect_err("empty sequence refuses");
        assert_eq!(error.code(), "TG-PNGSEQ-EMPTY");
        cleanup(&dir);
    }

    #[test]
    fn import_refuses_mismatched_drawing_dimensions() {
        let dir = temp_source("dims");
        write_png(&dir.join("big.png"), W, H, &solid(W, H, RED));
        write_png(&dir.join("small.png"), 4, 4, &solid(4, 4, BLUE));
        let path = manifest(
            &dir,
            r#"{"format":"torogao-drawn-seq/1","name":"dims-test","fps":24,
                "tracks":[{"name":"base","exposures":[
                    {"file":"big.png","hold":1},
                    {"file":"small.png","hold":1}]}]}"#,
        );
        let error = read_png_manifest(&path).expect_err("dimension drift refuses");
        assert_eq!(error.code(), "TG-PNGSEQ-DIMENSIONS");
        cleanup(&dir);
    }

    #[test]
    fn import_refuses_non_png_bytes() {
        let dir = temp_source("not-png");
        std::fs::write(dir.join("fake.png"), b"this is not a png").unwrap();
        let path = manifest(
            &dir,
            r#"{"format":"torogao-drawn-seq/1","name":"fake-test","fps":24,
                "tracks":[{"name":"base","exposures":[{"file":"fake.png","hold":1}]}]}"#,
        );
        let error = read_png_manifest(&path).expect_err("non-png refuses");
        assert_eq!(error.code(), "TG-PNGSEQ-DECODE");
        cleanup(&dir);
    }

    #[test]
    fn identity_follows_pixels_not_file_names() {
        let dir = temp_source("identity");
        // A file named to impersonate the blue drawing's
        // content-derived id, but carrying red pixels:
        // the name must not buy it the blue identity.
        let blue_sha = {
            let digest = Sha256::digest(solid(W, H, BLUE));
            let mut hex = String::with_capacity(64);
            for byte in digest {
                hex.push_str(&format!("{byte:02x}"));
            }
            hex
        };
        write_png(
            &dir.join(format!("d-{blue_sha}.png")),
            W,
            H,
            &solid(W, H, RED),
        );
        // Two differently named files, identical pixels:
        // one drawing, referenced twice.
        write_png(&dir.join("one.png"), W, H, &solid(W, H, BLUE));
        write_png(&dir.join("two.png"), W, H, &solid(W, H, BLUE));
        let path = manifest(
            &dir,
            &format!(
                r#"{{"format":"torogao-drawn-seq/1","name":"identity-test","fps":24,
                    "tracks":[{{"name":"base","exposures":[
                        {{"file":"d-{blue_sha}.png","hold":1}},
                        {{"file":"one.png","hold":1}},
                        {{"file":"two.png","hold":1}}]}}]}}"#
            ),
        );
        let doc = read_png_manifest(&path).expect("manifest reads");
        let keys = &doc.tracks[0].keys;
        // The impersonator imports under red's hash, not
        // the blue hash its name claims.
        assert_ne!(keys[0].raster.sha256_hex(), blue_sha);
        assert_eq!(keys[1].raster.sha256_hex(), blue_sha);
        assert_eq!(keys[2].raster.sha256_hex(), blue_sha);
        assert_eq!(doc.census.distinct_drawings, 2);
        cleanup(&dir);
    }
}
