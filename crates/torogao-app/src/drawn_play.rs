//! Drawn-clip import and playback (`torogao-drawn-clip/1`,
//! drawn-animation spike, 2026-11-27).
//!
//! Import reads a source through `torogao_format` — a
//! `.kra` through `torogao_format::kra` (no Krita process
//! anywhere in the path), or PNG drawings through
//! `torogao_format::png_seq` (a `torogao-drawn-seq/1`
//! manifest, or a numbered render dump) — and writes a
//! frozen clip directory: `clip.json` plus one PNG
//! payload per distinct drawing under `drawings/`. Both
//! routes converge on the one emitter, so a clip's kind
//! never depends on which tool authored it. Playback
//! loads a frozen clip, verifies every payload against
//! its declared pixel hash, and composites each frame
//! through `torogao_paint::Document` — the same integer
//! composite path the egui paint surface renders —
//! writing one PNG per frame. Substitution only: the
//! drawing in force is the latest key at or before the
//! frame (hold semantics), exactly as the format
//! specifies.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use torogao_format::drawn_clip;
use torogao_format::jsonfmt::to_d_json;
use torogao_format::kra::{self, KraRaster};
use torogao_format::png_seq;
use torogao_paint::document::{BaseRaster, Document};
use torogao_paint::layer::{BlendMode, Layer, LayerId};

use crate::error::{AppError, AppResult};

/// Options for `--drawn-import`.
pub struct DrawnImportOptions {
    /// The source `.kra` file.
    pub kra_path: PathBuf,
    /// The clip directory to create.
    pub out_dir: PathBuf,
    /// The clip's name (validated by the format).
    pub name: String,
    /// Freeze compiler string.
    pub compiler: String,
    /// Freeze band string.
    pub band: String,
}

/// Options for `--drawn-import-png`.
pub struct DrawnImportPngOptions {
    /// The directory holding the PNG drawings.
    pub source_dir: PathBuf,
    /// The `torogao-drawn-seq/1` manifest, for a
    /// manifest-driven import; without one the
    /// directory is read as a numbered sequence.
    pub manifest: Option<PathBuf>,
    /// The clip directory to create.
    pub out_dir: PathBuf,
    /// The clip's name for a numbered-sequence import
    /// (a manifest carries its own).
    pub name: String,
    /// Frames per second for a numbered-sequence
    /// import (a manifest carries its own).
    pub fps: i64,
    /// Freeze compiler string.
    pub compiler: String,
    /// Freeze band string.
    pub band: String,
}

/// Options for `--drawn-play`.
pub struct DrawnPlayOptions {
    /// The frozen clip directory.
    pub clip_dir: PathBuf,
    /// Where frame PNGs land.
    pub out_dir: PathBuf,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn io(context: &'static str) -> impl Fn(std::io::Error) -> AppError {
    move |error| AppError::Io {
        context,
        message: error.to_string(),
    }
}

/// One track in the shape the clip emitter consumes:
/// the common denominator of the `.kra` reader's
/// layers and the PNG-sequence reader's tracks.
struct EmitTrack {
    name: String,
    blend: String,
    opacity: u8,
    visible: bool,
    keys: Vec<(i64, KraRaster)>,
}

/// Everything the clip emitter needs, assembled by
/// either importer.
struct EmitClip {
    /// Label for the import report line.
    source_label: String,
    /// The clip directory to create.
    out_dir: PathBuf,
    /// The clip's name.
    name: String,
    /// Freeze compiler string.
    compiler: String,
    /// Freeze band string.
    band: String,
    /// Canvas `(width, height)`.
    canvas: (u32, u32),
    /// Timeline frames per second.
    fps: i64,
    /// Playback range start (inclusive).
    range_from: i64,
    /// Playback range end (inclusive).
    range_to: i64,
    /// Whether playback loops.
    loop_playback: bool,
    /// The matte under the stack.
    matte: [u8; 4],
    /// Tracks, bottom first.
    tracks: Vec<EmitTrack>,
    /// The provenance object (covered by the freeze).
    provenance: Value,
}

/// `--drawn-import`: `.kra` in, frozen drawn clip out.
pub fn run_drawn_import(options: &DrawnImportOptions) -> AppResult<i32> {
    let bytes = std::fs::read(&options.kra_path).map_err(io("read kra"))?;
    let source_sha = sha256_hex(&bytes);
    let doc = kra::read_kra(&bytes).map_err(|error| AppError::Format(error.to_string()))?;
    let (fps, range_from, range_to) = doc.animation.ok_or_else(|| {
        AppError::Format(
            "the document carries no <animation> element; a drawn clip needs a timeline"
                .to_string(),
        )
    })?;

    println!(
        "kra census: canvas={}x{} layers={} keyframed_layers={} keyframes={} tiles={} blends={:?} node_types={:?} colour_space={}",
        doc.width,
        doc.height,
        doc.census.layer_count,
        doc.census.keyframed_layers,
        doc.census.keyframe_count,
        doc.census.tile_count,
        doc.census.blend_modes,
        doc.census.node_types,
        doc.census.color_space,
    );

    let tracks: Vec<EmitTrack> = doc
        .layers
        .iter()
        .map(|layer| EmitTrack {
            name: layer.name.clone(),
            blend: layer.blend.clone(),
            opacity: layer.opacity,
            visible: layer.visible,
            keys: layer
                .frames
                .iter()
                .map(|frame| (frame.time, frame.raster.clone()))
                .collect(),
        })
        .collect();
    let provenance = json!({
        "source": {
            "kind": "kra",
            "file": options
                .kra_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "sha256": source_sha,
            "document_name": doc.name,
        },
        "census": {
            "layers": doc.census.layer_count,
            "keyframed_layers": doc.census.keyframed_layers,
            "keyframes": doc.census.keyframe_count,
            "tiles": doc.census.tile_count,
            "blend_modes": doc.census.blend_modes,
            "node_types": doc.census.node_types,
            "color_space": doc.census.color_space,
        },
    });
    emit_clip(&EmitClip {
        source_label: options.kra_path.display().to_string(),
        out_dir: options.out_dir.clone(),
        name: options.name.clone(),
        compiler: options.compiler.clone(),
        band: options.band.clone(),
        canvas: (doc.width, doc.height),
        fps,
        range_from,
        range_to,
        loop_playback: true,
        // The matte the source renderer composited onto.
        // For this witness document the banked Krita
        // renders are opaque white outside the figure and
        // its stored merged image agrees, so the matte is
        // recorded as data here, per file, never assumed
        // silently by the player.
        matte: [255, 255, 255, 255],
        tracks,
        provenance,
    })
}

/// `--drawn-import-png`: PNG drawings in (a
/// `torogao-drawn-seq/1` manifest, or a numbered
/// sequence when no manifest is given), frozen drawn
/// clip out.
pub fn run_drawn_import_png(options: &DrawnImportPngOptions) -> AppResult<i32> {
    let doc = match &options.manifest {
        Some(manifest_path) => png_seq::read_png_manifest(manifest_path)
            .map_err(|error| AppError::Format(error.to_string()))?,
        None => png_seq::read_png_sequence(
            &options.source_dir,
            &png_seq::SequenceParams {
                name: options.name.clone(),
                fps: options.fps,
                loop_playback: true,
                matte: [0, 0, 0, 0],
            },
        )
        .map_err(|error| AppError::Format(error.to_string()))?,
    };

    println!(
        "png-seq census: mode={} canvas={}x{} files={} exposures={} drawings={} collapsed={}",
        doc.mode.as_str(),
        doc.width,
        doc.height,
        doc.census.files_read,
        doc.census.exposures,
        doc.census.distinct_drawings,
        doc.census.collapsed_frames,
    );

    let tracks: Vec<EmitTrack> = doc
        .tracks
        .iter()
        .map(|track| EmitTrack {
            name: track.name.clone(),
            blend: "normal".to_string(),
            opacity: track.opacity,
            visible: track.visible,
            keys: track
                .keys
                .iter()
                .map(|key| (key.time, key.raster.clone()))
                .collect(),
        })
        .collect();
    let provenance = json!({
        "source": {
            "kind": "png-seq",
            "mode": doc.mode.as_str(),
            "directory": options
                .source_dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "manifest": options.manifest.as_ref().and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            }),
        },
        "census": {
            "files": doc.census.files_read,
            "exposures": doc.census.exposures,
            "distinct_drawings": doc.census.distinct_drawings,
            "collapsed_frames": doc.census.collapsed_frames,
        },
    });
    emit_clip(&EmitClip {
        source_label: options.source_dir.display().to_string(),
        out_dir: options.out_dir.clone(),
        name: doc.name.clone(),
        compiler: options.compiler.clone(),
        band: options.band.clone(),
        canvas: (doc.width, doc.height),
        fps: doc.fps,
        range_from: doc.range_from,
        range_to: doc.range_to,
        loop_playback: doc.loop_playback,
        matte: doc.matte,
        tracks,
        provenance,
    })
}

/// The one clip emitter both importers converge on:
/// dedupe drawings by pixel hash, write each payload
/// once, build the document, freeze it, and write the
/// clip directory.
fn emit_clip(emit: &EmitClip) -> AppResult<i32> {
    let clip_json_path = emit.out_dir.join("clip.json");
    if clip_json_path.exists() {
        return Err(AppError::Cli(format!(
            "{} already holds a clip.json; refusing to overwrite a clip directory",
            emit.out_dir.display()
        )));
    }
    let drawings_dir = emit.out_dir.join("drawings");
    std::fs::create_dir_all(&drawings_dir).map_err(io("create clip directory"))?;

    // Drawings: dedupe by pixel hash; write each payload
    // once. The PNG is a container — identity is the
    // SHA-256 of the RGBA bytes.
    let mut drawing_ids: BTreeMap<String, String> = BTreeMap::new(); // sha -> id
    for track in &emit.tracks {
        for (_, raster) in &track.keys {
            let sha = raster.sha256_hex();
            if let std::collections::btree_map::Entry::Vacant(slot) = drawing_ids.entry(sha) {
                let id = format!("d-{}", slot.key());
                let png = torogao_paint::png_io::encode_png_rgba(
                    raster.width,
                    raster.height,
                    &raster.rgba,
                )
                .map_err(|error| AppError::Format(error.to_string()))?;
                std::fs::write(drawings_dir.join(format!("{id}.png")), png)
                    .map_err(io("write drawing payload"))?;
                slot.insert(id);
            }
        }
    }

    let drawings_json: Vec<Value> = drawing_ids
        .iter()
        .map(|(sha, id)| {
            json!({
                "id": id,
                "sha256": sha,
                "width": emit.canvas.0,
                "height": emit.canvas.1,
                "file": drawn_clip::drawing_file_name(id),
            })
        })
        .collect();

    let tracks_json: Vec<Value> = emit
        .tracks
        .iter()
        .map(|track| {
            let keys: Vec<Value> = track
                .keys
                .iter()
                .map(|(time, raster)| {
                    json!({
                        "time": time,
                        "drawing": drawing_ids[&raster.sha256_hex()],
                    })
                })
                .collect();
            json!({
                "name": track.name,
                "blend": track.blend,
                "opacity": track.opacity,
                "visible": track.visible,
                "keys": keys,
            })
        })
        .collect();

    let working = json!({
        "format": drawn_clip::DRAWN_CLIP_FORMAT,
        "name": emit.name,
        "canvas": {"width": emit.canvas.0, "height": emit.canvas.1},
        "fps": emit.fps,
        "range": {"from": emit.range_from, "to": emit.range_to},
        "loop": emit.loop_playback,
        "matte": emit.matte,
        "drawings": drawings_json,
        "tracks": tracks_json,
        "provenance": emit.provenance,
    });

    let frozen = drawn_clip::freeze_document(&working, &emit.compiler, &emit.band)
        .map_err(|error| AppError::Format(error.to_string()))?;
    let report = drawn_clip::validate_document(&frozen)
        .map_err(|error| AppError::Format(error.to_string()))?;
    std::fs::write(&clip_json_path, format!("{}\n", to_d_json(&frozen)))
        .map_err(io("write clip.json"))?;

    println!(
        "imported {} -> {} (drawings={} tracks={} keys={} freeze={})",
        emit.source_label,
        emit.out_dir.display(),
        report.drawings,
        report.tracks,
        report.keys,
        report.freeze_sha256.as_deref().unwrap_or("-"),
    );
    Ok(0)
}

/// `--drawn-play`: frozen clip in, composited frames out.
pub fn run_drawn_play(options: &DrawnPlayOptions) -> AppResult<i32> {
    let clip_json_path = options.clip_dir.join("clip.json");
    let bytes = std::fs::read(&clip_json_path).map_err(io("read clip.json"))?;
    let report =
        drawn_clip::validate_clip(&bytes).map_err(|error| AppError::Format(error.to_string()))?;
    if report.freeze_sha256.is_none() {
        return Err(AppError::Format(
            "the clip is not frozen; playback runs frozen clips only".to_string(),
        ));
    }
    let doc: Value = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::Format(format!("clip.json is not JSON: {error}")))?;
    let (width, height) = (report.canvas.0 as u32, report.canvas.1 as u32);

    // Payloads: decode and verify each against its
    // declared pixel hash before any frame is built.
    let mut payloads: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for drawing in doc["drawings"].as_array().expect("validated") {
        let id = drawing["id"].as_str().expect("validated");
        let sha = drawing["sha256"].as_str().expect("validated");
        let file = drawing["file"].as_str().expect("validated");
        let png = std::fs::read(options.clip_dir.join(file)).map_err(io("read drawing payload"))?;
        let (pw, ph, rgba) = torogao_paint::png_io::decode_png_rgba(&png)
            .map_err(|error| AppError::Format(error.to_string()))?;
        if (u64::from(pw), u64::from(ph)) != report.canvas {
            return Err(AppError::Format(format!(
                "drawing {id} decodes to {pw}x{ph}, not the canvas {}x{}",
                report.canvas.0, report.canvas.1
            )));
        }
        let actual = sha256_hex(&rgba);
        if actual != sha {
            return Err(AppError::Format(format!(
                "drawing {id} payload hashes to {actual}, the document declares {sha}"
            )));
        }
        payloads.insert(id.to_string(), rgba);
    }

    // The matte base raster.
    let matte = doc["matte"].as_array().expect("validated");
    let matte_px: [u8; 4] = [
        matte[0].as_u64().expect("validated") as u8,
        matte[1].as_u64().expect("validated") as u8,
        matte[2].as_u64().expect("validated") as u8,
        matte[3].as_u64().expect("validated") as u8,
    ];
    let mut matte_pixels = vec![0_u8; (width as usize) * (height as usize) * 4];
    for px in matte_pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&matte_px);
    }
    let base = BaseRaster::Raster {
        sha256: Sha256::digest(&matte_pixels).into(),
        pixels: matte_pixels,
    };

    std::fs::create_dir_all(&options.out_dir).map_err(io("create frames directory"))?;
    let tracks = doc["tracks"].as_array().expect("validated");
    for time in report.range.0..=report.range.1 {
        let mut layers: Vec<Layer> = Vec::with_capacity(tracks.len());
        for (index, track) in tracks.iter().enumerate() {
            if !track["visible"].as_bool().expect("validated") {
                continue;
            }
            // Hold semantics: the latest key at or before
            // this frame. Validation guaranteed coverage.
            let active = track["keys"]
                .as_array()
                .expect("validated")
                .iter()
                .rev()
                .find(|key| key["time"].as_i64().expect("validated") <= time)
                .expect("validator guaranteed a key in force");
            let drawing_id = active["drawing"].as_str().expect("validated");
            let mut layer = Layer::from_rgba(
                LayerId((index + 1) as u32),
                track["name"].as_str().expect("validated").to_string(),
                width,
                height,
                &payloads[drawing_id],
            )
            .map_err(|error| AppError::Format(error.to_string()))?;
            layer.blend = BlendMode::Normal;
            layer.opacity = track["opacity"].as_u64().expect("validated") as u8;
            layers.push(layer);
        }
        let document = Document::from_base_and_layers(width, height, base.clone(), layers)
            .map_err(|error| AppError::Format(error.to_string()))?;
        let composite = document.composite();
        let png = torogao_paint::png_io::encode_png_rgba(width, height, &composite)
            .map_err(|error| AppError::Format(error.to_string()))?;
        let frame_path = options.out_dir.join(format!("frame{time:04}.png"));
        std::fs::write(&frame_path, png).map_err(io("write frame"))?;
        println!(
            "frame {time} rgba_sha256={} composite_hash={}",
            sha256_hex(&composite),
            hex_bytes(&document.composite_hash()),
        );
    }
    Ok(0)
}

fn hex_bytes(bytes: &[u8; 32]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}
