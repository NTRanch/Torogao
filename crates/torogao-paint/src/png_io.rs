//! PNG import/export (SPEC SS3.3/SS8 Phase 1): decode to
//! RGBA8 and encode from RGBA8, through the `png` crate
//! only. All size bounds are checked before allocation.

use std::io::Cursor;

use crate::document::CANVAS_DIM_MAX;
use crate::error::{PaintError, PaintResult};

/// Ceiling on a PNG file's byte length at import.
pub const PNG_FILE_BYTES_MAX: usize = 512 * 1024 * 1024;

/// Decode PNG bytes to (width, height, RGBA8 pixels).
/// Colour types are expanded to RGBA8; 16-bit channels
/// are stripped to 8 (the document is 8-bit, DP-8).
pub fn decode_png_rgba(bytes: &[u8]) -> PaintResult<(u32, u32, Vec<u8>)> {
    if bytes.len() > PNG_FILE_BYTES_MAX {
        return Err(PaintError::limit(
            "png file bytes",
            bytes.len() as u64,
            PNG_FILE_BYTES_MAX as u64,
        ));
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|err| PaintError::Png {
        message: err.to_string(),
    })?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    if width == 0 || width > CANVAS_DIM_MAX || height == 0 || height > CANVAS_DIM_MAX {
        return Err(PaintError::limit(
            "png dimension",
            u64::from(width.max(height)),
            u64::from(CANVAS_DIM_MAX),
        ));
    }
    // output_buffer_size is None for interlaced images;
    // the EXPAND|STRIP_16 output is at most 4 bytes per
    // pixel, so the canvas size is the honest bound.
    let buffer_size = reader
        .output_buffer_size()
        .unwrap_or((width as usize) * (height as usize) * 4);
    let mut buf = vec![0u8; buffer_size];
    let frame = reader.next_frame(&mut buf).map_err(|err| PaintError::Png {
        message: err.to_string(),
    })?;
    buf.truncate(frame.buffer_size());
    let pixels = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => expand_alpha(buf, 255),
        png::ColorType::GrayscaleAlpha => expand_gray_alpha(buf),
        png::ColorType::Grayscale => expand_gray(buf),
        png::ColorType::Indexed => {
            return Err(PaintError::Png {
                message: "indexed colour survived the EXPAND transform".to_string(),
            });
        }
    };
    let expected = (width as usize) * (height as usize) * 4;
    if pixels.len() != expected {
        return Err(PaintError::Png {
            message: format!("decoded {} bytes, canvas needs {expected}", pixels.len()),
        });
    }
    Ok((width, height, pixels))
}

/// RGB -> RGBA with a constant alpha.
fn expand_alpha(rgb: Vec<u8>, alpha: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity((rgb.len() / 3) * 4);
    for px in rgb.chunks_exact(3) {
        out.extend_from_slice(&[px[0], px[1], px[2], alpha]);
    }
    out
}

/// Gray+alpha -> RGBA.
fn expand_gray_alpha(ga: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity((ga.len() / 2) * 4);
    for px in ga.chunks_exact(2) {
        out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
    }
    out
}

/// Gray -> opaque RGBA.
fn expand_gray(gray: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(gray.len() * 4);
    for value in gray {
        out.extend_from_slice(&[value, value, value, 255]);
    }
    out
}

/// Encode RGBA8 pixels as PNG bytes.
pub fn encode_png_rgba(width: u32, height: u32, pixels: &[u8]) -> PaintResult<Vec<u8>> {
    let expected = (width as usize) * (height as usize) * 4;
    if pixels.len() != expected {
        return Err(PaintError::Png {
            message: format!(
                "encode given {} bytes, canvas needs {expected}",
                pixels.len()
            ),
        });
    }
    let mut out: Vec<u8> = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|err| PaintError::Png {
            message: err.to_string(),
        })?;
        writer
            .write_image_data(pixels)
            .map_err(|err| PaintError::Png {
                message: err.to_string(),
            })?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip_edge_sizes() {
        for (w, h) in [(1u32, 1u32), (257, 1), (1, 257), (3, 5), (256, 256)] {
            let pixels: Vec<u8> = (0..(w * h * 4)).map(|i| (i % 251) as u8).collect();
            let encoded = encode_png_rgba(w, h, &pixels).expect("encode");
            let (dw, dh, decoded) = decode_png_rgba(&encoded).expect("decode");
            assert_eq!((dw, dh), (w, h));
            assert_eq!(decoded, pixels);
        }
    }

    #[test]
    fn adversarial_png_inputs_refuse() {
        // Not a PNG at all.
        assert!(decode_png_rgba(b"this is not a png").is_err());
        // Empty input.
        assert!(decode_png_rgba(&[]).is_err());
        // A real PNG, truncated mid-stream.
        let encoded = encode_png_rgba(4, 4, &[7u8; 64]).expect("encode");
        assert!(decode_png_rgba(&encoded[..encoded.len() / 2]).is_err());
        // Encode with a mismatched buffer refuses before writing.
        assert!(encode_png_rgba(4, 4, &[0u8; 10]).is_err());
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;

    #[test]
    fn adversarial_png_zero_dimension_refuses() {
        // A real 1x1 PNG with its IHDR width patched to
        // zero: the decoder must refuse, never allocate
        // or panic.
        let mut bytes = encode_png_rgba(1, 1, &[9u8; 4]).expect("encode");
        // IHDR width sits at bytes 16..20 in the PNG stream.
        bytes[19] = 0;
        assert!(decode_png_rgba(&bytes).is_err());
    }
}
