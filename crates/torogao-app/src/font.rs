//! The merged font stack (carried spec test C1 of the
//! parity inventory).
//!
//! The reference editor builds one merged font atlas from
//! several fonts: base fonts first, then user drop-ins
//! from the data fonts directory in sorted order, then
//! icon fonts last. In a merged stack, when two fonts
//! provide the same codepoint, **the glyph from the font
//! added first wins** — the later font never replaces it.
//! That rule is a spec of this module, carried by tests,
//! not a behaviour to rediscover.
//!
//! This module owns the stack and the rule (the cmap
//! reader included); text shaping/rendering consumes
//! `glyph_for` when the presentation layer loads real
//! font files through it.

use std::collections::BTreeMap;

use crate::error::{AppError, AppResult};

/// Largest font file the cmap reader will parse.
pub const FONT_BYTES_MAX: usize = 64 * 1024 * 1024;
/// Largest segment/group count in one cmap subtable.
const CMAP_SEGMENTS_MAX: usize = 1 << 17;

/// One parsed font face: its cmap as a codepoint → glyph
/// id map.
#[derive(Debug, Clone)]
pub struct FontFace {
    /// Display name (file stem or caller-given label).
    pub name: String,
    /// Codepoint → glyph id, from the font's cmap
    /// (format 12 preferred over format 4 when both
    /// exist, as the reference stack reads the fullest
    /// subtable).
    pub glyphs: BTreeMap<u32, u32>,
}

impl FontFace {
    /// Parses a TrueType/OpenType font's cmap from raw
    /// bytes. Bounds and offsets are checked before every
    /// read; a truncated or malformed font is a typed
    /// refusal, never a panic.
    pub fn from_bytes(name: &str, bytes: &[u8]) -> AppResult<FontFace> {
        if bytes.len() > FONT_BYTES_MAX {
            return Err(AppError::Format(format!(
                "font {name} is {} bytes; the bound is {FONT_BYTES_MAX}",
                bytes.len()
            )));
        }
        let cmap_offset = find_table(bytes, b"cmap")?;
        let glyphs = parse_cmap(bytes, cmap_offset)?;
        Ok(FontFace {
            name: name.to_string(),
            glyphs,
        })
    }

    /// Whether this face provides `codepoint`.
    pub fn covers(&self, codepoint: u32) -> bool {
        self.glyphs.contains_key(&codepoint)
    }
}

/// A merged font stack: faces in add order. A codepoint
/// resolves to the first face that provides it.
#[derive(Debug, Default)]
pub struct FontStack {
    faces: Vec<FontFace>,
}

impl FontStack {
    /// An empty stack.
    pub fn new() -> FontStack {
        FontStack { faces: Vec::new() }
    }

    /// Builds the stack in the reference add order: the
    /// base faces in the order given, then the user
    /// drop-ins sorted by name (deterministic — never
    /// filesystem enumeration order), then the icon faces
    /// in the order given.
    pub fn in_reference_order(
        base: Vec<FontFace>,
        mut drop_ins: Vec<FontFace>,
        icons: Vec<FontFace>,
    ) -> FontStack {
        drop_ins.sort_by(|a, b| a.name.cmp(&b.name));
        let mut stack = FontStack::new();
        for face in base.into_iter().chain(drop_ins).chain(icons) {
            stack.add(face);
        }
        stack
    }

    /// Adds a face at the end of the stack (lowest
    /// priority for shared codepoints).
    pub fn add(&mut self, face: FontFace) {
        self.faces.push(face);
    }

    /// The faces in add order.
    pub fn faces(&self) -> &[FontFace] {
        &self.faces
    }

    /// Resolves a codepoint: the glyph id from the FIRST
    /// face that provides it (the C1 rule), with the
    /// winning face's index.
    pub fn glyph_for(&self, codepoint: u32) -> Option<(usize, u32)> {
        self.faces
            .iter()
            .enumerate()
            .find_map(|(index, face)| face.glyphs.get(&codepoint).map(|glyph| (index, *glyph)))
    }
}

/// Big-endian u16 at `offset`, checked.
fn be_u16(bytes: &[u8], offset: usize) -> AppResult<u16> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| AppError::Format("font read past the end of the file".to_string()))?;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

/// Big-endian u32 at `offset`, checked.
fn be_u32(bytes: &[u8], offset: usize) -> AppResult<u32> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| AppError::Format("font read past the end of the file".to_string()))?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Locates a table by tag in the offset table; returns
/// its file offset. Collections (.ttc) are refused by
/// name — the stack consumes single fonts.
fn find_table(bytes: &[u8], tag: &[u8; 4]) -> AppResult<usize> {
    if bytes.len() < 12 {
        return Err(AppError::Format("font file is truncated".to_string()));
    }
    let scaler = be_u32(bytes, 0)?;
    if scaler == 0x7474_6366 {
        return Err(AppError::Format(
            "font collections (.ttc) are not single fonts; extract a face first".to_string(),
        ));
    }
    let table_count = usize::from(be_u16(bytes, 4)?);
    for index in 0..table_count {
        let record = 12 + index * 16;
        let found = bytes
            .get(record..record + 4)
            .ok_or_else(|| AppError::Format("font table directory is truncated".to_string()))?;
        if found == tag {
            return Ok(be_u32(bytes, record + 8)? as usize);
        }
    }
    Err(AppError::Format(format!(
        "font carries no {} table",
        String::from_utf8_lossy(tag)
    )))
}

/// Parses the cmap table: chooses a format 12 subtable
/// when present, else format 4, and expands it to the
/// codepoint → glyph map.
fn parse_cmap(bytes: &[u8], cmap_offset: usize) -> AppResult<BTreeMap<u32, u32>> {
    let subtable_count = usize::from(be_u16(bytes, cmap_offset + 2)?);
    let mut format4: Option<usize> = None;
    let mut format12: Option<usize> = None;
    for index in 0..subtable_count {
        let record = cmap_offset + 4 + index * 8;
        let offset = cmap_offset + be_u32(bytes, record + 4)? as usize;
        let format = be_u16(bytes, offset)?;
        match format {
            12 => format12 = Some(offset),
            4 if format4.is_none() => format4 = Some(offset),
            _ => {}
        }
    }
    if let Some(offset) = format12 {
        return parse_cmap12(bytes, offset);
    }
    if let Some(offset) = format4 {
        return parse_cmap4(bytes, offset);
    }
    Err(AppError::Format(
        "font cmap has no format 4 or 12 subtable".to_string(),
    ))
}

/// Format 12: sequential groups of (start, end, glyph).
fn parse_cmap12(bytes: &[u8], offset: usize) -> AppResult<BTreeMap<u32, u32>> {
    let groups = be_u32(bytes, offset + 12)? as usize;
    if groups > CMAP_SEGMENTS_MAX {
        return Err(AppError::Format(format!(
            "cmap12 group count {groups} exceeds the bound {CMAP_SEGMENTS_MAX}"
        )));
    }
    let mut glyphs = BTreeMap::new();
    for group in 0..groups {
        let record = offset + 16 + group * 12;
        let start = be_u32(bytes, record)?;
        let end = be_u32(bytes, record + 4)?;
        let first_glyph = be_u32(bytes, record + 8)?;
        if end < start || end - start > 0x10_FFFF {
            return Err(AppError::Format(
                "cmap12 group range is invalid".to_string(),
            ));
        }
        for (delta, codepoint) in (start..=end).enumerate() {
            glyphs.insert(codepoint, first_glyph + delta as u32);
        }
    }
    Ok(glyphs)
}

/// Format 4: segment-mapped deltas with the standard
/// idDelta / idRangeOffset addressing.
fn parse_cmap4(bytes: &[u8], offset: usize) -> AppResult<BTreeMap<u32, u32>> {
    let seg_count = usize::from(be_u16(bytes, offset + 6)?) / 2;
    if seg_count == 0 || seg_count > CMAP_SEGMENTS_MAX {
        return Err(AppError::Format(format!(
            "cmap4 segment count {seg_count} is outside the bound"
        )));
    }
    let end_base = offset + 14;
    let start_base = end_base + 2 * seg_count + 2;
    let delta_base = start_base + 2 * seg_count;
    let range_base = delta_base + 2 * seg_count;
    let mut glyphs = BTreeMap::new();
    for seg in 0..seg_count {
        let end = u32::from(be_u16(bytes, end_base + 2 * seg)?);
        let start = u32::from(be_u16(bytes, start_base + 2 * seg)?);
        let delta = i32::from(be_u16(bytes, delta_base + 2 * seg)? as i16);
        let range_offset = be_u16(bytes, range_base + 2 * seg)?;
        if end < start {
            return Err(AppError::Format(
                "cmap4 segment range is invalid".to_string(),
            ));
        }
        for codepoint in start..=end {
            if codepoint == 0xFFFF {
                continue;
            }
            let glyph = if range_offset == 0 {
                (codepoint as i32 + delta).rem_euclid(65536) as u32
            } else {
                let glyph_index_offset = range_base
                    + 2 * seg
                    + usize::from(range_offset)
                    + 2 * (codepoint - start) as usize;
                let glyph = be_u16(bytes, glyph_index_offset)?;
                if glyph == 0 {
                    0
                } else {
                    (i32::from(glyph) + delta).rem_euclid(65536) as u32
                }
            };
            if glyph != 0 {
                glyphs.insert(codepoint, glyph);
            }
        }
    }
    Ok(glyphs)
}

#[cfg(test)]
mod tests {
    use super::{FontFace, FontStack};

    /// Builds a minimal synthetic TTF carrying one cmap
    /// format 4 subtable with two segments: 'A' (U+0041)
    /// → `glyph_a`, 'B' (U+0042) → `glyph_b`, via idDelta
    /// addressing. A fixture for the stack rule, shaped
    /// like the real thing the parser reads.
    fn push16(bytes: &mut Vec<u8>, v: u16) {
        bytes.extend_from_slice(&v.to_be_bytes());
    }

    fn push32(bytes: &mut Vec<u8>, v: u32) {
        bytes.extend_from_slice(&v.to_be_bytes());
    }

    fn synthetic_font(glyph_a: u16, glyph_b: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        // Offset table: one table (cmap at offset 28).
        push32(&mut bytes, 0x0001_0000);
        push16(&mut bytes, 1);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        // Table record: tag, checksum, offset, length.
        bytes.extend_from_slice(b"cmap");
        push32(&mut bytes, 0);
        push32(&mut bytes, 28);
        push32(&mut bytes, 52);
        // cmap header: version, one subtable (platform 3,
        // encoding 1, offset 12).
        push16(&mut bytes, 0);
        push16(&mut bytes, 1);
        push16(&mut bytes, 3);
        push16(&mut bytes, 1);
        push32(&mut bytes, 12);
        // Format 4 subtable at 28+12=40: two segments
        // ('A'..'A', 'B'..'B') plus the 0xFFFF terminator
        // segment = 3 segments.
        let seg_count: u16 = 3;
        push16(&mut bytes, 4); // format
        push16(&mut bytes, 16 + 8 * seg_count); // length
        push16(&mut bytes, 0); // language
        push16(&mut bytes, seg_count * 2); // segCountX2
        push16(&mut bytes, 4);
        push16(&mut bytes, 1);
        push16(&mut bytes, 2);
        // endCodes
        push16(&mut bytes, 0x41);
        push16(&mut bytes, 0x42);
        push16(&mut bytes, 0xFFFF);
        push16(&mut bytes, 0); // reservedPad
        // startCodes
        push16(&mut bytes, 0x41);
        push16(&mut bytes, 0x42);
        push16(&mut bytes, 0xFFFF);
        // idDeltas: glyph = codepoint + delta (mod 65536)
        push16(&mut bytes, (glyph_a as i32 - 0x41) as u16);
        push16(&mut bytes, (glyph_b as i32 - 0x42) as u16);
        push16(&mut bytes, 1); // terminator delta (0xFFFF + 1 -> 0)
        // idRangeOffsets: all zero (delta addressing)
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        bytes
    }

    fn face(name: &str, glyph_a: u16, glyph_b: u16) -> FontFace {
        FontFace::from_bytes(name, &synthetic_font(glyph_a, glyph_b))
            .expect("synthetic font parses")
    }

    /// THE C1 SPEC TEST: two fonts share a codepoint with
    /// distinguishable glyphs; the first-added font's
    /// glyph wins, in both add orders.
    #[test]
    fn first_added_glyph_wins_a_shared_codepoint() {
        let alpha = face("alpha", 10, 11);
        let beta = face("beta", 20, 21);
        assert_eq!(alpha.glyphs[&0x41], 10);
        assert_eq!(beta.glyphs[&0x41], 20);

        let mut stack = FontStack::new();
        stack.add(alpha.clone());
        stack.add(beta.clone());
        assert_eq!(stack.glyph_for(0x41), Some((0, 10)));
        assert_eq!(stack.glyph_for(0x42), Some((0, 11)));

        let mut reversed = FontStack::new();
        reversed.add(beta);
        reversed.add(alpha);
        assert_eq!(reversed.glyph_for(0x41), Some((0, 20)));
        assert_eq!(reversed.glyph_for(0x42), Some((0, 21)));
    }

    #[test]
    fn add_order_is_deterministic_sorted_drop_ins() {
        let base = face("base", 1, 2);
        let zeta = face("zeta", 3, 4);
        let alpha = face("alpha-drop", 5, 6);
        let icon = face("icons", 7, 8);
        let stack = FontStack::in_reference_order(vec![base], vec![zeta, alpha], vec![icon]);
        let names: Vec<&str> = stack.faces().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["base", "alpha-drop", "zeta", "icons"]);
    }

    #[test]
    fn truncated_fonts_refuse_typed() {
        assert!(FontFace::from_bytes("bad", &[0u8; 4]).is_err());
        assert!(FontFace::from_bytes("bad", &synthetic_font(1, 2)[..20]).is_err());
        let mut ttc = synthetic_font(1, 2);
        ttc[0..4].copy_from_slice(&0x7474_6366_u32.to_be_bytes());
        assert!(FontFace::from_bytes("collection", &ttc).is_err());
    }
}
