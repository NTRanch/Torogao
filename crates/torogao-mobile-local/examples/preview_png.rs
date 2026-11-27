//! Host-side preview dumper for the trial: renders the bundled
//! base (neutral, smiled, posed) to PNGs so the renderer can be
//! judged by eye before any APK exists.
//!
//! Usage: `cargo run -p torogao-mobile-local --example
//! preview_png -- <out-dir>`
//!
//! The PNG writer is a minimal stored-block encoder (std
//! only) — trial tooling, not a shipped surface.

use std::fs;
use std::path::PathBuf;

use torogao_mobile_local::record::prima_template;
use torogao_mobile_local::{LocalEngine, PreviewView};

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

fn write_png(path: &PathBuf, width: u32, height: u32, rgba: &[u8]) {
    // zlib stream of stored DEFLATE blocks over filter-0 rows.
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in rgba.chunks_exact(width as usize * 4) {
        raw.push(0u8);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78u8, 0x01];
    let mut adler = (1u32, 0u32);
    for block in raw.chunks(65535) {
        let last = block.as_ptr_range().end == raw.as_ptr_range().end;
        z.push(if last { 1 } else { 0 });
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
        for &b in block {
            adler.0 = (adler.0 + b as u32) % 65521;
            adler.1 = (adler.1 + adler.0) % 65521;
        }
    }
    z.extend_from_slice(&((adler.1 << 16) | adler.0).to_be_bytes());

    let mut png = vec![137u8, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    fs::write(path, png).expect("write png");
}

fn main() {
    let out_dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: preview_png <out-dir>"),
    );
    fs::create_dir_all(&out_dir).expect("create out dir");
    let data_dir = out_dir.join("data");
    let engine = LocalEngine::open(&data_dir).expect("engine opens");

    let neutral = prima_template();
    let (img, stats) = engine
        .preview(&neutral, PreviewView::Body, 540, 720)
        .expect("body preview");
    println!(
        "body neutral: morph {:.1} ms · skin {:.1} ms · raster {:.1} ms · {} tris",
        stats.morph_ms, stats.skin_ms, stats.raster_ms, stats.triangles
    );
    write_png(
        &out_dir.join("body-neutral.png"),
        img.width,
        img.height,
        &img.rgba,
    );

    let (img, stats) = engine
        .preview(&neutral, PreviewView::Face, 540, 540)
        .expect("face preview");
    println!("face neutral: raster {:.1} ms", stats.raster_ms);
    write_png(
        &out_dir.join("face-neutral.png"),
        img.width,
        img.height,
        &img.rgba,
    );

    let mut smiling = prima_template();
    smiling.expressions.insert("Fac_Mth_Smile".to_string(), 1.0);
    smiling
        .expressions
        .insert("Fac_Eye_Close".to_string(), 0.55);
    smiling.expressions.insert("Fac_Ebr_Up".to_string(), 0.5);
    smiling.body_params.insert("bust_size".to_string(), 0.7);
    smiling
        .pose
        .insert("mixamorig:Head".to_string(), [4.0, 18.0, 0.0]);
    let (img, stats) = engine
        .preview(&smiling, PreviewView::Face, 540, 540)
        .expect("face smile preview");
    println!("face smile+brows+blink: raster {:.1} ms", stats.raster_ms);
    write_png(
        &out_dir.join("face-smile.png"),
        img.width,
        img.height,
        &img.rgba,
    );
    let (img, _) = engine
        .preview(&smiling, PreviewView::Body, 540, 720)
        .expect("body smile preview");
    write_png(
        &out_dir.join("body-smile.png"),
        img.width,
        img.height,
        &img.rgba,
    );
    println!("wrote PNGs to {}", out_dir.display());
}
