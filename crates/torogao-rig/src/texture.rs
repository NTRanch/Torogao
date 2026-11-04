//! Texture storage and decoding.
//!
//! Textures are RGBA8 with rows stored top-down. Sampling is
//! bilinear with CLAMP_TO_BORDER semantics (a tap outside the image
//! reads transparent black), matching the reference route's GL
//! texture parameters (LINEAR min/mag filters, border (0,0,0,0)).
//!
//! PNG payloads decode through the `image` crate, the same decoder
//! family the reference route uses. TGA payloads decode here: the
//! reference route's reader is line-buffered, and its RLE packet
//! handling re-reads the run pixel when a packet continues onto the
//! next scanline chunk; this decoder mirrors that structure so its
//! output is byte-identical on every input, and counts the packets
//! that cross a scanline for the census.

use crate::error::RigError;

/// A decoded RGBA8 texture, rows top-down.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// RGBA bytes, `width * height * 4`, row-major from the top row.
    pub rgba: Vec<u8>,
}

impl Texture {
    /// Bilinear sample at normalized UV with CLAMP_TO_BORDER:
    /// texel centers sit at half-integer coordinates, and any tap
    /// outside the image contributes transparent black.
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let fx = u * self.width as f32 - 0.5;
        let fy = v * self.height as f32 - 0.5;
        let x0 = fx.floor() as i32;
        let y0 = fy.floor() as i32;
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;
        let mut out = [0.0f32; 4];
        for (dx, wx) in [(0i32, 1.0 - tx), (1i32, tx)] {
            for (dy, wy) in [(0i32, 1.0 - ty), (1i32, ty)] {
                let w = wx * wy;
                if w == 0.0 {
                    continue;
                }
                let texel = self.texel(x0 + dx, y0 + dy);
                for c in 0..4 {
                    out[c] += texel[c] * w;
                }
            }
        }
        out
    }

    /// One texel as normalized floats; out-of-range reads the
    /// transparent border color.
    fn texel(&self, x: i32, y: i32) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let idx = ((y as u32 * self.width + x as u32) * 4) as usize;
        [
            self.rgba[idx] as f32 / 255.0,
            self.rgba[idx + 1] as f32 / 255.0,
            self.rgba[idx + 2] as f32 / 255.0,
            self.rgba[idx + 3] as f32 / 255.0,
        ]
    }
}

/// Decode outcome for the census: how many TGA RLE packets crossed a
/// scanline boundary while decoding this texture set.
#[derive(Debug, Clone, Copy, Default)]
pub struct DecodeStats {
    /// RLE packets that continued past the end of a scanline chunk.
    pub rle_line_crossings: usize,
}

/// Decode one texture payload. `encoding` is the container's
/// per-texture byte: 0 = PNG, 1 = TGA, 2 = BC7 (refused: the
/// reference route cannot load BC7 either, so refusal is parity).
pub fn decode_texture(
    bytes: &[u8],
    encoding: u8,
    stats: &mut DecodeStats,
) -> Result<Texture, RigError> {
    match encoding {
        0 => decode_png(bytes),
        1 => decode_tga(bytes, stats),
        2 => Err(RigError::new(
            "TRG-INP-004",
            "BC7 texture encoding is not supported by this evaluator",
        )),
        other => Err(RigError::new(
            "TRG-INP-004",
            format!("unknown texture encoding {other}"),
        )),
    }
}

/// PNG decode via the `image` crate, converted to RGBA8.
fn decode_png(bytes: &[u8]) -> Result<Texture, RigError> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|e| RigError::new("TRG-INP-005", format!("PNG decode failed: {e}")))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(Texture {
        width,
        height,
        rgba: rgba.into_raw(),
    })
}

/// TGA decode with the reference reader's semantics: true-color and
/// grayscale, raw or RLE packets, rows flipped unless the header's
/// origin bit marks top origin, right-to-left and interlaced and
/// paletted images refused.
fn decode_tga(bytes: &[u8], stats: &mut DecodeStats) -> Result<Texture, RigError> {
    let bad = |msg: &str| RigError::new("TRG-INP-005", format!("TGA decode failed: {msg}"));
    if bytes.len() < 18 {
        return Err(bad("header truncated"));
    }
    let id_len = bytes[0] as usize;
    let palette_type = bytes[1];
    let data_type = bytes[2];
    let width = u16::from_le_bytes([bytes[12], bytes[13]]) as usize;
    let height = u16::from_le_bytes([bytes[14], bytes[15]]) as usize;
    let bits_pp = bytes[16] as usize;
    let flags = bytes[17];
    if width < 1 || height < 1 || palette_type > 1 {
        return Err(bad("invalid header"));
    }
    if palette_type != 0 {
        return Err(bad("paletted images unsupported"));
    }
    if flags & 0xC0 != 0 {
        return Err(bad("interlaced images unsupported"));
    }
    if flags & 0x10 != 0 {
        return Err(bad("right-to-left images unsupported"));
    }
    let attr_bits = flags & 0x0F;
    if attr_bits != 0 && attr_bits != 8 {
        return Err(bad("alpha bits per pixel other than 0 or 8"));
    }
    let channels = match (data_type, bits_pp) {
        (2, 24) | (10, 24) => 3usize,
        (2, 32) | (10, 32) => 4usize,
        (3, 8) | (11, 8) => 1usize,
        (3, 16) | (11, 16) if attr_bits == 8 => 2usize,
        _ => return Err(bad("unsupported data type or bit depth")),
    };
    let is_rle = matches!(data_type, 10 | 11);
    let origin_at_top = flags & 0x20 != 0;
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| bad("image dimensions overflow"))?;
    if pixel_count > (1 << 28) {
        return Err(bad("image too big"));
    }
    let mut data = vec![0u8; pixel_count * 4];
    let line_bytes = width * channels;
    let start = 18usize
        .checked_add(id_len)
        .ok_or_else(|| bad("header id overflows"))?;
    let mut reader = ByteReader {
        bytes,
        cursor: start,
    };
    let mut linebuf = vec![0u8; line_bytes];
    let mut packet_len = 0usize; // remaining bytes of the open packet, in source bytes
    let mut packet_is_rle = false;
    for row in 0..height {
        if is_rle {
            let mut wanted = line_bytes;
            while wanted > 0 {
                if packet_len == 0 {
                    let head = reader.take(1)?[0];
                    packet_is_rle = head & 0x80 != 0;
                    packet_len = ((head & 0x7F) as usize + 1) * channels;
                } else if packet_is_rle {
                    // The packet continues onto this chunk: this is
                    // the crossing the census counts.
                    stats.rle_line_crossings += 1;
                }
                let gotten = line_bytes - wanted;
                let copy_size = wanted.min(packet_len);
                if packet_is_rle {
                    // Reference structure: the run pixel is read at
                    // every chunk, not carried across chunks.
                    let pixel = reader.take(channels)?.to_vec();
                    let mut p = gotten;
                    while p < gotten + copy_size {
                        linebuf[p..p + channels].copy_from_slice(&pixel);
                        p += channels;
                    }
                } else {
                    let raw = reader.take(copy_size)?.to_vec();
                    linebuf[gotten..gotten + copy_size].copy_from_slice(&raw);
                }
                wanted -= copy_size;
                packet_len -= copy_size;
            }
        } else {
            let raw = reader.take(line_bytes)?.to_vec();
            linebuf.copy_from_slice(&raw);
        }
        let dest_row = if origin_at_top { row } else { height - 1 - row };
        let dest = dest_row * width * 4;
        convert_line(channels, &linebuf, &mut data[dest..dest + width * 4]);
    }
    Ok(Texture {
        width: width as u32,
        height: height as u32,
        rgba: data,
    })
}

/// A cursor over the input bytes with checked takes.
struct ByteReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> ByteReader<'a> {
    /// Take the next `n` bytes; truncated input is a typed error.
    fn take(&mut self, n: usize) -> Result<&'a [u8], RigError> {
        let end = self
            .cursor
            .checked_add(n)
            .ok_or_else(|| RigError::new("TRG-INP-005", "TGA packet overruns input"))?;
        let slice = self
            .bytes
            .get(self.cursor..end)
            .ok_or_else(|| RigError::new("TRG-INP-005", "TGA packet overruns input"))?;
        self.cursor = end;
        Ok(slice)
    }
}

/// Convert one source scanline (BGR(A) / gray / gray+alpha order)
/// into RGBA.
fn convert_line(channels: usize, src: &[u8], dest: &mut [u8]) {
    match channels {
        1 => {
            for (i, &y) in src.iter().enumerate() {
                let t = i * 4;
                dest[t] = y;
                dest[t + 1] = y;
                dest[t + 2] = y;
                dest[t + 3] = 255;
            }
        }
        2 => {
            for i in 0..src.len() / 2 {
                let (k, t) = (i * 2, i * 4);
                dest[t] = src[k];
                dest[t + 1] = src[k];
                dest[t + 2] = src[k];
                dest[t + 3] = src[k + 1];
            }
        }
        3 => {
            for i in 0..src.len() / 3 {
                let (k, t) = (i * 3, i * 4);
                dest[t] = src[k + 2];
                dest[t + 1] = src[k + 1];
                dest[t + 2] = src[k];
                dest[t + 3] = 255;
            }
        }
        _ => {
            for i in 0..src.len() / 4 {
                let (k, t) = (i * 4, i * 4);
                dest[t] = src[k + 2];
                dest[t + 1] = src[k + 1];
                dest[t + 2] = src[k];
                dest[t + 3] = src[k + 3];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny uncompressed bottom-origin 32bpp TGA:
    /// stored row 0 (red, green) is the image's bottom row.
    fn tiny_tga() -> Vec<u8> {
        let mut bytes = vec![0u8; 18];
        bytes[2] = 2; // true color
        bytes[12] = 2; // width 2
        bytes[14] = 2; // height 2
        bytes[16] = 32; // bpp
        bytes[17] = 8; // 8 alpha bits, bottom origin
        // BGRA pixels, bottom row first: red, green; top row: blue, white
        bytes.extend_from_slice(&[0, 0, 255, 255, 0, 255, 0, 255]);
        bytes.extend_from_slice(&[255, 0, 0, 255, 255, 255, 255, 255]);
        bytes
    }

    #[test]
    fn tga_bottom_origin_flips_to_top_down_rgba() {
        let mut stats = DecodeStats::default();
        let tex = decode_tga(&tiny_tga(), &mut stats).expect("tiny tga decodes");
        assert_eq!((tex.width, tex.height), (2, 2));
        // Top row first in memory: blue, white, then red, green.
        assert_eq!(&tex.rgba[0..4], &[0, 0, 255, 255]);
        assert_eq!(&tex.rgba[4..8], &[255, 255, 255, 255]);
        assert_eq!(&tex.rgba[8..12], &[255, 0, 0, 255]);
        assert_eq!(&tex.rgba[12..16], &[0, 255, 0, 255]);
    }

    #[test]
    fn sample_uses_border_outside_and_texel_centers_inside() {
        let mut stats = DecodeStats::default();
        let tex = decode_tga(&tiny_tga(), &mut stats).expect("tiny tga decodes");
        let outside = tex.sample(-1.0, 0.5);
        assert_eq!(outside, [0.0, 0.0, 0.0, 0.0]);
        let center_blue = tex.sample(0.25, 0.25);
        assert!(center_blue[2] > 0.99 && center_blue[3] > 0.99);
    }
}
