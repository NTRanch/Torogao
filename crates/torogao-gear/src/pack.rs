//! The deterministic `.tgearpack` format (§2.9 step 4).
//!
//! Layout: a magic line, a canonical text manifest (documents
//! sorted by id, payloads sorted by name, each with its
//! sha256 and byte length), an end-of-manifest line, then the
//! entries' raw bytes in manifest order. No timestamps, no
//! host data, no ordering freedom: packing the same inputs
//! twice produces byte-identical output (gate T4-G1.3).
//!
//! Installing a pack (§2.9 step 5) writes it into the gear
//! library under a caller-provided Addition S data root. It
//! never wears an item and never enables anything.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::document::GearDocument;
use crate::emit::document_text;
use crate::payloads::{PayloadLookup, sha256_hex};
use crate::refusal::{
    GearRefusal, GearResult, TG_GEAR_BOUND, TG_GEAR_PART_HASH, TG_GEAR_PART_MISSING, TG_GEAR_SCHEMA,
};

/// Pack magic, first line of every pack.
pub const PACK_MAGIC: &str = "TGEARPK1";
/// Pack format token named in the manifest.
pub const PACK_FORMAT: &str = "torogao-gear-pack/1";
/// Documents per pack, maximum.
pub const PACK_DOCUMENTS_MAX: usize = 256;
/// Payloads per pack, maximum.
pub const PACK_PAYLOADS_MAX: usize = 1024;
/// Document bytes per pack, maximum.
pub const PACK_DOCUMENT_BYTES_MAX: usize = 16 * 1024 * 1024;

/// One pack read back: its documents (canonical text form)
/// and its payloads by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GearPack {
    /// Document canonical texts, sorted by document id.
    pub document_texts: Vec<String>,
    /// Payload bytes by payload name.
    pub payloads: BTreeMap<String, Vec<u8>>,
}

fn schema(message: impl Into<String>) -> GearRefusal {
    GearRefusal::new(TG_GEAR_SCHEMA, "pack", "$", message)
}

fn check_name(kind: &str, name: &str) -> GearResult<()> {
    if name.is_empty() || name.chars().any(|c| c.is_whitespace()) {
        return Err(schema(format!(
            "{kind} name `{name}` cannot be recorded in a pack manifest"
        )));
    }
    Ok(())
}

/// Builds the pack bytes for a set of validated documents and
/// the payload store their parts resolve against. Documents
/// are the caller's responsibility to have validated
/// ([`crate::validate::validate_document`] +
/// [`crate::validate::validate_pack`]); this writer enforces
/// presence, name hygiene, and the pack bounds.
pub fn build_pack(documents: &[GearDocument], payloads: &dyn PayloadLookup) -> GearResult<Vec<u8>> {
    if documents.len() > PACK_DOCUMENTS_MAX {
        return Err(GearRefusal::new(
            TG_GEAR_BOUND,
            "pack",
            "documents",
            format!("{} documents exceed the pack bound", documents.len()),
        ));
    }
    let mut docs: Vec<&GearDocument> = documents.iter().collect();
    docs.sort_by(|a, b| a.id.cmp(&b.id));
    let mut payload_names: Vec<String> = Vec::new();
    for doc in &docs {
        check_name("document id", &doc.id)?;
        for part in &doc.parts {
            check_name("payload", &part.name)?;
            if !payload_names.contains(&part.name) {
                payload_names.push(part.name.clone());
            }
        }
    }
    payload_names.sort();
    if payload_names.len() > PACK_PAYLOADS_MAX {
        return Err(GearRefusal::new(
            TG_GEAR_BOUND,
            "pack",
            "parts",
            format!("{} payloads exceed the pack bound", payload_names.len()),
        ));
    }
    let mut manifest = String::new();
    manifest.push_str(PACK_MAGIC);
    manifest.push('\n');
    manifest.push_str(&format!("pack {PACK_FORMAT}\n"));
    manifest.push_str(&format!("documents {}\n", docs.len()));
    let mut body: Vec<u8> = Vec::new();
    for doc in &docs {
        let text = document_text(doc);
        manifest.push_str(&format!(
            "doc {} {} {}\n",
            doc.id,
            sha256_hex(text.as_bytes()),
            text.len()
        ));
        body.extend_from_slice(text.as_bytes());
    }
    for name in &payload_names {
        let bytes = payloads.payload(name).ok_or_else(|| {
            GearRefusal::new(
                TG_GEAR_PART_MISSING,
                "pack",
                "parts",
                format!("payload `{name}` is absent"),
            )
        })?;
        manifest.push_str(&format!(
            "payload {name} {} {}\n",
            sha256_hex(&bytes),
            bytes.len()
        ));
        body.extend_from_slice(&bytes);
    }
    manifest.push_str("end-manifest\n");
    let mut out = manifest.into_bytes();
    out.extend_from_slice(&body);
    Ok(out)
}

/// Reads and verifies a pack: structure, manifest, entry
/// bounds, and every entry's content hash.
pub fn read_pack(bytes: &[u8]) -> GearResult<GearPack> {
    // The manifest region is text; entry bodies may be binary.
    // Locate the end-of-manifest line in bytes first, then
    // decode only the manifest.
    let marker: &[u8] = b"end-manifest\n";
    let manifest_end = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|at| at + marker.len())
        .ok_or_else(|| schema("pack manifest is unterminated"))?;
    let text = std::str::from_utf8(&bytes[..manifest_end])
        .map_err(|_| schema("pack manifest is not valid UTF-8"))?;
    let mut lines = text.split_inclusive('\n');
    let magic = lines.next().unwrap_or("");
    if magic != format!("{PACK_MAGIC}\n") {
        return Err(schema("pack magic mismatch"));
    }
    let mut offset = magic.len();
    let mut docs_meta: Vec<(String, String, usize)> = Vec::new();
    let mut payloads_meta: Vec<(String, String, usize)> = Vec::new();
    let mut saw_pack_line = false;
    for line in &mut lines {
        offset += line.len();
        let line = line.trim_end_matches('\n');
        if line == "end-manifest" {
            break;
        }
        let tokens: Vec<&str> = line.split(' ').collect();
        match tokens.as_slice() {
            ["pack", token] if *token == PACK_FORMAT => saw_pack_line = true,
            ["documents", _] => {}
            ["doc", id, hash, len] => {
                docs_meta.push((
                    id.to_string(),
                    hash.to_string(),
                    len.parse().map_err(|_| schema("bad doc length"))?,
                ));
            }
            ["payload", name, hash, len] => {
                payloads_meta.push((
                    name.to_string(),
                    hash.to_string(),
                    len.parse().map_err(|_| schema("bad payload length"))?,
                ));
            }
            _ => return Err(schema(format!("unrecognized manifest line `{line}`"))),
        }
    }
    if !saw_pack_line {
        return Err(schema("pack manifest lacks its format line"));
    }
    if docs_meta.len() > PACK_DOCUMENTS_MAX || payloads_meta.len() > PACK_PAYLOADS_MAX {
        return Err(GearRefusal::new(
            TG_GEAR_BOUND,
            "pack",
            "documents",
            "pack entry counts exceed the pack bounds",
        ));
    }
    let mut cursor = offset;
    let take = |cursor: &mut usize, len: usize| -> GearResult<&[u8]> {
        let end = cursor
            .checked_add(len)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| schema("pack body is truncated"))?;
        let slice = &bytes[*cursor..end];
        *cursor = end;
        Ok(slice)
    };
    let mut document_texts = Vec::new();
    let mut document_bytes_total: usize = 0;
    for (id, hash, len) in &docs_meta {
        let slice = take(&mut cursor, *len)?;
        document_bytes_total = document_bytes_total.saturating_add(*len);
        if sha256_hex(slice) != *hash {
            return Err(GearRefusal::new(
                TG_GEAR_PART_HASH,
                id,
                "pack",
                format!("document `{id}` bytes do not match the manifest hash"),
            ));
        }
        document_texts.push(
            String::from_utf8(slice.to_vec())
                .map_err(|_| schema("document bytes are not UTF-8"))?,
        );
    }
    if document_bytes_total > PACK_DOCUMENT_BYTES_MAX {
        return Err(GearRefusal::new(
            TG_GEAR_BOUND,
            "pack",
            "documents",
            "document bytes exceed the pack bound",
        ));
    }
    let mut payloads = BTreeMap::new();
    for (name, hash, len) in &payloads_meta {
        let slice = take(&mut cursor, *len)?;
        if sha256_hex(slice) != *hash {
            return Err(GearRefusal::new(
                TG_GEAR_PART_HASH,
                name,
                "pack",
                format!("payload `{name}` bytes do not match the manifest hash"),
            ));
        }
        payloads.insert(name.clone(), slice.to_vec());
    }
    if cursor != bytes.len() {
        return Err(schema("pack carries trailing bytes after its entries"));
    }
    Ok(GearPack {
        document_texts,
        payloads,
    })
}

/// Installs a pack into the gear library: `<data_root>/gear/
/// <stem>.tgearpack`, written via a sibling temp file +
/// rename. The pack is verified before it is installed.
/// Installing never wears an item (§2.9 step 5).
pub fn install_pack(pack_bytes: &[u8], data_root: &Path, stem: &str) -> GearResult<PathBuf> {
    read_pack(pack_bytes)?;
    check_name("pack stem", stem)?;
    let dir = data_root.join("gear");
    std::fs::create_dir_all(&dir).map_err(|e| {
        GearRefusal::new(TG_GEAR_SCHEMA, "pack", "install", format!("library: {e}"))
    })?;
    let target = dir.join(format!("{stem}.tgearpack"));
    let temp = dir.join(format!(".{stem}.tgearpack.tmp"));
    std::fs::write(&temp, pack_bytes)
        .map_err(|e| GearRefusal::new(TG_GEAR_SCHEMA, "pack", "install", format!("write: {e}")))?;
    std::fs::rename(&temp, &target)
        .map_err(|e| GearRefusal::new(TG_GEAR_SCHEMA, "pack", "install", format!("rename: {e}")))?;
    Ok(target)
}
