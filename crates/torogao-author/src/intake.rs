//! Art intake: reading sources into observations.
//!
//! This module touches the filesystem and decodes bytes; it makes
//! no admission decisions. Its outputs (`ImageObservation`,
//! `PsdObservation`) are plain data that `decide()` judges
//! purely. The one policy function here, [`source_is_readonly`],
//! is pure and is called both before any source byte is read and
//! again inside `decide()`, so evidence files are never opened.
//!
//! Supported inputs, stated rather than implied:
//! - PNG (`intake_image`): signature and IHDR are checked
//!   (CRC included); pixels are not decoded, so a PNG asset has no
//!   content bounds and `mesh_trim` refuses it.
//! - PSD (`intake_psd`): version 1, 8 bits per channel, RGB mode,
//!   raw or RLE channel data. Each pixel layer is trimmed to its
//!   painted content (alpha > 0) and written as an RGBA PNG.
//!   Everything else is refused by name.

use crate::limits::{IMAGE_SIDE_MAX, PSD_LAYERS_MAX, TREE_DEPTH_MAX};
use crate::refusal::{Refusal, RefusalClass, refuse};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Path components that mark a source as read-only evidence.
pub const READONLY_COMPONENTS: [&str; 1] = ["evidence"];

/// True when a canonical source path lies in read-only evidence.
/// Pure: classifies the path text, reads nothing.
pub fn source_is_readonly(canonical: &Path) -> bool {
    canonical.components().any(|component| match component {
        Component::Normal(name) => READONLY_COMPONENTS
            .iter()
            .any(|marker| name.eq_ignore_ascii_case(marker)),
        _ => false,
    })
}

/// Lowercase hex sha256 of a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Resolves a source path to its canonical form (symlinks
/// resolved, so a link into evidence is classified by its target)
/// and refuses read-only sources before any content is read.
pub fn resolve_source(path: &str, base: &Path) -> Result<PathBuf, Refusal> {
    let joined = base.join(path);
    let canonical = std::fs::canonicalize(&joined).or_else(|err| {
        refuse(
            RefusalClass::Io,
            format!("source '{path}' cannot be resolved: {err}"),
        )
    })?;
    if source_is_readonly(&canonical) {
        return refuse(
            RefusalClass::ReadonlySource,
            format!(
                "source '{path}' lies in read-only evidence; studied material never enters a project"
            ),
        );
    }
    Ok(canonical)
}

/// Reads a whole file, refusing files larger than `bytes_max`
/// before reading them and never reading past the bound.
pub fn read_bounded(path: &Path, bytes_max: u64) -> Result<Vec<u8>, Refusal> {
    let io_refusal = |err: std::io::Error| {
        Refusal::new(
            RefusalClass::Io,
            format!("cannot read '{}': {err}", path.display()),
        )
    };
    let file = std::fs::File::open(path).map_err(io_refusal)?;
    let length = file.metadata().map_err(io_refusal)?.len();
    if length > bytes_max {
        return refuse(
            RefusalClass::Range,
            format!(
                "'{}' is {length} bytes; the bound is {bytes_max}",
                path.display()
            ),
        );
    }
    let mut bytes = Vec::new();
    file.take(bytes_max + 1)
        .read_to_end(&mut bytes)
        .map_err(io_refusal)?;
    if bytes.len() as u64 > bytes_max {
        return refuse(
            RefusalClass::Range,
            format!("'{}' grew past the bound while reading", path.display()),
        );
    }
    Ok(bytes)
}

/// The base name of a path, as text.
pub fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What `intake_image` observed about one PNG source.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageObservation {
    /// Canonical source path.
    pub canonical_path: String,
    /// Source base name.
    pub source_name: String,
    /// sha256 of the bytes.
    pub sha256: String,
    /// Width, pixels.
    pub width: u32,
    /// Height, pixels.
    pub height: u32,
    /// The file bytes (copied verbatim into the asset store).
    pub bytes: Vec<u8>,
}

/// Observes a PNG file's bytes: signature, IHDR, dimensions.
pub fn observe_png(canonical: &Path, bytes: Vec<u8>) -> Result<ImageObservation, Refusal> {
    let (width, height) = png_dimensions(&bytes)?;
    Ok(ImageObservation {
        canonical_path: canonical.to_string_lossy().into_owned(),
        source_name: base_name(canonical),
        sha256: sha256_hex(&bytes),
        width,
        height,
        bytes,
    })
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Reads width and height from a PNG's IHDR chunk, verifying the
/// signature, chunk layout, and CRC. Dimensions must be within
/// `1..=IMAGE_SIDE_MAX`.
pub fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), Refusal> {
    // 8 signature + 4 length + 4 type + 13 data + 4 crc.
    const IHDR_END: usize = 33;
    if bytes.len() < IHDR_END || bytes[..8] != PNG_SIGNATURE {
        return refuse(RefusalClass::UnknownKind, "source is not a PNG file");
    }
    if bytes[8..12] != 13u32.to_be_bytes() || &bytes[12..16] != b"IHDR" {
        return refuse(
            RefusalClass::Malformed,
            "PNG does not open with an IHDR chunk",
        );
    }
    let crc_stored = u32::from_be_bytes([bytes[29], bytes[30], bytes[31], bytes[32]]);
    if crc32(&bytes[12..29]) != crc_stored {
        return refuse(RefusalClass::Malformed, "PNG IHDR chunk fails its CRC");
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    check_side(width, "PNG width")?;
    check_side(height, "PNG height")?;
    Ok((width, height))
}

fn check_side(side: u32, what: &str) -> Result<(), Refusal> {
    if side == 0 || side > IMAGE_SIDE_MAX {
        return refuse(
            RefusalClass::Range,
            format!("{what} {side} is outside 1..={IMAGE_SIDE_MAX}"),
        );
    }
    Ok(())
}

/// CRC-32 (ISO 3309, the PNG chunk CRC), bitwise: inputs are
/// bounded and this runs once per chunk, so no table is kept.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let (mut low, mut high) = (1u32, 0u32);
    for byte in bytes {
        low = (low + u32::from(*byte)) % MODULUS;
        high = (high + low) % MODULUS;
    }
    (high << 16) | low
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    // Chunk data is bounded by IMAGE_SIDE_MAX^2 * 4 + rows, far
    // below u32::MAX, so the length conversion cannot truncate.
    let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&length.to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encodes 8-bit RGBA pixels as a PNG: filter 0 on every row,
/// zlib stored (uncompressed) blocks. Deterministic: the same
/// pixels always produce the same bytes.
pub fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let row_bytes = width as usize * 4;
    assert_eq!(rgba.len(), row_bytes * height as usize, "pixel buffer size");
    let mut raw = Vec::with_capacity((row_bytes + 1) * height as usize);
    for row in rgba.chunks_exact(row_bytes) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    // zlib header: deflate, 32K window, no preset dictionary,
    // check bits making 0x7801 divisible by 31.
    let mut zlib = vec![0x78, 0x01];
    const STORED_BLOCK_BYTES_MAX: usize = 65_535;
    let blocks: Vec<&[u8]> = if raw.is_empty() {
        vec![&raw[..]]
    } else {
        raw.chunks(STORED_BLOCK_BYTES_MAX).collect()
    };
    for (index, block) in blocks.iter().enumerate() {
        let last = index + 1 == blocks.len();
        zlib.push(u8::from(last));
        let length = block.len() as u16;
        zlib.extend_from_slice(&length.to_le_bytes());
        zlib.extend_from_slice(&(!length).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    // Bit depth 8, colour type 6 (RGBA), compression 0, filter 0,
    // interlace 0.
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = PNG_SIGNATURE.to_vec();
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(&mut out, b"IDAT", &zlib);
    push_chunk(&mut out, b"IEND", &[]);
    out
}

// ---- PSD ----------------------------------------------------------

/// One trimmed pixel layer, encoded.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerImage {
    /// The PNG bytes.
    pub png: Vec<u8>,
    /// sha256 of the PNG bytes.
    pub sha256: String,
    /// Trimmed width, pixels.
    pub width: u32,
    /// Trimmed height, pixels.
    pub height: u32,
    /// Top-left of the trimmed content on the PSD canvas.
    pub canvas_origin: [i64; 2],
}

/// One PSD layer record as observed (groups included).
#[derive(Clone, Debug, PartialEq)]
pub struct PsdLayer {
    /// Group names and layer name joined by `/`.
    pub path: String,
    /// The enclosing group's path, if any.
    pub group: Option<String>,
    /// True for a group (folder) record.
    pub is_group: bool,
    /// The clip base's layer path, when this layer is clipped.
    pub clip_base: Option<String>,
    /// The trimmed pixels (pixel layers only).
    pub image: Option<LayerImage>,
}

/// What `intake_psd` observed about one PSD source.
#[derive(Clone, Debug, PartialEq)]
pub struct PsdObservation {
    /// Canonical source path.
    pub canonical_path: String,
    /// Source base name.
    pub source_name: String,
    /// sha256 of the PSD bytes.
    pub sha256: String,
    /// The PSD bytes (kept in the asset store for replay).
    pub bytes: Vec<u8>,
    /// Layers, top of the stack first.
    pub layers: Vec<PsdLayer>,
}

/// A bounds-checked big-endian reader over a byte slice.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

fn truncated<T>() -> Result<T, Refusal> {
    refuse(RefusalClass::Malformed, "PSD is truncated")
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], Refusal> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len());
        match end {
            Some(end) => {
                let slice = &self.bytes[self.at..end];
                self.at = end;
                Ok(slice)
            }
            None => truncated(),
        }
    }

    fn u8(&mut self) -> Result<u8, Refusal> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Refusal> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn i16(&mut self) -> Result<i16, Refusal> {
        Ok(self.u16()? as i16)
    }

    fn u32(&mut self) -> Result<u32, Refusal> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i32(&mut self) -> Result<i32, Refusal> {
        Ok(self.u32()? as i32)
    }

    /// Skips a u32-length-prefixed section.
    fn skip_section(&mut self) -> Result<(), Refusal> {
        let length = self.u32()? as usize;
        self.take(length).map(|_| ())
    }
}

/// One layer record before pixel decode.
struct Record {
    name: String,
    top: i32,
    left: i32,
    height: u32,
    width: u32,
    channels: Vec<(i16, usize)>,
    clipped: bool,
    /// 0 plain, 1/2 group open/closed, 3 group end divider.
    section: u32,
}

/// Observes a PSD: parses layer records and trims every pixel
/// layer to content.
pub fn observe_psd(canonical: &Path, bytes: Vec<u8>) -> Result<PsdObservation, Refusal> {
    let mut cursor = Cursor {
        bytes: &bytes,
        at: 0,
    };
    read_psd_header(&mut cursor)?;
    cursor.skip_section()?; // colour mode data
    cursor.skip_section()?; // image resources
    let section_length = cursor.u32()? as usize;
    if section_length == 0 {
        return refuse(RefusalClass::Malformed, "PSD carries no layers");
    }
    let info_length = cursor.u32()? as usize;
    let info_end = cursor.at.checked_add(info_length);
    if info_length == 0 || info_end.is_none_or(|end| end > bytes.len()) {
        return refuse(
            RefusalClass::Malformed,
            "PSD layer info is empty or truncated",
        );
    }
    let count = usize::from(cursor.i16()?.unsigned_abs());
    if count == 0 || count > PSD_LAYERS_MAX {
        return refuse(
            RefusalClass::Range,
            format!("PSD layer count {count} is outside 1..={PSD_LAYERS_MAX}"),
        );
    }
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        records.push(read_record(&mut cursor)?);
    }
    let mut images = Vec::with_capacity(count);
    for record in &records {
        images.push(read_layer_pixels(&mut cursor, record)?);
    }
    let layers = structure_layers(records, images)?;
    Ok(PsdObservation {
        canonical_path: canonical.to_string_lossy().into_owned(),
        source_name: base_name(canonical),
        sha256: sha256_hex(&bytes),
        layers,
        bytes,
    })
}

fn read_psd_header(cursor: &mut Cursor<'_>) -> Result<(), Refusal> {
    if cursor.take(4)? != b"8BPS" {
        return refuse(RefusalClass::UnknownKind, "source is not a PSD file");
    }
    let version = cursor.u16()?;
    if version != 1 {
        return refuse(
            RefusalClass::UnknownKind,
            format!("PSD version {version} (PSB is 2) is not supported"),
        );
    }
    cursor.take(6)?;
    let _channels = cursor.u16()?;
    let height = cursor.u32()?;
    let width = cursor.u32()?;
    check_side(width, "PSD canvas width")?;
    check_side(height, "PSD canvas height")?;
    let depth = cursor.u16()?;
    let mode = cursor.u16()?;
    if depth != 8 || mode != 3 {
        return refuse(
            RefusalClass::UnknownKind,
            format!("PSD depth {depth} / colour mode {mode}: only 8-bit RGB (3) is supported"),
        );
    }
    Ok(())
}

fn read_record(cursor: &mut Cursor<'_>) -> Result<Record, Refusal> {
    let top = cursor.i32()?;
    let left = cursor.i32()?;
    let bottom = cursor.i32()?;
    let right = cursor.i32()?;
    let height = layer_side(top, bottom)?;
    let width = layer_side(left, right)?;
    let channel_count = usize::from(cursor.u16()?);
    if channel_count > 56 {
        return refuse(
            RefusalClass::Malformed,
            "PSD layer has more than 56 channels",
        );
    }
    let mut channels = Vec::with_capacity(channel_count);
    for _ in 0..channel_count {
        let id = cursor.i16()?;
        let length = cursor.u32()? as usize;
        channels.push((id, length));
    }
    if cursor.take(4)? != b"8BIM" {
        return refuse(
            RefusalClass::Malformed,
            "PSD layer blend signature is not 8BIM",
        );
    }
    cursor.take(4)?; // blend mode key
    let _opacity = cursor.u8()?;
    let clipped = cursor.u8()? != 0;
    cursor.take(2)?; // flags, filler
    let extra_length = cursor.u32()? as usize;
    let extra = cursor.take(extra_length)?;
    let (name, section) = read_extra(extra)?;
    Ok(Record {
        name,
        top,
        left,
        height,
        width,
        channels,
        clipped,
        section,
    })
}

fn layer_side(start: i32, end: i32) -> Result<u32, Refusal> {
    let side = i64::from(end) - i64::from(start);
    if !(0..=i64::from(IMAGE_SIDE_MAX)).contains(&side) {
        return refuse(
            RefusalClass::Range,
            format!("PSD layer side {side} is outside 0..={IMAGE_SIDE_MAX}"),
        );
    }
    Ok(side as u32)
}

/// Reads a record's extra data: mask and blending ranges are
/// skipped; the name comes from `luni` when present, else the
/// Pascal name (bytes as Latin-1); `lsct` gives the section kind.
fn read_extra(extra: &[u8]) -> Result<(String, u32), Refusal> {
    let mut cursor = Cursor {
        bytes: extra,
        at: 0,
    };
    cursor.skip_section()?; // layer mask data
    cursor.skip_section()?; // blending ranges
    let name_length = usize::from(cursor.u8()?);
    let mut name: String = cursor
        .take(name_length)?
        .iter()
        .map(|b| char::from(*b))
        .collect();
    let padded = (1 + name_length).div_ceil(4) * 4;
    cursor.take(padded - 1 - name_length)?;
    let mut section = 0;
    while cursor.at + 12 <= extra.len() {
        let signature = cursor.take(4)?;
        if signature != b"8BIM" && signature != b"8B64" {
            break;
        }
        let key: [u8; 4] = [cursor.u8()?, cursor.u8()?, cursor.u8()?, cursor.u8()?];
        let length = cursor.u32()? as usize;
        let data = cursor.take(length)?;
        match &key {
            b"lsct" if data.len() >= 4 => {
                section = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            }
            b"luni" => name = read_unicode_name(data)?,
            _ => {}
        }
    }
    Ok((name, section))
}

fn read_unicode_name(data: &[u8]) -> Result<String, Refusal> {
    let mut cursor = Cursor { bytes: data, at: 0 };
    let units = cursor.u32()? as usize;
    let raw = cursor.take(units.checked_mul(2).ok_or_else(|| {
        Refusal::new(
            RefusalClass::Malformed,
            "PSD unicode layer name length overflows",
        )
    })?)?;
    let code_units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16(&code_units)
        .or_else(|_| refuse(RefusalClass::Malformed, "PSD unicode layer name is invalid"))
}

/// Reads one layer's channel data and returns its trimmed image
/// (None for group records and dividers).
fn read_layer_pixels(
    cursor: &mut Cursor<'_>,
    record: &Record,
) -> Result<Option<LayerImage>, Refusal> {
    let width = record.width as usize;
    let height = record.height as usize;
    let pixels = width * height;
    let is_pixel_layer = record.section == 0;
    let mut rgba = if is_pixel_layer {
        vec![255u8; pixels * 4]
    } else {
        Vec::new()
    };
    for (id, length) in &record.channels {
        let data = cursor.take(*length)?;
        let slot = match id {
            0 => Some(0),
            1 => Some(1),
            2 => Some(2),
            -1 => Some(3),
            _ => None,
        };
        let Some(slot) = slot.filter(|_| is_pixel_layer && pixels > 0) else {
            continue;
        };
        let plane = decode_channel(data, width, height)?;
        for (index, value) in plane.into_iter().enumerate() {
            rgba[index * 4 + slot] = value;
        }
    }
    if !is_pixel_layer {
        return Ok(None);
    }
    trim_to_content(record, &rgba).map(Some)
}

fn decode_channel(data: &[u8], width: usize, height: usize) -> Result<Vec<u8>, Refusal> {
    let mut cursor = Cursor { bytes: data, at: 0 };
    let compression = cursor.u16()?;
    match compression {
        0 => Ok(cursor.take(width * height)?.to_vec()),
        1 => {
            let mut row_lengths = Vec::with_capacity(height);
            for _ in 0..height {
                row_lengths.push(usize::from(cursor.u16()?));
            }
            let mut plane = Vec::with_capacity(width * height);
            for row_length in row_lengths {
                unpack_bits(cursor.take(row_length)?, width, &mut plane)?;
            }
            Ok(plane)
        }
        other => refuse(
            RefusalClass::UnknownKind,
            format!("PSD channel compression {other} (zip) is not supported"),
        ),
    }
}

/// PackBits decode of one row into exactly `width` bytes.
fn unpack_bits(row: &[u8], width: usize, out: &mut Vec<u8>) -> Result<(), Refusal> {
    let target = out.len() + width;
    let mut at = 0;
    while at < row.len() {
        let header = row[at] as i8;
        at += 1;
        if header >= 0 {
            let count = header as usize + 1;
            let Some(literal) = row.get(at..at + count) else {
                return truncated();
            };
            out.extend_from_slice(literal);
            at += count;
        } else if header != -128 {
            let count = 1 - header as isize;
            let Some(value) = row.get(at) else {
                return truncated();
            };
            out.extend(std::iter::repeat_n(*value, count as usize));
            at += 1;
        }
        if out.len() > target {
            return refuse(RefusalClass::Malformed, "PSD RLE row overruns its width");
        }
    }
    if out.len() != target {
        return refuse(RefusalClass::Malformed, "PSD RLE row is short of its width");
    }
    Ok(())
}

fn trim_to_content(record: &Record, rgba: &[u8]) -> Result<LayerImage, Refusal> {
    let width = record.width as usize;
    let height = record.height as usize;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..height {
        for x in 0..width {
            if rgba[(y * width + x) * 4 + 3] > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x0 == usize::MAX {
        return refuse(
            RefusalClass::Malformed,
            format!("PSD layer '{}' has no painted content", record.name),
        );
    }
    let (trim_width, trim_height) = (x1 - x0, y1 - y0);
    let mut trimmed = Vec::with_capacity(trim_width * trim_height * 4);
    for y in y0..y1 {
        trimmed.extend_from_slice(&rgba[(y * width + x0) * 4..(y * width + x1) * 4]);
    }
    // Trimmed sides are at most the record sides, which are
    // bounded by IMAGE_SIDE_MAX.
    let png = encode_png_rgba(trim_width as u32, trim_height as u32, &trimmed);
    Ok(LayerImage {
        sha256: sha256_hex(&png),
        png,
        width: trim_width as u32,
        height: trim_height as u32,
        canvas_origin: [
            i64::from(record.left) + x0 as i64,
            i64::from(record.top) + y0 as i64,
        ],
    })
}

/// Builds layer paths, groups, and clip bases. Records arrive
/// bottom of the stack first; groups are a divider record (3)
/// below their children and the folder record (1/2) above them.
fn structure_layers(
    records: Vec<Record>,
    images: Vec<Option<LayerImage>>,
) -> Result<Vec<PsdLayer>, Refusal> {
    // Clip bases resolve bottom-up: a clipped layer's base is the
    // nearest unclipped pixel layer below it in the same group.
    let mut clip_base_index: Vec<Option<usize>> = vec![None; records.len()];
    let mut last_base: Option<usize> = None;
    for (index, record) in records.iter().enumerate() {
        match (record.section, record.clipped) {
            (0, false) => last_base = Some(index),
            (0, true) => match last_base {
                Some(base) => clip_base_index[index] = Some(base),
                None => {
                    return refuse(
                        RefusalClass::Malformed,
                        format!(
                            "PSD layer '{}' is clipped with no base below it",
                            record.name
                        ),
                    );
                }
            },
            _ => last_base = None,
        }
    }
    // Paths resolve top-down: walk from the top of the stack.
    let mut paths: Vec<Option<(String, Option<String>)>> = vec![None; records.len()];
    let mut groups: Vec<String> = Vec::new();
    for index in (0..records.len()).rev() {
        let record = &records[index];
        if record.section == 3 {
            if groups.pop().is_none() {
                return refuse(RefusalClass::Malformed, "PSD group divider without a group");
            }
            continue;
        }
        if record.name.is_empty() || record.name.contains('/') {
            return refuse(
                RefusalClass::Malformed,
                format!("PSD layer name '{}' is empty or contains '/'", record.name),
            );
        }
        let parent = groups.last().cloned();
        let path = match &parent {
            Some(group) => format!("{group}/{}", record.name),
            None => record.name.clone(),
        };
        if matches!(record.section, 1 | 2) {
            if groups.len() >= TREE_DEPTH_MAX {
                return refuse(
                    RefusalClass::Range,
                    "PSD group nesting exceeds the depth bound",
                );
            }
            groups.push(path.clone());
        }
        paths[index] = Some((path, parent));
    }
    if !groups.is_empty() {
        return refuse(RefusalClass::Malformed, "PSD group is never closed");
    }
    let mut layers = Vec::with_capacity(records.len());
    for index in (0..records.len()).rev() {
        let Some((path, group)) = paths[index].clone() else {
            continue;
        };
        let clip_base = clip_base_index[index]
            .and_then(|base| paths[base].as_ref().map(|(base_path, _)| base_path.clone()));
        layers.push(PsdLayer {
            path,
            group,
            is_group: records[index].section != 0,
            clip_base,
            image: images[index].clone(),
        });
    }
    let mut seen: Vec<&str> = layers.iter().map(|layer| layer.path.as_str()).collect();
    seen.sort_unstable();
    if let Some(pair) = seen.windows(2).find(|pair| pair[0] == pair[1]) {
        return refuse(
            RefusalClass::Malformed,
            format!("PSD carries two layers at path '{}'", pair[0]),
        );
    }
    Ok(layers)
}

#[cfg(test)]
mod tests {
    use super::{crc32, encode_png_rgba, png_dimensions, source_is_readonly};
    use std::path::Path;

    #[test]
    fn crc32_matches_the_reference_check_value() {
        // The standard CRC-32 check value for "123456789".
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn encoded_png_reads_back_its_dimensions() {
        let png = encode_png_rgba(3, 2, &[7u8; 3 * 2 * 4]);
        assert_eq!(png_dimensions(&png).expect("valid"), (3, 2));
        assert_eq!(
            png,
            encode_png_rgba(3, 2, &[7u8; 3 * 2 * 4]),
            "deterministic"
        );
    }

    #[test]
    fn corrupt_png_header_refuses() {
        let mut png = encode_png_rgba(1, 1, &[0u8; 4]);
        png[17] ^= 0xff;
        assert_eq!(
            png_dimensions(&png).expect_err("crc").code(),
            "TG-AUTH-MALFORMED"
        );
        assert_eq!(
            png_dimensions(b"GIF89a....................................")
                .expect_err("kind")
                .code(),
            "TG-AUTH-UNKNOWN-KIND"
        );
    }

    #[test]
    fn evidence_components_are_readonly() {
        assert!(source_is_readonly(Path::new("/a/evidence/b.png")));
        assert!(source_is_readonly(Path::new("/a/Evidence/b.png")));
        assert!(!source_is_readonly(Path::new("/a/evidenced/b.png")));
    }
}
