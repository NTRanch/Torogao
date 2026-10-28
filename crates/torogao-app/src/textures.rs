//! Decoding of embedded texture payloads to RGBA8.
//!
//! Container texture tags: 0 = PNG (decoded with the `png`
//! crate), 1 = TGA (decoded here: uncompressed and
//! run-length true-color and grayscale, 8/24/32 bits — the
//! forms the model corpus carries), 2 = BC7 (refused with a
//! typed error: no decoder in this slice). Every decode is
//! bounded before allocation: dimensions and pixel counts
//! are checked against fixed maxima, and RLE packets are
//! bounded by the remaining input and the output size.

use torogao_format::TextureSlot;

use crate::error::{AppError, AppResult};

/// Largest accepted texture width or height, in pixels.
pub const TEXTURE_SIDE_MAX: u32 = 8192;
/// Largest accepted texture area, in pixels (64 Mi pixels).
pub const TEXTURE_PIXELS_MAX: u32 = 1 << 26;

/// One decoded RGBA8 texture, rows top-to-bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA bytes, `width * height * 4`, row-major from the
    /// top row.
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    /// Builds an image, asserting the buffer matches the
    /// dimensions exactly.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> RgbaImage {
        assert_eq!(
            pixels.len(),
            (width as usize) * (height as usize) * 4,
            "RGBA buffer must match its dimensions"
        );
        RgbaImage {
            width,
            height,
            pixels,
        }
    }

    /// A solid single-colour image (the renderer's fallback
    /// for a slot that cannot be decoded at draw time).
    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        let count = (width as usize) * (height as usize);
        let mut pixels = Vec::with_capacity(count * 4);
        for _ in 0..count {
            pixels.extend_from_slice(&rgba);
        }
        RgbaImage::new(width, height, pixels)
    }
}

fn check_dimensions(width: u32, height: u32) -> AppResult<usize> {
    if width == 0 || height == 0 || width > TEXTURE_SIDE_MAX || height > TEXTURE_SIDE_MAX {
        return Err(AppError::Render(format!(
            "texture dimensions {width}x{height} are outside 1..={TEXTURE_SIDE_MAX} per side"
        )));
    }
    let pixels = (width as u64) * u64::from(height);
    if pixels > u64::from(TEXTURE_PIXELS_MAX) {
        return Err(AppError::Render(format!(
            "texture area {pixels} pixels exceeds the {TEXTURE_PIXELS_MAX} pixel limit"
        )));
    }
    Ok(pixels as usize)
}

/// Decodes one embedded texture slot to RGBA8.
pub fn decode_texture(slot: &TextureSlot) -> AppResult<RgbaImage> {
    match slot.tag {
        0 => decode_png(&slot.data),
        1 => decode_tga(&slot.data),
        2 => Err(AppError::Render(
            "BC7 texture slot cannot be decoded in this slice".to_string(),
        )),
        other => Err(AppError::Render(format!(
            "texture slot carries unknown format tag {other}"
        ))),
    }
}

fn decode_png(data: &[u8]) -> AppResult<RgbaImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .map_err(|err| AppError::Render(format!("PNG header refused: {err}")))?;
    let info = reader.info();
    let pixel_count = check_dimensions(info.width, info.height)?;
    // The buffer is sized for the largest EXPAND output
    // (RGBA8); every 8-bit frame type fits inside it.
    let mut buffer = vec![0_u8; pixel_count * 4];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|err| AppError::Render(format!("PNG data refused: {err}")))?;
    // After EXPAND, frames are 8-bit Gray, GrayAlpha, RGB,
    // or RGBA; normalize all four to RGBA.
    let out_size = frame.buffer_size();
    match frame.color_type {
        png::ColorType::Rgba => {
            buffer.truncate(out_size);
            Ok(RgbaImage::new(frame.width, frame.height, buffer))
        }
        png::ColorType::Rgb => Ok(expand_rgb(&buffer[..out_size], frame.width, frame.height)),
        png::ColorType::Grayscale => Ok(expand_gray(
            &buffer[..out_size],
            frame.width,
            frame.height,
            false,
        )),
        png::ColorType::GrayscaleAlpha => Ok(expand_gray(
            &buffer[..out_size],
            frame.width,
            frame.height,
            true,
        )),
        other => Err(AppError::Render(format!(
            "PNG colour type {other:?} is not supported"
        ))),
    }
}

fn expand_rgb(data: &[u8], width: u32, height: u32) -> RgbaImage {
    let mut out = Vec::with_capacity(data.len() / 3 * 4);
    for pixel in data.chunks_exact(3) {
        out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
    }
    RgbaImage::new(width, height, out)
}

fn expand_gray(data: &[u8], width: u32, height: u32, alpha: bool) -> RgbaImage {
    let step = if alpha { 2 } else { 1 };
    let mut out = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for pixel in data.chunks_exact(step) {
        let a = if alpha { pixel[1] } else { 255 };
        out.extend_from_slice(&[pixel[0], pixel[0], pixel[0], a]);
    }
    RgbaImage::new(width, height, out)
}

/// The subset of the TGA header this decoder reads.
struct TgaHeader {
    id_length: usize,
    image_type: u8,
    width: u32,
    height: u32,
    bits_per_pixel: u32,
    top_origin: bool,
    right_origin: bool,
}

fn parse_tga_header(data: &[u8]) -> AppResult<TgaHeader> {
    if data.len() < 18 {
        return Err(AppError::Render(
            "TGA payload is shorter than its header".to_string(),
        ));
    }
    let color_map_type = data[1];
    if color_map_type != 0 {
        return Err(AppError::Render(
            "colour-mapped TGA textures are not supported".to_string(),
        ));
    }
    let width = u32::from(u16::from_le_bytes([data[12], data[13]]));
    let height = u32::from(u16::from_le_bytes([data[14], data[15]]));
    let descriptor = data[17];
    Ok(TgaHeader {
        id_length: usize::from(data[0]),
        image_type: data[2],
        width,
        height,
        bits_per_pixel: u32::from(data[16]),
        top_origin: descriptor & 0x20 != 0,
        right_origin: descriptor & 0x10 != 0,
    })
}

fn decode_tga(data: &[u8]) -> AppResult<RgbaImage> {
    let header = parse_tga_header(data)?;
    let run_length = match header.image_type {
        2 | 3 => false,
        10 | 11 => true,
        other => {
            return Err(AppError::Render(format!(
                "TGA image type {other} is not supported"
            )));
        }
    };
    let bytes_per_pixel = match header.bits_per_pixel {
        8 | 24 | 32 => (header.bits_per_pixel / 8) as usize,
        other => {
            return Err(AppError::Render(format!(
                "TGA bit depth {other} is not supported"
            )));
        }
    };
    let pixel_count = check_dimensions(header.width, header.height)?;
    let body_start = 18_usize
        .checked_add(header.id_length)
        .filter(|start| *start <= data.len())
        .ok_or_else(|| AppError::Render("TGA image id overruns the payload".to_string()))?;
    let body = &data[body_start..];
    let mut stored = vec![0_u8; pixel_count * 4];
    if run_length {
        decode_tga_rle(body, bytes_per_pixel, pixel_count, &mut stored)?;
    } else {
        decode_tga_raw(body, bytes_per_pixel, pixel_count, &mut stored)?;
    }
    orient_tga(&header, pixel_count, &mut stored);
    Ok(RgbaImage::new(header.width, header.height, stored))
}

/// Converts one stored pixel (BGR(A) or gray) to RGBA in
/// place order: writes at `out`.
fn write_tga_pixel(source: &[u8], bytes_per_pixel: usize, out: &mut [u8]) {
    match bytes_per_pixel {
        1 => out.copy_from_slice(&[source[0], source[0], source[0], 255]),
        3 => out.copy_from_slice(&[source[2], source[1], source[0], 255]),
        _ => out.copy_from_slice(&[source[2], source[1], source[0], source[3]]),
    }
}

fn decode_tga_raw(
    body: &[u8],
    bytes_per_pixel: usize,
    pixel_count: usize,
    out: &mut [u8],
) -> AppResult<()> {
    let needed = pixel_count * bytes_per_pixel;
    if body.len() < needed {
        return Err(AppError::Render("TGA pixel data is truncated".to_string()));
    }
    for index in 0..pixel_count {
        let source = &body[index * bytes_per_pixel..(index + 1) * bytes_per_pixel];
        write_tga_pixel(source, bytes_per_pixel, &mut out[index * 4..index * 4 + 4]);
    }
    Ok(())
}

fn decode_tga_rle(
    body: &[u8],
    bytes_per_pixel: usize,
    pixel_count: usize,
    out: &mut [u8],
) -> AppResult<()> {
    let mut written = 0_usize;
    let mut at = 0_usize;
    while written < pixel_count {
        if at >= body.len() {
            return Err(AppError::Render("TGA RLE stream ends early".to_string()));
        }
        let packet = body[at];
        at += 1;
        let count = usize::from(packet & 0x7f) + 1;
        if written + count > pixel_count {
            return Err(AppError::Render(
                "TGA RLE packet overruns the image".to_string(),
            ));
        }
        if packet & 0x80 != 0 {
            if at + bytes_per_pixel > body.len() {
                return Err(AppError::Render(
                    "TGA RLE run pixel is truncated".to_string(),
                ));
            }
            let source = &body[at..at + bytes_per_pixel];
            let mut pixel = [0_u8; 4];
            write_tga_pixel(source, bytes_per_pixel, &mut pixel);
            at += bytes_per_pixel;
            for _ in 0..count {
                out[written * 4..written * 4 + 4].copy_from_slice(&pixel);
                written += 1;
            }
        } else {
            let bytes = count * bytes_per_pixel;
            if at + bytes > body.len() {
                return Err(AppError::Render(
                    "TGA RLE raw packet is truncated".to_string(),
                ));
            }
            for index in 0..count {
                let source =
                    &body[at + index * bytes_per_pixel..at + (index + 1) * bytes_per_pixel];
                write_tga_pixel(
                    source,
                    bytes_per_pixel,
                    &mut out[(written + index) * 4..(written + index) * 4 + 4],
                );
            }
            at += bytes;
            written += count;
        }
    }
    Ok(())
}

/// Flips the stored rows/columns into top-left origin order
/// per the header's origin bits (stored order is bottom-up,
/// left-to-right unless the bits say otherwise).
fn orient_tga(header: &TgaHeader, pixel_count: usize, pixels: &mut [u8]) {
    let width = header.width as usize;
    let height = header.height as usize;
    debug_assert_eq!(pixel_count, width * height);
    if !header.top_origin {
        for row in 0..height / 2 {
            let top = row * width * 4;
            let bottom = (height - 1 - row) * width * 4;
            for i in 0..width * 4 {
                pixels.swap(top + i, bottom + i);
            }
        }
    }
    if header.right_origin {
        for row in 0..height {
            let base = row * width * 4;
            for col in 0..width / 2 {
                let left = base + col * 4;
                let right = base + (width - 1 - col) * 4;
                for i in 0..4 {
                    pixels.swap(left + i, right + i);
                }
            }
        }
    }
}

/// Encodes an RGBA image as a PNG file body.
pub fn encode_png(image: &RgbaImage) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|err| AppError::Render(format!("PNG encode failed: {err}")))?;
        writer
            .write_image_data(&image.pixels)
            .map_err(|err| AppError::Render(format!("PNG encode failed: {err}")))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{decode_tga, encode_png};
    use crate::textures::RgbaImage;

    fn tga_fixture(image_type: u8, descriptor: u8, body: &[u8]) -> Vec<u8> {
        let mut data = vec![0_u8; 18];
        data[2] = image_type;
        data[12] = 2; // width 2
        data[14] = 2; // height 2
        data[16] = 24;
        data[17] = descriptor;
        data.extend_from_slice(body);
        data
    }

    #[test]
    fn decodes_uncompressed_tga_bottom_up() {
        // Stored rows are bottom-first: bottom row blue,
        // white; top row red, green (BGR byte order).
        let body: [u8; 12] = [
            255, 0, 0, 255, 255, 255, // bottom: blue, white
            0, 0, 255, 0, 255, 0, // top: red, green
        ];
        let image = decode_tga(&tga_fixture(2, 0, &body)).expect("decodes");
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(
            image.pixels,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, // top row: red, green
                0, 0, 255, 255, 255, 255, 255, 255, // bottom: blue, white
            ]
        );
    }

    #[test]
    fn decodes_rle_tga_top_down() {
        // One raw packet of 2 (top row), one run packet of 2
        // (bottom row), descriptor marks top origin.
        let body: [u8; 11] = [
            0x01, 0, 0, 255, 0, 255, 0, // raw: red, green
            0x81, 255, 0, 0, // run: blue x2
        ];
        let image = decode_tga(&tga_fixture(10, 0x20, &body)).expect("decodes");
        assert_eq!(
            image.pixels,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255,
            ]
        );
    }

    #[test]
    fn refuses_truncated_and_overrunning_tga() {
        assert!(decode_tga(&tga_fixture(2, 0, &[0, 0, 255])).is_err());
        // RLE packet claims 128 pixels in a 2x2 image.
        assert!(decode_tga(&tga_fixture(10, 0x20, &[0xff, 1, 2, 3])).is_err());
    }

    #[test]
    fn png_round_trip_preserves_pixels() {
        let image = RgbaImage::solid(3, 2, [9, 8, 7, 6]);
        let encoded = encode_png(&image).expect("encodes");
        let slot = torogao_format::TextureSlot {
            tag: 0,
            data: encoded,
        };
        let decoded = super::decode_texture(&slot).expect("decodes");
        assert_eq!(decoded, image);
    }
}
