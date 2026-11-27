//! Minimal `.kra` reader for the drawn-clip importer
//! (torogao-drawn-clip spike, 2026-11-27).
//!
//! Reads the container directly — zip entries, `maindoc.xml`,
//! per-layer keyframes XML, and Krita's tile-stream layer
//! payloads — with no Krita process anywhere in the path.
//! Anything the reader does not map is a typed refusal
//! (`TG-KRA-*`), never a silent approximation: non-paint
//! layers, blend modes beyond `normal`, non-zero layer or
//! keyframe offsets, unknown tile-stream versions, and
//! non-RGBA colour spaces all refuse by name.
//!
//! The decode law implemented here (zip of deflate entries;
//! tile stream `VERSION 2` with 64x64x4 tiles; each tile
//! record is a 1-byte marker `0x01` plus a liblzf stream;
//! decompressed tiles are four channel planes in B, G, R, A
//! order; keyframes hold until the next key; layers composite
//! source-over, bottom first) was proven byte-exact against
//! Krita 6.0.5's own rendered frames of the witness document
//! before this port was written: all ten frames of the
//! pass-1 blink loop matched at max delta 0 (spike record,
//! `~/workspace/torogao-drawn-spike-20261127/RECORD.md`).

use std::collections::BTreeSet;
use std::fmt;
use std::io::{Cursor, Read};

use sha2::{Digest, Sha256};

/// The reader's own canvas sanity cap (the importer's
/// format cap is tighter; this only refuses absurd headers).
pub const KRA_DIM_MAX: u32 = 16_384;

/// Tile geometry of the `VERSION 2` tile stream.
const TILE_PX: usize = 64;
const PIXEL_SIZE: usize = 4;
const TILE_BYTES: usize = TILE_PX * TILE_PX * PIXEL_SIZE;

/// A typed refusal from the `.kra` reader: a stable
/// `TG-KRA-` code plus a human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KraError {
    code: &'static str,
    message: String,
}

impl KraError {
    fn new(code: &'static str, message: impl Into<String>) -> KraError {
        KraError {
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

impl fmt::Display for KraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for KraError {}

/// The reader's result type.
pub type KraResult<T> = Result<T, KraError>;

/// One decoded raster: RGBA8, row-major, canvas-sized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KraRaster {
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of RGBA8.
    pub rgba: Vec<u8>,
}

impl KraRaster {
    /// Lowercase hex SHA-256 of the RGBA bytes: the
    /// drawing identity the drawn-clip format names.
    pub fn sha256_hex(&self) -> String {
        let digest = Sha256::digest(&self.rgba);
        let mut hex = String::with_capacity(64);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }

    /// True when every pixel is fully transparent.
    pub fn is_empty(&self) -> bool {
        self.rgba.chunks_exact(4).all(|px| px[3] == 0)
    }
}

/// One keyframe of an animated layer: the drawing in force
/// from `time` until the layer's next key (hold semantics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KraFrame {
    /// Timeline position (frame index).
    pub time: i64,
    /// The drawing shown from `time` on.
    pub raster: KraRaster,
}

/// One paint layer, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KraLayer {
    /// Layer name as written (`anim/blink` etc.).
    pub name: String,
    /// The `compositeop` as written; only `normal` is
    /// accepted by this reader.
    pub blend: String,
    /// Layer opacity, 0..=255.
    pub opacity: u8,
    /// Visibility as written.
    pub visible: bool,
    /// Keyframes sorted by time; a static layer carries
    /// exactly one frame at time 0.
    pub frames: Vec<KraFrame>,
}

/// What the source file actually uses, counted at read
/// time: the importer reports this census so a refusal or
/// an acceptance is always against the file's real shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KraCensus {
    /// Layers in the stack.
    pub layer_count: usize,
    /// Layers carrying a keyframes file.
    pub keyframed_layers: usize,
    /// Total keyframes across all layers.
    pub keyframe_count: usize,
    /// Sorted unique `compositeop` values seen.
    pub blend_modes: Vec<String>,
    /// Sorted unique `nodetype` values seen.
    pub node_types: Vec<String>,
    /// The image colour space as written.
    pub color_space: String,
    /// Total tiles decoded across all payloads.
    pub tile_count: usize,
}

/// A decoded `.kra` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KraDocument {
    /// Document name (`<IMAGE name=...>`).
    pub name: String,
    /// Canvas width.
    pub width: u32,
    /// Canvas height.
    pub height: u32,
    /// Animation `(fps, range_from, range_to)` when the
    /// document carries an `<animation>` element.
    pub animation: Option<(i64, i64, i64)>,
    /// Layers, bottom first (the document lists them top
    /// first; the reader reverses).
    pub layers: Vec<KraLayer>,
    /// The read-time census.
    pub census: KraCensus,
}

/// Reads a `.kra` document from its bytes.
pub fn read_kra(bytes: &[u8]) -> KraResult<KraDocument> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        KraError::new(
            "TG-KRA-ZIP",
            format!("the container is not a readable zip: {error}"),
        )
    })?;
    let maindoc = read_entry_string(&mut archive, "maindoc.xml")?;

    // <IMAGE ...>: canvas + colour space.
    let image_tag = find_tag(&maindoc, "IMAGE")?;
    let width = tag_i64(&image_tag, "width")?;
    let height = tag_i64(&image_tag, "height")?;
    if width <= 0
        || height <= 0
        || width > i64::from(KRA_DIM_MAX)
        || height > i64::from(KRA_DIM_MAX)
    {
        return Err(KraError::new(
            "TG-KRA-DIMENSIONS",
            format!("canvas {width}x{height} is outside 1..={KRA_DIM_MAX}"),
        ));
    }
    let (width, height) = (width as u32, height as u32);
    let color_space = tag_attr(&image_tag, "colorspacename").unwrap_or_default();
    if color_space != "RGBA" {
        return Err(KraError::new(
            "TG-KRA-COLORSPACE",
            format!(
                "image colour space is {color_space:?}; this reader maps RGBA only (conversion is a colour decision, not a decode)"
            ),
        ));
    }
    let doc_name = tag_attr(&image_tag, "name").unwrap_or_default();

    // <animation>: optional in the format; the drawn-clip
    // importer refuses its absence one level up.
    let animation = match (find_tag(&maindoc, "framerate"), find_tag(&maindoc, "range")) {
        (Ok(fps_tag), Ok(range_tag)) => {
            let fps = tag_i64(&fps_tag, "value")?;
            let from = tag_i64(&range_tag, "from")?;
            let to = tag_i64(&range_tag, "to")?;
            Some((fps, from, to))
        }
        _ => None,
    };

    // Layer payloads live under "<doc>/layers/<filename>";
    // find the prefix from any entry ending in "/layers/".
    let mut entry_names: Vec<String> = Vec::new();
    for name in archive.file_names() {
        let name = name.map_err(|error| {
            KraError::new(
                "TG-KRA-ZIP",
                format!("an entry name is unreadable: {error}"),
            )
        })?;
        entry_names.push(name.into_owned());
    }
    let layers_prefix = entry_names
        .iter()
        .find_map(|name| {
            name.find("/layers/")
                .map(|at| name[..at + "/layers/".len()].to_string())
        })
        .ok_or_else(|| KraError::new("TG-KRA-ZIP", "the container carries no layers/ directory"))?;

    let mut blend_modes = BTreeSet::new();
    let mut node_types = BTreeSet::new();
    let mut census_tiles = 0_usize;
    let mut keyframed_layers = 0_usize;
    let mut keyframe_count = 0_usize;

    // Layers as written, top first.
    let mut written: Vec<KraLayer> = Vec::new();
    for tag in find_tags(&maindoc, "layer") {
        let name = tag_attr(&tag, "name").unwrap_or_default();
        let nodetype = tag_attr(&tag, "nodetype").unwrap_or_default();
        node_types.insert(nodetype.clone());
        if nodetype != "paintlayer" {
            return Err(KraError::new(
                "TG-KRA-NODETYPE",
                format!(
                    "layer {name:?} has nodetype {nodetype:?}; this reader maps paintlayer only"
                ),
            ));
        }
        let layer_space = tag_attr(&tag, "colorspacename").unwrap_or_default();
        if layer_space != "RGBA" {
            return Err(KraError::new(
                "TG-KRA-COLORSPACE",
                format!("layer {name:?} colour space is {layer_space:?}, not RGBA"),
            ));
        }
        let blend = tag_attr(&tag, "compositeop").unwrap_or_default();
        blend_modes.insert(blend.clone());
        if blend != "normal" {
            return Err(KraError::new(
                "TG-KRA-BLEND",
                format!(
                    "layer {name:?} uses compositeop {blend:?}; Krita's blend arithmetic for it is unverified against torogao-paint's, so the reader refuses rather than approximates"
                ),
            ));
        }
        let opacity_raw = tag_i64(&tag, "opacity")?;
        if !(0..=255).contains(&opacity_raw) {
            return Err(KraError::new(
                "TG-KRA-OPACITY",
                format!("layer {name:?} opacity {opacity_raw} is outside 0..=255"),
            ));
        }
        let x = tag_i64(&tag, "x")?;
        let y = tag_i64(&tag, "y")?;
        if x != 0 || y != 0 {
            return Err(KraError::new(
                "TG-KRA-OFFSET",
                format!(
                    "layer {name:?} sits at offset ({x}, {y}); offset layers are not mapped by this reader"
                ),
            ));
        }
        let visible = tag_attr(&tag, "visible").map(|v| v != "0").unwrap_or(true);
        let filename = tag_attr(&tag, "filename").ok_or_else(|| {
            KraError::new(
                "TG-KRA-XML",
                format!("layer {name:?} carries no filename attribute"),
            )
        })?;

        // Keyframes (optional): (time, frame-file) pairs.
        let mut frame_files: Vec<(i64, String)> = vec![(0, filename.clone())];
        if let Some(keyframes_attr) = tag_attr(&tag, "keyframes") {
            keyframed_layers += 1;
            let keyframes_xml =
                read_entry_string(&mut archive, &format!("{layers_prefix}{keyframes_attr}"))?;
            frame_files.clear();
            for element in find_elements(&keyframes_xml, "keyframe") {
                let ktag = element.split('>').next().unwrap_or_default();
                let time = tag_i64(ktag, "time")?;
                let frame = tag_attr(ktag, "frame").ok_or_else(|| {
                    KraError::new(
                        "TG-KRA-XML",
                        format!("layer {name:?} has a keyframe with no frame attribute"),
                    )
                })?;
                // A keyframe may carry an <offset> child;
                // only the zero offset is mapped.
                if let Some(offset_tag) = find_tags(&element, "offset").into_iter().next() {
                    let ox = tag_i64(&offset_tag, "x")?;
                    let oy = tag_i64(&offset_tag, "y")?;
                    if ox != 0 || oy != 0 {
                        return Err(KraError::new(
                            "TG-KRA-OFFSET",
                            format!(
                                "layer {name:?} keyframe at time {time} carries offset ({ox}, {oy}); not mapped"
                            ),
                        ));
                    }
                }
                frame_files.push((time, frame));
            }
            if frame_files.is_empty() {
                return Err(KraError::new(
                    "TG-KRA-XML",
                    format!("layer {name:?} keyframes file lists no keyframes"),
                ));
            }
            frame_files.sort_by_key(|(time, _)| *time);
            keyframe_count += frame_files.len();
        }

        let mut frames = Vec::with_capacity(frame_files.len());
        for (time, frame_file) in frame_files {
            let payload = read_entry_bytes(&mut archive, &format!("{layers_prefix}{frame_file}"))
                .map_err(|error| {
                KraError::new(
                    error.code,
                    format!(
                        "layer {name:?} keyframe payload {frame_file:?}: {}",
                        error.message
                    ),
                )
            })?;
            let (raster, tiles) = decode_tile_stream(&payload, width, height)?;
            census_tiles += tiles;
            frames.push(KraFrame { time, raster });
        }

        written.push(KraLayer {
            name,
            blend,
            opacity: opacity_raw as u8,
            visible,
            frames,
        });
    }
    if written.is_empty() {
        return Err(KraError::new("TG-KRA-XML", "maindoc.xml lists no layers"));
    }
    written.reverse();
    let layer_count = written.len();

    Ok(KraDocument {
        name: doc_name,
        width,
        height,
        animation,
        layers: written,
        census: KraCensus {
            layer_count,
            keyframed_layers,
            keyframe_count,
            blend_modes: blend_modes.into_iter().collect(),
            node_types: node_types.into_iter().collect(),
            color_space,
            tile_count: census_tiles,
        },
    })
}

/// Decodes one layer payload (the tile stream) into a
/// canvas raster. Returns the raster and the tile count.
fn decode_tile_stream(blob: &[u8], width: u32, height: u32) -> KraResult<(KraRaster, usize)> {
    let mut lines = LineReader::new(blob);
    let version = lines.next_line("TG-KRA-TILEHEADER")?;
    if version != "VERSION 2" {
        return Err(KraError::new(
            "TG-KRA-TILEHEADER",
            format!("tile stream opens with {version:?}; this reader maps VERSION 2 only"),
        ));
    }
    for (key, expected) in [
        ("TILEWIDTH", TILE_PX),
        ("TILEHEIGHT", TILE_PX),
        ("PIXELSIZE", PIXEL_SIZE),
    ] {
        let line = lines.next_line("TG-KRA-TILEHEADER")?;
        let mut parts = line.splitn(2, ' ');
        let got_key = parts.next().unwrap_or_default();
        let got_val: usize = parts
            .next()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(usize::MAX);
        if got_key != key || got_val != expected {
            return Err(KraError::new(
                "TG-KRA-TILEHEADER",
                format!("tile stream header line {line:?}; this reader maps {key} {expected} only"),
            ));
        }
    }
    let data_line = lines.next_line("TG-KRA-TILEHEADER")?;
    let tile_count: usize = data_line
        .strip_prefix("DATA ")
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(|| {
            KraError::new(
                "TG-KRA-TILEHEADER",
                format!("tile stream DATA line is {data_line:?}"),
            )
        })?;

    let mut rgba = vec![0_u8; (width as usize) * (height as usize) * 4];
    for _ in 0..tile_count {
        let header = lines.next_line("TG-KRA-TILEHEADER")?;
        let mut fields = header.split(',');
        let tx: i64 = fields
            .next()
            .and_then(|text| text.trim().parse().ok())
            .ok_or_else(|| tile_header_err(&header))?;
        let ty: i64 = fields
            .next()
            .and_then(|text| text.trim().parse().ok())
            .ok_or_else(|| tile_header_err(&header))?;
        let encoding = fields.next().map(str::trim).unwrap_or_default();
        if encoding != "LZF" {
            return Err(KraError::new(
                "TG-KRA-TILEHEADER",
                format!("tile ({tx}, {ty}) uses encoding {encoding:?}; LZF only"),
            ));
        }
        let record_len: usize = fields
            .next()
            .and_then(|text| text.trim().parse().ok())
            .ok_or_else(|| tile_header_err(&header))?;
        let record = lines.take_bytes(record_len)?;
        if record.first() != Some(&0x01) {
            return Err(KraError::new(
                "TG-KRA-TILE-COMPRESSION",
                format!(
                    "tile ({tx}, {ty}) record opens with marker {:?}; only 0x01 (liblzf) is mapped",
                    record.first()
                ),
            ));
        }
        let raw = lzf_decompress(&record[1..], TILE_BYTES)?;
        blit_tile(&mut rgba, width, height, tx, ty, &raw);
    }
    Ok((
        KraRaster {
            width,
            height,
            rgba,
        },
        tile_count,
    ))
}

fn tile_header_err(header: &str) -> KraError {
    KraError::new(
        "TG-KRA-TILEHEADER",
        format!("tile record header is {header:?}"),
    )
}

/// Places one decompressed tile (four channel planes in
/// B, G, R, A order, each `TILE_PX * TILE_PX` bytes) into
/// the canvas raster as interleaved RGBA, clipped to the
/// canvas.
fn blit_tile(rgba: &mut [u8], width: u32, height: u32, tx: i64, ty: i64, raw: &[u8]) {
    let plane = TILE_PX * TILE_PX;
    for ly in 0..TILE_PX as i64 {
        let gy = ty + ly;
        if gy < 0 || gy >= i64::from(height) {
            continue;
        }
        for lx in 0..TILE_PX as i64 {
            let gx = tx + lx;
            if gx < 0 || gx >= i64::from(width) {
                continue;
            }
            let at = (ly as usize) * TILE_PX + lx as usize;
            let out = ((gy as usize) * (width as usize) + gx as usize) * 4;
            rgba[out] = raw[2 * plane + at]; // R plane
            rgba[out + 1] = raw[plane + at]; // G plane
            rgba[out + 2] = raw[at]; // B plane
            rgba[out + 3] = raw[3 * plane + at]; // A plane
        }
    }
}

/// liblzf decompression, bounded to an exact expected
/// output length: a stream that under-runs, over-runs, or
/// leaves input unconsumed is a typed refusal, because the
/// tile law is exact.
fn lzf_decompress(input: &[u8], expected: usize) -> KraResult<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(expected);
    let mut i = 0_usize;
    while i < input.len() {
        let ctrl = input[i];
        i += 1;
        if ctrl < 32 {
            let len = usize::from(ctrl) + 1;
            if i + len > input.len() || out.len() + len > expected {
                return Err(lzf_err("literal run overruns the stream"));
            }
            out.extend_from_slice(&input[i..i + len]);
            i += len;
        } else {
            let mut len = usize::from(ctrl >> 5);
            let mut offset = usize::from(ctrl & 0x1f) << 8;
            if len == 7 {
                if i >= input.len() {
                    return Err(lzf_err("extended length byte missing"));
                }
                len += usize::from(input[i]);
                i += 1;
            }
            if i >= input.len() {
                return Err(lzf_err("offset byte missing"));
            }
            offset += usize::from(input[i]) + 1;
            i += 1;
            len += 2;
            if offset == 0 || offset > out.len() || out.len() + len > expected {
                return Err(lzf_err("back-reference outside the output"));
            }
            let start = out.len() - offset;
            for k in 0..len {
                let byte = out[start + k];
                out.push(byte);
            }
        }
    }
    if out.len() != expected {
        return Err(lzf_err("stream produced the wrong byte count"));
    }
    Ok(out)
}

fn lzf_err(message: &str) -> KraError {
    KraError::new("TG-KRA-LZF", message.to_string())
}

/// A cursor over a tile-stream blob that alternates text
/// header lines and binary records.
struct LineReader<'a> {
    blob: &'a [u8],
    pos: usize,
}

impl<'a> LineReader<'a> {
    fn new(blob: &'a [u8]) -> LineReader<'a> {
        LineReader { blob, pos: 0 }
    }

    fn next_line(&mut self, code: &'static str) -> KraResult<String> {
        let start = self.pos;
        while self.pos < self.blob.len() && self.blob[self.pos] != b'\n' {
            self.pos += 1;
        }
        if self.pos >= self.blob.len() {
            return Err(KraError::new(code, "tile stream ends inside a header line"));
        }
        let text = String::from_utf8_lossy(&self.blob[start..self.pos]).into_owned();
        self.pos += 1; // the newline
        Ok(text.trim_end_matches('\r').to_string())
    }

    fn take_bytes(&mut self, count: usize) -> KraResult<&'a [u8]> {
        if self.pos + count > self.blob.len() {
            return Err(KraError::new(
                "TG-KRA-LZF",
                "tile record overruns the payload",
            ));
        }
        let slice = &self.blob[self.pos..self.pos + count];
        self.pos += count;
        Ok(slice)
    }
}

fn read_entry_bytes(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> KraResult<Vec<u8>> {
    let mut entry = archive.by_name(name).map_err(|error| {
        KraError::new(
            "TG-KRA-ZIP",
            format!("entry {name:?} is not readable: {error}"),
        )
    })?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes).map_err(|error| {
        KraError::new(
            "TG-KRA-ZIP",
            format!("entry {name:?} failed to decompress: {error}"),
        )
    })?;
    Ok(bytes)
}

fn read_entry_string(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> KraResult<String> {
    let bytes = read_entry_bytes(archive, name)?;
    String::from_utf8(bytes).map_err(|error| {
        KraError::new(
            "TG-KRA-XML",
            format!("entry {name:?} is not UTF-8: {error}"),
        )
    })
}

/// Every `<name ...>` tag's attribute text in `xml`, in
/// document order. Tag text runs to the first `>`; Krita's
/// writer never puts `>` inside an attribute value.
fn find_tags(xml: &str, name: &str) -> Vec<String> {
    let needle = format!("<{name} ");
    let mut tags = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at + needle.len()..];
        match after.find('>') {
            Some(end) => {
                tags.push(after[..end].to_string());
                rest = &after[end..];
            }
            None => break,
        }
    }
    tags
}

fn find_tag(xml: &str, name: &str) -> KraResult<String> {
    find_tags(xml, name)
        .into_iter()
        .next()
        .ok_or_else(|| KraError::new("TG-KRA-XML", format!("no <{name}> element in the document")))
}

/// Every `<name ...>...</name>` element's full text
/// (opening tag plus content), for elements whose children
/// matter — keyframes carry an `<offset>` child. A
/// self-closing element returns just its tag text.
fn find_elements(xml: &str, name: &str) -> Vec<String> {
    let needle = format!("<{name} ");
    let closing = format!("</{name}>");
    let mut elements = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at..];
        match after.find(&closing) {
            Some(end) => {
                elements.push(after[..end + closing.len()].to_string());
                rest = &after[end + closing.len()..];
            }
            None => {
                // Self-closing or unterminated: take the tag.
                match after.find('>') {
                    Some(end) => {
                        elements.push(after[..=end].to_string());
                        rest = &after[end..];
                    }
                    None => break,
                }
            }
        }
    }
    elements
}

/// One attribute's value from tag text, with the five XML
/// entities decoded. The match requires a space before the
/// attribute name, so `name` never matches the tail of
/// `colorspacename`.
fn tag_attr(tag: &str, attr: &str) -> Option<String> {
    let padded = format!(" {tag}");
    let needle = format!(" {attr}=\"");
    let at = padded.find(&needle)? + needle.len();
    let rest = &padded[at..];
    let end = rest.find('"')?;
    Some(
        rest[..end]
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

fn tag_i64(tag: &str, attr: &str) -> KraResult<i64> {
    tag_attr(tag, attr)
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(|| {
            KraError::new(
                "TG-KRA-XML",
                format!("attribute {attr} is missing or not an integer in tag {tag:?}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A literal-only LZF stream encoding `data` (runs of
    /// at most 32 literals), prefixed like a tile record.
    fn literal_stream(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x01_u8];
        for chunk in data.chunks(32) {
            out.push((chunk.len() - 1) as u8);
            out.extend_from_slice(chunk);
        }
        out
    }

    fn tile_stream(tiles: &[((i64, i64), Vec<u8>)]) -> Vec<u8> {
        let mut blob = format!(
            "VERSION 2\nTILEWIDTH 64\nTILEHEIGHT 64\nPIXELSIZE 4\nDATA {}\n",
            tiles.len()
        )
        .into_bytes();
        for ((x, y), record) in tiles {
            blob.extend_from_slice(format!("{x},{y},LZF,{}\n", record.len()).as_bytes());
            blob.extend_from_slice(record);
        }
        blob
    }

    fn solid_tile(r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        let mut raw = vec![0_u8; TILE_BYTES];
        for i in 0..TILE_PX * TILE_PX {
            raw[i] = b;
            raw[TILE_PX * TILE_PX + i] = g;
            raw[2 * TILE_PX * TILE_PX + i] = r;
            raw[3 * TILE_PX * TILE_PX + i] = a;
        }
        literal_stream(&raw)
    }

    fn build_kra(layer_tags: &str, payloads: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let maindoc = format!(
            "<?xml version=\"1.0\"?><DOC><IMAGE colorspacename=\"RGBA\" height=\"64\" name=\"t\" width=\"64\">\
             <layers>{layer_tags}</layers>\
             <animation><framerate type=\"value\" value=\"24\"/><range from=\"0\" to=\"3\" type=\"timerange\"/></animation>\
             </IMAGE></DOC>"
        );
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file("maindoc.xml", options).unwrap();
        writer.write_all(maindoc.as_bytes()).unwrap();
        for (name, bytes) in payloads {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_a_minimal_document() {
        let layer = "<layer compositeop=\"normal\" colorspacename=\"RGBA\" filename=\"layer2\" name=\"base\" nodetype=\"paintlayer\" opacity=\"255\" visible=\"1\" x=\"0\" y=\"0\"/>";
        let kra = build_kra(
            layer,
            &[(
                "t/layers/layer2",
                tile_stream(&[((0, 0), solid_tile(10, 20, 30, 255))]),
            )],
        );
        let doc = read_kra(&kra).expect("minimal kra reads");
        assert_eq!((doc.width, doc.height), (64, 64));
        assert_eq!(doc.animation, Some((24, 0, 3)));
        assert_eq!(doc.layers.len(), 1);
        let px = &doc.layers[0].frames[0].raster.rgba;
        assert_eq!(&px[0..4], &[10, 20, 30, 255]);
        assert_eq!(doc.census.tile_count, 1);
    }

    #[test]
    fn refuses_an_unmapped_blend_by_name() {
        let layer = "<layer compositeop=\"multiply\" colorspacename=\"RGBA\" filename=\"layer2\" name=\"base\" nodetype=\"paintlayer\" opacity=\"255\" visible=\"1\" x=\"0\" y=\"0\"/>";
        let kra = build_kra(layer, &[("t/layers/layer2", tile_stream(&[]))]);
        let error = read_kra(&kra).expect_err("multiply refuses");
        assert_eq!(error.code(), "TG-KRA-BLEND");
        assert!(error.message().contains("multiply"));
    }

    #[test]
    fn refuses_a_group_layer_by_name() {
        let layer = "<layer compositeop=\"normal\" colorspacename=\"RGBA\" filename=\"layer2\" name=\"g\" nodetype=\"grouplayer\" opacity=\"255\" visible=\"1\" x=\"0\" y=\"0\"/>";
        let kra = build_kra(layer, &[("t/layers/layer2", tile_stream(&[]))]);
        let error = read_kra(&kra).expect_err("group refuses");
        assert_eq!(error.code(), "TG-KRA-NODETYPE");
    }

    #[test]
    fn refuses_an_unknown_tile_version() {
        let mut blob = b"VERSION 3\n".to_vec();
        blob.extend_from_slice(&tile_stream(&[])[10..]);
        let layer = "<layer compositeop=\"normal\" colorspacename=\"RGBA\" filename=\"layer2\" name=\"base\" nodetype=\"paintlayer\" opacity=\"255\" visible=\"1\" x=\"0\" y=\"0\"/>";
        let kra = build_kra(layer, &[("t/layers/layer2", blob)]);
        let error = read_kra(&kra).expect_err("version refuses");
        assert_eq!(error.code(), "TG-KRA-TILEHEADER");
    }

    #[test]
    fn lzf_rejects_a_short_stream() {
        let error = lzf_decompress(&[0x00, b'a'], 16).expect_err("short stream refuses");
        assert_eq!(error.code(), "TG-KRA-LZF");
    }
}
