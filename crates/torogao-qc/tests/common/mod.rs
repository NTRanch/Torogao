//! Shared fixture builders for the asset-level verb tests
//! (`asset`, `export-check`, `roundtrip`).

use serde_json::Value;
use std::path::{Path, PathBuf};
use torogao_qc::report::Report;

/// A fresh per-test temp directory (the inspect fixtures'
/// idiom).
pub fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("torogao-qc-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

/// Writes bytes into the directory, returning the path.
pub fn write_bytes(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

/// Writes text into the directory, returning the path.
pub fn write_text(dir: &Path, name: &str, contents: &str) -> PathBuf {
    write_bytes(dir, name, contents.as_bytes())
}

/// The finding codes of a report, in published order.
pub fn codes(report: &Report) -> Vec<&str> {
    report.findings().iter().map(|f| f.code).collect()
}

/// Assembles a GLB byte string from a JSON document and a
/// binary chunk: header, JSON chunk (space-padded to 4),
/// BIN chunk (zero-padded to 4).
pub fn glb_bytes(json: &Value, bin: &[u8]) -> Vec<u8> {
    let mut json_bytes = serde_json::to_vec(json).expect("fixture json serializes");
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut bin_bytes = bin.to_vec();
    while bin_bytes.len() % 4 != 0 {
        bin_bytes.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0x4654_6c67u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4e4f_534au32.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004e_4942u32.to_le_bytes());
    out.extend_from_slice(&bin_bytes);
    out
}

/// A solid-colour PNG of the given size, as bytes. (Used by
/// the `asset` fixtures; other fixture crates share this
/// module and do not call it.)
#[allow(dead_code)]
pub fn png_bytes(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let img = image::ImageBuffer::from_pixel(width, height, image::Rgb(rgb));
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .expect("encode fixture png");
    bytes
}
