//! The `torogao-paint/1` document container (SPEC SS3.3):
//! a directory holding `header.ron` (the commit point,
//! written last), `base.png` when the base is a raster,
//! `layers.ron` (descriptors as of the save, cross-checked
//! against replay at load), and `ops.ron` (the log).
//!
//! Strict parsing throughout: unknown fields refused,
//! unknown major versions refused by name, a base whose
//! pixels do not hash to the header's record refused.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::document::{BaseRaster, Document, SourceRef};
use crate::error::{PaintError, PaintResult};
use crate::hash::{from_hex, sha256_bytes, to_hex};
use crate::layer::{BlendMode, LayerId};
use crate::ops::Op;

/// The format tag written in every header.
pub const FORMAT_TAG: &str = "torogao-paint/1";
/// The format major version this build reads and writes.
pub const FORMAT_MAJOR: u32 = 1;
/// The format minor version (additive extensions only).
pub const FORMAT_MINOR: u32 = 0;
/// Ceiling on `header.ron` bytes.
pub const HEADER_FILE_BYTES_MAX: usize = 1024 * 1024;
/// Ceiling on `layers.ron` bytes.
pub const LAYERS_FILE_BYTES_MAX: usize = 1024 * 1024;
/// Ceiling on `ops.ron` bytes (stamp runs are bulky).
pub const OPS_FILE_BYTES_MAX: usize = 1024 * 1024 * 1024;

/// Read a file reading at most `max` bytes; a file past
/// the ceiling is refused, never fully read.
pub fn read_bounded(path: &Path, max: usize) -> PaintResult<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|err| PaintError::io("open", path, &err))?;
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| PaintError::io("read", path, &err))?;
    if bytes.len() > max {
        return Err(PaintError::limit(
            "file bytes",
            bytes.len() as u64,
            max as u64,
        ));
    }
    Ok(bytes)
}

/// Write a file, mapping failures into the typed error.
fn write_file(path: &Path, bytes: &[u8]) -> PaintResult<()> {
    std::fs::write(path, bytes).map_err(|err| PaintError::io("write", path, &err))
}

/// Parse RON text with the strictness the format demands.
fn parse_ron<T: for<'de> Deserialize<'de>>(text: &[u8], what: &str) -> PaintResult<T> {
    let text = std::str::from_utf8(text)
        .map_err(|_| PaintError::format(format!("{what} is not UTF-8")))?;
    ron::from_str(text).map_err(|err| PaintError::format(format!("{what} parse failed: {err}")))
}

/// Serialize a record to deterministic RON text (field
/// order is declaration order; no ambient data enters).
fn to_ron<T: Serialize>(value: &T, what: &str) -> PaintResult<String> {
    ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::new())
        .map_err(|err| PaintError::format(format!("{what} serialize failed: {err}")))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderFile {
    format: String,
    major: u32,
    minor: u32,
    width: u32,
    height: u32,
    color_space: String,
    created_by: String,
    crate_version: String,
    base: BaseRecord,
    sources: Vec<SourceRecord>,
    palette_source: Option<SourceRecord>,
    truncated_ops: u32,
}

#[derive(Serialize, Deserialize)]
enum BaseRecord {
    Empty,
    Raster { sha256: String },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRecord {
    path: String,
    sha256: String,
}

impl SourceRecord {
    fn from_ref(source: &SourceRef) -> SourceRecord {
        SourceRecord {
            path: source.path.clone(),
            sha256: to_hex(&source.sha256),
        }
    }

    fn into_ref(self) -> PaintResult<SourceRef> {
        let bytes = from_hex(&self.sha256)
            .filter(|held| held.len() == 32)
            .ok_or_else(|| PaintError::format("source hash is not 32-byte hex"))?;
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&bytes);
        Ok(SourceRef {
            path: self.path,
            sha256,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayersFile {
    layers: Vec<LayerRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerRecord {
    id: u32,
    name: String,
    opacity: u8,
    blend: BlendMode,
    visible: bool,
    lock_alpha: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpsFile {
    ops: Vec<Op>,
}

/// File names inside the container.
fn file(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}

/// Save a document into `dir` (created if needed). The
/// redo tail is truncated first (SS3.3: saving truncates,
/// and the header records the cumulative count). The
/// header is written last: it is the commit point, and a
/// container without it does not open.
pub fn save_document(doc: &mut Document, dir: &Path) -> PaintResult<()> {
    doc.truncate_redo_tail();
    std::fs::create_dir_all(dir).map_err(|err| PaintError::io("create dir", dir, &err))?;
    if let BaseRaster::Raster { pixels, .. } = doc.base() {
        let png = crate::png_io::encode_png_rgba(doc.width(), doc.height(), pixels)?;
        write_file(&file(dir, "base.png"), &png)?;
    }
    let ops_text = to_ron(
        &OpsFile {
            ops: doc.ops().to_vec(),
        },
        "ops.ron",
    )?;
    write_file(&file(dir, "ops.ron"), ops_text.as_bytes())?;
    let layers = LayersFile {
        layers: doc
            .layers()
            .iter()
            .map(|layer| LayerRecord {
                id: layer.id.0,
                name: layer.name.clone(),
                opacity: layer.opacity,
                blend: layer.blend,
                visible: layer.visible,
                lock_alpha: layer.lock_alpha,
            })
            .collect(),
    };
    let layers_text = to_ron(&layers, "layers.ron")?;
    write_file(&file(dir, "layers.ron"), layers_text.as_bytes())?;
    let header = HeaderFile {
        format: FORMAT_TAG.to_string(),
        major: FORMAT_MAJOR,
        minor: FORMAT_MINOR,
        width: doc.width(),
        height: doc.height(),
        color_space: "sRGB".to_string(),
        created_by: "torogao-paint".to_string(),
        crate_version: env!("CARGO_PKG_VERSION").to_string(),
        base: match doc.base() {
            BaseRaster::Empty => BaseRecord::Empty,
            BaseRaster::Raster { sha256, .. } => BaseRecord::Raster {
                sha256: to_hex(sha256),
            },
        },
        sources: doc.sources().iter().map(SourceRecord::from_ref).collect(),
        palette_source: doc.palette_source().map(SourceRecord::from_ref),
        truncated_ops: doc.truncated_ops(),
    };
    let header_text = to_ron(&header, "header.ron")?;
    write_file(&file(dir, "header.ron"), header_text.as_bytes())?;
    Ok(())
}

/// Load a document from its container, replaying the log
/// with full validation and cross-checking `layers.ron`
/// against the replayed stack.
pub fn load_document(dir: &Path) -> PaintResult<Document> {
    let header_bytes = read_bounded(&file(dir, "header.ron"), HEADER_FILE_BYTES_MAX)?;
    let header: HeaderFile = parse_ron(&header_bytes, "header.ron")?;
    if header.format != FORMAT_TAG {
        return Err(PaintError::format(format!(
            "format tag {:?} is not {FORMAT_TAG:?}",
            header.format
        )));
    }
    if header.major != FORMAT_MAJOR {
        return Err(PaintError::UnsupportedVersion {
            found: header.major,
        });
    }
    if header.color_space != "sRGB" {
        return Err(PaintError::format(format!(
            "colour space {:?} is not the v1 value sRGB",
            header.color_space
        )));
    }
    let base = match &header.base {
        BaseRecord::Empty => BaseRaster::Empty,
        BaseRecord::Raster { sha256 } => {
            let png = read_bounded(&file(dir, "base.png"), crate::png_io::PNG_FILE_BYTES_MAX)?;
            let (base_w, base_h, pixels) = crate::png_io::decode_png_rgba(&png)?;
            if base_w != header.width || base_h != header.height {
                return Err(PaintError::format(format!(
                    "base.png is {base_w}x{base_h}, header records {}x{}",
                    header.width, header.height
                )));
            }
            let actual = to_hex(&sha256_bytes(&pixels));
            if actual != *sha256 {
                return Err(PaintError::BaseHashMismatch {
                    expected: sha256.clone(),
                    actual,
                });
            }
            let bytes = from_hex(sha256)
                .filter(|held| held.len() == 32)
                .ok_or_else(|| PaintError::format("base hash is not 32-byte hex"))?;
            let mut digest = [0u8; 32];
            digest.copy_from_slice(&bytes);
            BaseRaster::Raster {
                sha256: digest,
                pixels,
            }
        }
    };
    let ops_bytes = read_bounded(&file(dir, "ops.ron"), OPS_FILE_BYTES_MAX)?;
    let ops_file: OpsFile = parse_ron(&ops_bytes, "ops.ron")?;
    let mut sources = Vec::with_capacity(header.sources.len());
    for record in header.sources {
        sources.push(record.into_ref()?);
    }
    let palette_source = match header.palette_source {
        Some(record) => Some(record.into_ref()?),
        None => None,
    };
    let doc = Document::from_parts(
        header.width,
        header.height,
        base,
        ops_file.ops,
        header.truncated_ops,
        sources,
        palette_source,
    )?;
    let layers_bytes = read_bounded(&file(dir, "layers.ron"), LAYERS_FILE_BYTES_MAX)?;
    let layers_file: LayersFile = parse_ron(&layers_bytes, "layers.ron")?;
    check_layers_match(&doc, &layers_file)?;
    Ok(doc)
}

/// Cross-check the saved layer descriptors against the
/// replayed stack; any divergence is a format refusal
/// naming the first divergent layer.
fn check_layers_match(doc: &Document, file: &LayersFile) -> PaintResult<()> {
    let live = doc.layers();
    if live.len() != file.layers.len() {
        return Err(PaintError::format(format!(
            "layers.ron records {} layers, replay produced {}",
            file.layers.len(),
            live.len()
        )));
    }
    for (held, record) in live.iter().zip(&file.layers) {
        let matches = held.id == LayerId(record.id)
            && held.name == record.name
            && held.opacity == record.opacity
            && held.blend == record.blend
            && held.visible == record.visible
            && held.lock_alpha == record.lock_alpha;
        if !matches {
            return Err(PaintError::format(format!(
                "layers.ron diverges from replay at layer {}",
                record.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Rect;
    use crate::stroke::{Stamp, StampMode};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("torogao-paint-format-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    pub fn sample_doc_pub() -> Document {
        sample_doc()
    }

    fn sample_doc() -> Document {
        let mut doc = Document::new_empty(64, 48).expect("doc");
        let paper = doc.add_layer("paper".to_string()).expect("layer");
        doc.apply_op(Op::FillRect {
            layer: paper,
            rect: Rect {
                x: 0,
                y: 0,
                width: 64,
                height: 48,
            },
            color: [240, 238, 230, 255],
        })
        .expect("fill");
        let ink = doc.add_layer("ink".to_string()).expect("layer");
        doc.apply_op(Op::StampRun {
            layer: ink,
            mode: StampMode::Paint,
            color: [20, 20, 60, 255],
            opacity: 255,
            flow: 255,
            stamps: vec![Stamp {
                x_fixed: 32 * 64,
                y_fixed: 24 * 64,
                radius_px: 6,
                alpha: 255,
            }],
        })
        .expect("stamp");
        doc
    }

    #[test]
    fn save_load_round_trip_preserves_the_composite() {
        let dir = temp_dir("roundtrip");
        let mut doc = sample_doc();
        let before = doc.composite_hash();
        save_document(&mut doc, &dir).expect("save");
        let loaded = load_document(&dir).expect("load");
        assert_eq!(loaded.composite_hash(), before);
        assert_eq!(loaded.ops().len(), doc.ops().len());
        assert_eq!(loaded.layers().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_truncates_the_redo_tail_and_records_it() {
        let dir = temp_dir("truncate");
        let mut doc = sample_doc();
        assert!(doc.undo());
        save_document(&mut doc, &dir).expect("save");
        assert_eq!(doc.truncated_ops(), 1);
        assert!(!doc.can_redo());
        let loaded = load_document(&dir).expect("load");
        assert_eq!(loaded.truncated_ops(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adversarial_containers_refuse() {
        let dir = temp_dir("adversarial");
        let mut doc = sample_doc();
        save_document(&mut doc, &dir).expect("save");
        // Unknown major version: refused by name.
        let header = std::fs::read_to_string(dir.join("header.ron")).expect("read");
        std::fs::write(
            dir.join("header.ron"),
            header.replace("major: 1", "major: 99"),
        )
        .expect("write");
        assert!(matches!(
            load_document(&dir),
            Err(PaintError::UnsupportedVersion { found: 99 })
        ));
        // Restore, then tamper with layers.ron.
        save_document(&mut doc, &dir).expect("save again");
        let layers = std::fs::read_to_string(dir.join("layers.ron")).expect("read");
        std::fs::write(dir.join("layers.ron"), layers.replace("paper", "forged")).expect("write");
        assert!(matches!(
            load_document(&dir),
            Err(PaintError::Format { .. })
        ));
        // Truncated ops.ron refuses rather than half-loading.
        save_document(&mut doc, &dir).expect("save third");
        let ops = std::fs::read(dir.join("ops.ron")).expect("read");
        std::fs::write(dir.join("ops.ron"), &ops[..ops.len() / 2]).expect("write");
        assert!(load_document(&dir).is_err());
        // Unknown field in the header refuses.
        save_document(&mut doc, &dir).expect("save fourth");
        let header = std::fs::read_to_string(dir.join("header.ron")).expect("read");
        std::fs::write(
            dir.join("header.ron"),
            header.replace("minor:", "bogus: 1, minor:"),
        )
        .expect("write");
        assert!(load_document(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adversarial_base_tamper_is_caught_by_hash() {
        let dir = temp_dir("basetamper");
        let png = crate::png_io::encode_png_rgba(8, 8, &[3u8; 256]).expect("png");
        let mut doc = Document::from_png_bytes(&png, "plate.png").expect("doc");
        doc.add_layer("ink".to_string()).expect("layer");
        save_document(&mut doc, &dir).expect("save");
        // Swap base.png for a different image of the same size.
        let other = crate::png_io::encode_png_rgba(8, 8, &[9u8; 256]).expect("png");
        std::fs::write(dir.join("base.png"), other).expect("write");
        assert!(matches!(
            load_document(&dir),
            Err(PaintError::BaseHashMismatch { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "torogao-paint-format-adv-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn adversarial_truncated_header_refuses() {
        let dir = temp_dir("trunchead");
        let mut doc = super::tests::sample_doc_pub();
        save_document(&mut doc, &dir).expect("save");
        let header = std::fs::read(dir.join("header.ron")).expect("read");
        std::fs::write(dir.join("header.ron"), &header[..header.len() / 2]).expect("truncate");
        assert!(load_document(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adversarial_missing_container_files_refuse() {
        let dir = temp_dir("missing");
        std::fs::create_dir_all(&dir).expect("mkdir");
        assert!(load_document(&dir).is_err());
        let mut doc = super::tests::sample_doc_pub();
        save_document(&mut doc, &dir).expect("save");
        std::fs::remove_file(dir.join("ops.ron")).expect("remove ops");
        assert!(load_document(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adversarial_header_canvas_disagrees_with_base() {
        let dir = temp_dir("dimlie");
        let png = crate::png_io::encode_png_rgba(8, 8, &[3u8; 256]).expect("png");
        let mut doc = Document::from_png_bytes(&png, "plate.png").expect("doc");
        save_document(&mut doc, &dir).expect("save");
        let header = std::fs::read_to_string(dir.join("header.ron")).expect("read");
        let lied = header.replace("width: 8", "width: 9");
        assert_ne!(lied, header);
        std::fs::write(dir.join("header.ron"), lied).expect("write");
        assert!(load_document(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
