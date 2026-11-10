//! Plate comparison (comparison (a)): PNG IO and the
//! per-frame statistics the gap analysis names —
//! changed-fraction and per-channel deltas. The primary
//! changed-fraction uses the estate gate's own definition
//! (summed absolute RGB delta > 0.08 in [0,1] floats, the
//! `blink`-metric idiom) so rung numbers read against the
//! gate's history; a strict per-channel fraction rides
//! along for texture.

use std::path::Path;

use crate::error::RenderError;

/// An RGBA8 image, row-major, top row first.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Image, RenderError> {
        if rgba.len() != (width as usize) * (height as usize) * 4 {
            return Err(RenderError::Document(format!(
                "image buffer {} bytes for {width}x{height}",
                rgba.len()
            )));
        }
        Ok(Image {
            width,
            height,
            rgba,
        })
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }
}

/// Load a PNG as RGBA8 (RGB sources gain alpha 255).
pub fn load_png(path: &Path) -> Result<Image, RenderError> {
    let bytes = crate::error::read_file(path)?;
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| RenderError::Document(format!("png {}: {e}", path.display())))?;
    let mut buf = vec![0_u8; reader.output_buffer_size().unwrap_or(512 * 640 * 4)];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| RenderError::Document(format!("png {}: {e}", path.display())))?;
    buf.truncate(info.buffer_size());
    let (width, height) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((width * height * 4) as usize);
            for px in buf.chunks_exact(3) {
                out.extend_from_slice(px);
                out.push(255);
            }
            out
        }
        other => {
            return Err(RenderError::Document(format!(
                "png {}: unsupported colour type {other:?}",
                path.display()
            )));
        }
    };
    Image::new(width, height, rgba)
}

/// Write an RGBA8 PNG (deterministic encoder settings, so
/// PNG bytes hash stably for identical pixels).
pub fn write_png(path: &Path, image: &Image) -> Result<(), RenderError> {
    let file = std::fs::File::create(path).map_err(|e| RenderError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    })?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| RenderError::Document(format!("png {}: {e}", path.display())))?;
    writer
        .write_image_data(&image.rgba)
        .map_err(|e| RenderError::Document(format!("png {}: {e}", path.display())))?;
    Ok(())
}

/// Per-frame comparison statistics over a pixel subset.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FrameStats {
    /// Plate frame index.
    pub frame: u32,
    /// Pixels compared.
    pub pixels: u64,
    /// Estate gate definition: fraction of pixels whose
    /// summed absolute RGB delta exceeds 0.08 ([0,1] scale).
    pub changed_fraction_gate: f64,
    /// Fraction of pixels with any channel delta > 16/255.
    pub changed_fraction_strict: f64,
    /// Mean absolute delta per channel, in 0..255 units.
    pub mean_abs_delta: [f64; 3],
    /// Max absolute delta over channels, 0..255 units.
    pub max_abs_delta: u32,
    /// Mean signed delta per channel, 0..255 units
    /// (systematic tint/scale errors surface here).
    pub mean_signed_delta: [f64; 3],
}

fn accumulate(
    frame: u32,
    pairs: impl Iterator<Item = ([u8; 4], [u8; 4])>,
    pixels: u64,
) -> FrameStats {
    let mut gate_changed = 0_u64;
    let mut strict_changed = 0_u64;
    let mut abs_sum = [0_f64; 3];
    let mut signed_sum = [0_f64; 3];
    let mut max_abs = 0_u32;
    for (mine, plate) in pairs {
        let mut summed = 0.0_f64;
        let mut strict = false;
        for ch in 0..3 {
            let d = f64::from(mine[ch]) - f64::from(plate[ch]);
            abs_sum[ch] += d.abs();
            signed_sum[ch] += d;
            summed += d.abs() / 255.0;
            if d.abs() > 16.0 {
                strict = true;
            }
            max_abs = max_abs.max(d.abs() as u32);
        }
        if summed > 0.08 {
            gate_changed += 1;
        }
        if strict {
            strict_changed += 1;
        }
    }
    let n = pixels.max(1) as f64;
    FrameStats {
        frame,
        pixels,
        changed_fraction_gate: gate_changed as f64 / n,
        changed_fraction_strict: strict_changed as f64 / n,
        mean_abs_delta: [abs_sum[0] / n, abs_sum[1] / n, abs_sum[2] / n],
        max_abs_delta: max_abs,
        mean_signed_delta: [signed_sum[0] / n, signed_sum[1] / n, signed_sum[2] / n],
    }
}

/// Full-frame statistics. Both images must share dims.
pub fn frame_stats(frame: u32, mine: &Image, plate: &Image) -> Result<FrameStats, RenderError> {
    if mine.width != plate.width || mine.height != plate.height {
        return Err(RenderError::Document(format!(
            "frame {frame}: size mismatch {}x{} vs {}x{}",
            mine.width, mine.height, plate.width, plate.height
        )));
    }
    let pixels = u64::from(mine.width) * u64::from(mine.height);
    let pairs = mine
        .rgba
        .chunks_exact(4)
        .zip(plate.rgba.chunks_exact(4))
        .map(|(a, b)| ([a[0], a[1], a[2], a[3]], [b[0], b[1], b[2], b[3]]));
    Ok(accumulate(frame, pairs, pixels))
}

/// Statistics over the pixels where `mask` is true.
pub fn masked_stats(
    frame: u32,
    mine: &Image,
    plate: &Image,
    mask: &[bool],
) -> Result<FrameStats, RenderError> {
    if mine.width != plate.width || mine.height != plate.height {
        return Err(RenderError::Document(format!(
            "frame {frame}: size mismatch",
        )));
    }
    let total = (mine.width * mine.height) as usize;
    if mask.len() != total {
        return Err(RenderError::Document(format!(
            "mask has {} entries for {total} pixels",
            mask.len()
        )));
    }
    let pixels = mask.iter().filter(|m| **m).count() as u64;
    let pairs = (0..total).filter(move |i| mask[*i]).map(move |i| {
        let at = i * 4;
        (
            [
                mine.rgba[at],
                mine.rgba[at + 1],
                mine.rgba[at + 2],
                mine.rgba[at + 3],
            ],
            [
                plate.rgba[at],
                plate.rgba[at + 1],
                plate.rgba[at + 2],
                plate.rgba[at + 3],
            ],
        )
    });
    Ok(accumulate(frame, pairs, pixels))
}

/// Character mask from a character-only render: pixels
/// that differ from the uniform clear colour, eroded by
/// one pixel so antialiased silhouette edges (where the
/// two renders' coverage legitimately differs) do not
/// dominate the masked statistics.
pub fn character_mask(char_only: &Image, clear: [u8; 4]) -> Vec<bool> {
    let (w, h) = (char_only.width as usize, char_only.height as usize);
    let mut mask = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            mask[y * w + x] = char_only.pixel(x as u32, y as u32) != clear;
        }
    }
    let mut eroded = vec![false; w * h];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            eroded[y * w + x] = mask[y * w + x]
                && mask[y * w + x - 1]
                && mask[y * w + x + 1]
                && mask[(y - 1) * w + x]
                && mask[(y + 1) * w + x];
        }
    }
    eroded
}

/// SHA-256 hex of a byte string (frame-hash witness).
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

// -- Verdict-pack image composition ----------------------

/// Nearest-neighbour upscale by an integer factor.
pub fn upscale(image: &Image, factor: u32) -> Image {
    let (w, h) = (image.width * factor, image.height * factor);
    let mut rgba = vec![0_u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let src = image.pixel(x / factor, y / factor);
            let at = ((y * w + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&src);
        }
    }
    Image {
        width: w,
        height: h,
        rgba,
    }
}

/// Crop a rectangle (clamped to the image).
pub fn crop(image: &Image, x: u32, y: u32, width: u32, height: u32) -> Image {
    let width = width.min(image.width - x.min(image.width));
    let height = height.min(image.height - y.min(image.height));
    let mut rgba = vec![0_u8; (width * height * 4) as usize];
    for row in 0..height {
        let src_at = (((y + row) * image.width + x) * 4) as usize;
        let dst_at = (row * width * 4) as usize;
        rgba[dst_at..dst_at + (width * 4) as usize]
            .copy_from_slice(&image.rgba[src_at..src_at + (width * 4) as usize]);
    }
    Image {
        width,
        height,
        rgba,
    }
}

/// Place images side by side on a dark background with a
/// fixed gap; heights must match (upscale beforehand).
pub fn side_by_side(images: &[&Image], gap: u32, background: [u8; 4]) -> Image {
    let height = images.iter().map(|i| i.height).max().unwrap_or(0);
    let width =
        images.iter().map(|i| i.width).sum::<u32>() + gap * (images.len().saturating_sub(1) as u32);
    let mut rgba = vec![0_u8; (width * height * 4) as usize];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&background);
    }
    let mut x_off = 0_u32;
    for image in images {
        for row in 0..image.height {
            let src_at = (row * image.width * 4) as usize;
            let dst_at = ((row * width + x_off) * 4) as usize;
            rgba[dst_at..dst_at + (image.width * 4) as usize]
                .copy_from_slice(&image.rgba[src_at..src_at + (image.width * 4) as usize]);
        }
        x_off += image.width + gap;
    }
    Image {
        width,
        height,
        rgba,
    }
}

/// Stack images vertically with a fixed gap.
pub fn stack(images: &[&Image], gap: u32, background: [u8; 4]) -> Image {
    let width = images.iter().map(|i| i.width).max().unwrap_or(0);
    let height = images.iter().map(|i| i.height).sum::<u32>()
        + gap * (images.len().saturating_sub(1) as u32);
    let mut rgba = vec![0_u8; (width * height * 4) as usize];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&background);
    }
    let mut y_off = 0_u32;
    for image in images {
        for row in 0..image.height {
            let src_at = (row * image.width * 4) as usize;
            let dst_at = (((y_off + row) * width) * 4) as usize;
            rgba[dst_at..dst_at + (image.width * 4) as usize]
                .copy_from_slice(&image.rgba[src_at..src_at + (image.width * 4) as usize]);
        }
        y_off += image.height + gap;
    }
    Image {
        width,
        height,
        rgba,
    }
}

/// Amplified difference heat image: |delta| * gain per
/// channel on black.
pub fn diff_heat(mine: &Image, plate: &Image, gain: u32) -> Image {
    let mut rgba = vec![0_u8; mine.rgba.len()];
    for (dst, (a, b)) in rgba
        .chunks_exact_mut(4)
        .zip(mine.rgba.chunks_exact(4).zip(plate.rgba.chunks_exact(4)))
    {
        for ch in 0..3 {
            let d = (i32::from(a[ch]) - i32::from(b[ch])).unsigned_abs() * gain;
            dst[ch] = d.min(255) as u8;
        }
        dst[3] = 255;
    }
    Image {
        width: mine.width,
        height: mine.height,
        rgba,
    }
}
