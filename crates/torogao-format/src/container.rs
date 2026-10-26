//! The INP container: magic, payload JSON, embedded texture
//! slots, and vendor EXT sections.
//!
//! Reading mirrors the reference loader's strictness: a
//! structurally broken container is one typed refusal
//! (`TG-CONTAINER-MALFORMED` with a path-free structural
//! detail), never a partial read. Writing an authored project
//! is byte surgery — the prefix (magic, payload, texture
//! section) is copied verbatim and only the EXT region is
//! rebuilt — so a round trip through an authoring tool
//! preserves every byte it did not mean to touch.

use serde_json::Value;

use crate::error::{FormatResult, TorogaoError};
use crate::jsonfmt::to_d_json;

/// The container magic: the ASCII bytes `TRNSRTS` plus NUL.
pub const CONTAINER_MAGIC: &[u8; 8] = b"TRNSRTS\0";
/// The texture-section marker.
pub const TEXTURE_MAGIC: &[u8; 8] = b"TEX_SECT";
/// The extension-section marker.
pub const EXT_MAGIC: &[u8; 8] = b"EXT_SECT";
/// The Torogao vendor section name in the EXT region.
pub const TOROGAO_EXT_SECTION: &str = "io.ntranch.torogao";
/// Stable code for a container the loader rejects.
pub const TG_CONTAINER_MALFORMED: &str = "TG-CONTAINER-MALFORMED";

/// One embedded texture slot: a format tag byte plus data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureSlot {
    /// Raw tag byte (0 = png, 1 = tga, 2 = bc7).
    pub tag: u8,
    /// The texture bytes.
    pub data: Vec<u8>,
}

impl TextureSlot {
    /// The format name the census reports for this tag.
    pub fn format_name(&self) -> &'static str {
        match self.tag {
            0 => "png",
            1 => "tga",
            2 => "bc7",
            _ => "unknown",
        }
    }
}

/// One vendor EXT section: a name plus raw JSON bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtSection {
    /// The vendor section name.
    pub name: String,
    /// The section's raw bytes (a JSON document, by contract).
    pub data: Vec<u8>,
}

/// A fully read INP container.
#[derive(Debug, Clone)]
pub struct InpFile {
    /// The parsed payload document (null when the payload is
    /// not a JSON object — the payload's own shape is the
    /// payload layer's problem, not the container's).
    pub payload: Value,
    /// The payload's raw bytes, verbatim.
    pub payload_bytes: Vec<u8>,
    /// Everything from the start of the file through the end
    /// of the texture section, verbatim: the byte-surgery
    /// prefix.
    pub prefix: Vec<u8>,
    /// The embedded texture slots, in file order.
    pub textures: Vec<TextureSlot>,
    /// The EXT sections, in file order (duplicates kept: the
    /// scan layer, not the reader, judges them).
    pub ext_sections: Vec<ExtSection>,
    /// Whether the post-texture region parsed as a well-formed
    /// EXT region (the container scan's `ok`).
    pub scan_ok: bool,
    /// Bytes after the final EXT entry, when the EXT region
    /// parsed; zero otherwise.
    pub trailing_bytes: usize,
    /// How many EXT sections carry the Torogao vendor name.
    pub torogao_ext_count: usize,
}

impl InpFile {
    /// The Torogao extension document: the bytes of the
    /// (single, by contract) Torogao EXT section parsed as
    /// JSON. Returns `(document, found, malformed)`: document
    /// is null when absent or unparseable; `found` says a
    /// section exists at all; `malformed` says it exists but
    /// does not parse.
    pub fn torogao_ext(&self) -> (Value, bool, bool) {
        for section in &self.ext_sections {
            if section.name == TOROGAO_EXT_SECTION {
                if section.data.is_empty() {
                    return (Value::Null, false, false);
                }
                return match serde_json::from_slice::<Value>(&section.data) {
                    Ok(doc) => (doc, true, false),
                    Err(_) => (Value::Null, true, true),
                };
            }
        }
        (Value::Null, false, false)
    }
}

fn malformed(detail: &str) -> TorogaoError {
    TorogaoError::new(
        TG_CONTAINER_MALFORMED,
        format!("project could not be loaded: malformed INP container ({detail})"),
    )
}

fn get_be(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn put_be(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Reads and structurally validates a whole INP container.
///
/// Failure is the one typed `TG-CONTAINER-MALFORMED` error;
/// the detail names the structural defect (bad magic,
/// truncation, overrun) and nothing else — no paths, no
/// internal detail.
pub fn read_inp(bytes: &[u8]) -> FormatResult<InpFile> {
    if bytes.len() < 12 || &bytes[0..8] != CONTAINER_MAGIC {
        return Err(malformed("bad magic"));
    }
    let payload_len = get_be(bytes, 8) as usize;
    let payload_end = 12 + payload_len;
    if payload_end > bytes.len() {
        return Err(malformed("payload overruns the file"));
    }
    let payload_bytes = bytes[12..payload_end].to_vec();
    let payload: Value = match serde_json::from_slice(&payload_bytes) {
        Ok(value) => value,
        Err(_) => return Err(malformed("payload is not parseable JSON")),
    };
    let mut at = payload_end;
    if at + 8 > bytes.len() || &bytes[at..at + 8] != TEXTURE_MAGIC {
        return Err(malformed("texture section not found where expected"));
    }
    at += 8;
    if at + 4 > bytes.len() {
        return Err(malformed("texture section is truncated"));
    }
    let slot_count = get_be(bytes, at) as usize;
    at += 4;
    // Each slot costs at least its 5 header bytes; a count
    // beyond that bound is an overrun before any allocation.
    if slot_count > bytes.len() / 5 + 1 {
        return Err(malformed("texture section overruns the file"));
    }
    let mut textures = Vec::with_capacity(slot_count);
    for _ in 0..slot_count {
        if at + 5 > bytes.len() {
            return Err(malformed("texture slot is truncated"));
        }
        let len = get_be(bytes, at) as usize;
        let tag = bytes[at + 4];
        at += 5;
        if at + len > bytes.len() {
            return Err(malformed("texture section overruns the file"));
        }
        textures.push(TextureSlot {
            tag,
            data: bytes[at..at + len].to_vec(),
        });
        at += len;
    }
    let prefix = bytes[0..at].to_vec();
    let mut ext_sections: Vec<ExtSection> = Vec::new();
    let mut scan_ok = true;
    let mut trailing_bytes = 0usize;
    if at < bytes.len() {
        if at + 8 > bytes.len() || &bytes[at..at + 8] != EXT_MAGIC {
            scan_ok = false;
        } else {
            at += 8;
            if at + 4 > bytes.len() {
                scan_ok = false;
            } else {
                let count = get_be(bytes, at) as usize;
                at += 4;
                for _ in 0..count {
                    if at + 4 > bytes.len() {
                        scan_ok = false;
                        break;
                    }
                    let name_len = get_be(bytes, at) as usize;
                    at += 4;
                    if at + name_len + 4 > bytes.len() {
                        scan_ok = false;
                        break;
                    }
                    let name = String::from_utf8_lossy(&bytes[at..at + name_len]).into_owned();
                    at += name_len;
                    let data_len = get_be(bytes, at) as usize;
                    at += 4;
                    if at + data_len > bytes.len() {
                        scan_ok = false;
                        break;
                    }
                    let data = bytes[at..at + data_len].to_vec();
                    at += data_len;
                    ext_sections.push(ExtSection { name, data });
                }
                if scan_ok && at < bytes.len() {
                    trailing_bytes = bytes.len() - at;
                }
            }
        }
    }
    let torogao_ext_count = ext_sections
        .iter()
        .filter(|s| s.name == TOROGAO_EXT_SECTION)
        .count();
    Ok(InpFile {
        payload,
        payload_bytes,
        prefix,
        textures,
        ext_sections,
        scan_ok,
        trailing_bytes,
        torogao_ext_count,
    })
}

/// The container-structure facts only the raw bytes can show
/// (the loaded puppet's section map collapses duplicates and
/// hides trailing bytes). A scan failure yields `ok: false`
/// and no findings — the load boundary owns that failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerScan {
    /// The EXT region parsed structurally.
    pub ok: bool,
    /// Every EXT section, in file order.
    pub sections: Vec<ExtSection>,
    /// How many sections carry the Torogao vendor name.
    pub torogao_sections: usize,
    /// Bytes after the final EXT entry.
    pub trailing_bytes: usize,
}

/// Scans a container's EXT structure without loading it.
pub fn scan_container(bytes: &[u8]) -> ContainerScan {
    match read_inp(bytes) {
        Ok(file) => ContainerScan {
            ok: file.scan_ok,
            sections: file.ext_sections,
            torogao_sections: file.torogao_ext_count,
            trailing_bytes: file.trailing_bytes,
        },
        Err(_) => ContainerScan {
            ok: false,
            sections: Vec::new(),
            torogao_sections: 0,
            trailing_bytes: 0,
        },
    }
}

/// Assembles a complete container from a payload document,
/// texture slots, and EXT sections (creation paths: template
/// instantiation and character creation build new files rather
/// than doing surgery on an existing one).
pub fn build_inp(payload: &[u8], textures: &[TextureSlot], ext_sections: &[ExtSection]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(CONTAINER_MAGIC);
    put_be(&mut out, payload.len() as u32);
    out.extend_from_slice(payload);
    out.extend_from_slice(TEXTURE_MAGIC);
    put_be(&mut out, textures.len() as u32);
    for slot in textures {
        put_be(&mut out, slot.data.len() as u32);
        out.push(slot.tag);
        out.extend_from_slice(&slot.data);
    }
    if !ext_sections.is_empty() {
        out.extend_from_slice(EXT_MAGIC);
        put_be(&mut out, ext_sections.len() as u32);
        for section in ext_sections {
            put_be(&mut out, section.name.len() as u32);
            out.extend_from_slice(section.name.as_bytes());
            put_be(&mut out, section.data.len() as u32);
            out.extend_from_slice(&section.data);
        }
    }
    out
}

/// Byte surgery: the source container's prefix verbatim, plus
/// an EXT region holding every source section in order with
/// the Torogao section replaced by `ext` (or appended when the
/// source has none). A duplicate Torogao section is dropped —
/// the section is single by contract.
pub fn write_with_ext(source: &InpFile, ext: &Value) -> Vec<u8> {
    let ext_bytes = to_d_json(ext).into_bytes();
    let mut kept: Vec<ExtSection> = Vec::new();
    let mut replaced = false;
    for section in &source.ext_sections {
        if section.name == TOROGAO_EXT_SECTION {
            if !replaced {
                kept.push(ExtSection {
                    name: TOROGAO_EXT_SECTION.to_string(),
                    data: ext_bytes.clone(),
                });
                replaced = true;
            }
        } else {
            kept.push(section.clone());
        }
    }
    if !replaced {
        kept.push(ExtSection {
            name: TOROGAO_EXT_SECTION.to_string(),
            data: ext_bytes,
        });
    }
    let mut out = source.prefix.clone();
    if !kept.is_empty() {
        out.extend_from_slice(EXT_MAGIC);
        put_be(&mut out, kept.len() as u32);
        for section in &kept {
            put_be(&mut out, section.name.len() as u32);
            out.extend_from_slice(section.name.as_bytes());
            put_be(&mut out, section.data.len() as u32);
            out.extend_from_slice(&section.data);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{ExtSection, TextureSlot, build_inp, read_inp, scan_container, write_with_ext};
    use serde_json::json;

    fn fixture() -> Vec<u8> {
        build_inp(
            br#"{"meta":{"name":"T"},"nodes":{"type":"Node","name":"Root","uuid":1,"children":[]},"param":[]}"#,
            &[TextureSlot {
                tag: 0,
                data: vec![1, 2, 3],
            }],
            &[ExtSection {
                name: "io.ntranch.torogao".to_string(),
                data: br#"{"format":"torogao-ext/1"}"#.to_vec(),
            }],
        )
    }

    #[test]
    fn container_round_trips_byte_identically() {
        let bytes = fixture();
        let file = read_inp(&bytes).expect("fixture parses");
        assert_eq!(file.textures.len(), 1);
        assert_eq!(file.torogao_ext_count, 1);
        assert_eq!(file.trailing_bytes, 0);
        let rebuilt = build_inp(&file.payload_bytes, &file.textures, &file.ext_sections);
        assert_eq!(rebuilt, bytes);
    }

    #[test]
    fn surgery_preserves_the_prefix_verbatim() {
        let bytes = fixture();
        let file = read_inp(&bytes).expect("fixture parses");
        let out = write_with_ext(&file, &json!({"format": "torogao-ext/1", "x": 1}));
        let reread = read_inp(&out).expect("surgery output parses");
        assert_eq!(reread.prefix, file.prefix);
        let (doc, found, malformed) = reread.torogao_ext();
        assert!(found && !malformed);
        assert_eq!(doc["x"], json!(1));
    }

    #[test]
    fn malformed_containers_refuse_with_the_typed_code() {
        for bytes in [
            Vec::new(),
            vec![0u8; 8],
            b"NOTINP!!".to_vec(),
            b"TRNSRTS\0\xff\xff\xff\xff".to_vec(),
        ] {
            let err = read_inp(&bytes).expect_err("must refuse");
            assert_eq!(err.code(), "TG-CONTAINER-MALFORMED", "bytes: {bytes:?}");
        }
    }

    #[test]
    fn scan_reports_duplicates_and_trailing_bytes() {
        let mut bytes = fixture();
        // Append a second EXT region is not how duplicates
        // arise; build one directly instead: two torogao
        // sections inside one EXT region, plus slack.
        let dup = build_inp(
            br#"{}"#,
            &[],
            &[
                ExtSection {
                    name: "io.ntranch.torogao".to_string(),
                    data: b"{}".to_vec(),
                },
                ExtSection {
                    name: "io.ntranch.torogao".to_string(),
                    data: b"{}".to_vec(),
                },
            ],
        );
        bytes = dup;
        bytes.extend_from_slice(&[9, 9, 9]);
        let scan = scan_container(&bytes);
        assert!(scan.ok);
        assert_eq!(scan.torogao_sections, 2);
        assert_eq!(scan.trailing_bytes, 3);
    }
}
