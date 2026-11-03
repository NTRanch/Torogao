//! GLB container handling and the structural census.
//!
//! The exporter touches exactly two places in a source GLB: the
//! root `extensions` object and the `extensionsUsed` list. The
//! census is the proof surface for that claim: it fingerprints the
//! binary chunk and every structural count a consumer can observe,
//! and [`json_outside_extensions`] isolates everything else in the
//! JSON chunk for a deep-equality check.

use serde_json::{Map, Value};

use crate::error::{VrmError, VrmResult};
use crate::sha256::sha256_hex;

/// Largest GLB input this crate will parse, in bytes (256 MiB).
pub const GLB_BYTES_MAX: usize = 268_435_456;

/// GLB magic (`glTF`).
const GLB_MAGIC: u32 = 0x4654_6c67;
/// GLB container version this crate reads and writes.
const GLB_VERSION: u32 = 2;
/// Chunk type: JSON.
const CHUNK_JSON: u32 = 0x4e4f_534a;
/// Chunk type: binary buffer.
const CHUNK_BIN: u32 = 0x004e_4942;
/// Header size in bytes.
const HEADER_BYTES: usize = 12;
/// Chunk header size in bytes.
const CHUNK_HEADER_BYTES: usize = 8;

/// A parsed GLB: the JSON document as a value, and the binary
/// chunk exactly as stored (padding included).
#[derive(Clone, Debug)]
pub struct GlbFile {
    /// The glTF JSON document.
    pub json: Value,
    /// The binary chunk bytes, or empty when the file has none.
    pub bin: Vec<u8>,
}

/// Parse a GLB byte string. Bounded: inputs over
/// [`GLB_BYTES_MAX`] are refused before any chunk is walked.
pub fn parse_glb(bytes: &[u8]) -> VrmResult<GlbFile> {
    if bytes.len() > GLB_BYTES_MAX {
        return Err(VrmError::CensusMismatch {
            detail: format!(
                "input is {} bytes, over the {GLB_BYTES_MAX}-byte bound",
                bytes.len()
            ),
        });
    }
    if bytes.len() < HEADER_BYTES {
        return Err(VrmError::GlbTooShort {
            actual_bytes: bytes.len(),
        });
    }
    if read_u32(bytes, 0) != GLB_MAGIC {
        return Err(VrmError::GlbBadMagic);
    }
    let version = read_u32(bytes, 4);
    if version != GLB_VERSION {
        return Err(VrmError::GlbUnsupportedVersion { found: version });
    }
    let mut json: Option<Value> = None;
    let mut bin: Vec<u8> = Vec::new();
    let mut offset = HEADER_BYTES;
    while offset + CHUNK_HEADER_BYTES <= bytes.len() {
        let chunk_len = read_u32(bytes, offset) as usize;
        let chunk_type = read_u32(bytes, offset + 4);
        let data_start = offset + CHUNK_HEADER_BYTES;
        let data_end = match data_start.checked_add(chunk_len) {
            Some(end) if end <= bytes.len() => end,
            _ => {
                return Err(VrmError::CensusMismatch {
                    detail: format!("chunk at byte {offset} overruns the file"),
                });
            }
        };
        let data = &bytes[data_start..data_end];
        if chunk_type == CHUNK_JSON {
            let text = std::str::from_utf8(data).map_err(|err| VrmError::GlbJsonParse {
                detail: err.to_string(),
            })?;
            json = Some(
                serde_json::from_str(text).map_err(|err| VrmError::GlbJsonParse {
                    detail: err.to_string(),
                })?,
            );
        } else if chunk_type == CHUNK_BIN {
            bin = data.to_vec();
        }
        offset = data_end;
    }
    match json {
        Some(json) => Ok(GlbFile { json, bin }),
        None => Err(VrmError::GlbMissingJsonChunk),
    }
}

/// Serialize a JSON document plus binary chunk as a GLB. The JSON
/// chunk is space-padded and the binary chunk zero-padded, each
/// to a 4-byte boundary, per the glTF container rules.
pub fn write_glb(json: &Value, bin: &[u8]) -> VrmResult<Vec<u8>> {
    let mut json_bytes = serde_json::to_vec(json).map_err(|err| VrmError::GlbJsonParse {
        detail: err.to_string(),
    })?;
    pad_to_four(&mut json_bytes, b' ');
    let mut bin_bytes = bin.to_vec();
    pad_to_four(&mut bin_bytes, 0);
    let total =
        HEADER_BYTES + CHUNK_HEADER_BYTES + json_bytes.len() + CHUNK_HEADER_BYTES + bin_bytes.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&GLB_MAGIC.to_le_bytes());
    out.extend_from_slice(&GLB_VERSION.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
    out.extend_from_slice(&bin_bytes);
    Ok(out)
}

/// The structural census of a parsed GLB: everything a consumer
/// can observe except the extension blocks. Two GLBs with equal
/// censuses carry the same geometry, skinning, and morph data.
#[derive(Clone, Debug, PartialEq)]
pub struct Census {
    /// Accessor count.
    pub accessor_count: usize,
    /// Animation count.
    pub animation_count: usize,
    /// Binary chunk length in bytes, as stored.
    pub bin_bytes: usize,
    /// Binary chunk SHA-256, lowercase hex.
    pub bin_sha256: String,
    /// BufferView count.
    pub buffer_view_count: usize,
    /// Image count.
    pub image_count: usize,
    /// Material count.
    pub material_count: usize,
    /// Mesh count.
    pub mesh_count: usize,
    /// Node count.
    pub node_count: usize,
    /// Node names in document order (empty string when unnamed).
    pub node_names: Vec<String>,
    /// Skin joint counts in document order.
    pub skin_joint_counts: Vec<usize>,
    /// Morph target names per mesh, in document order (empty when
    /// the mesh declares no `extras.targetNames`).
    pub target_names: Vec<Vec<String>>,
}

/// Take the census of a parsed GLB.
pub fn census(glb: &GlbFile) -> Census {
    let root = glb.json.as_object();
    let array_len = |key: &str| -> usize {
        root.and_then(|map| map.get(key))
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    };
    let nodes = root
        .and_then(|map| map.get("nodes"))
        .and_then(Value::as_array);
    let node_names = nodes.map_or_else(Vec::new, |nodes| {
        nodes
            .iter()
            .map(|node| {
                node.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            })
            .collect()
    });
    let skins = root
        .and_then(|map| map.get("skins"))
        .and_then(Value::as_array);
    let skin_joint_counts = skins.map_or_else(Vec::new, |skins| {
        skins
            .iter()
            .map(|skin| {
                skin.get("joints")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
            })
            .collect()
    });
    let meshes = root
        .and_then(|map| map.get("meshes"))
        .and_then(Value::as_array);
    let target_names = meshes.map_or_else(Vec::new, |meshes| {
        meshes.iter().map(mesh_target_names).collect()
    });
    Census {
        accessor_count: array_len("accessors"),
        animation_count: array_len("animations"),
        bin_bytes: glb.bin.len(),
        bin_sha256: sha256_hex(&glb.bin),
        buffer_view_count: array_len("bufferViews"),
        image_count: array_len("images"),
        material_count: array_len("materials"),
        mesh_count: array_len("meshes"),
        node_count: array_len("nodes"),
        node_names,
        skin_joint_counts,
        target_names,
    }
}

/// The morph target names a mesh declares (glTF stores them in
/// `mesh.extras.targetNames`, parallel to the primitives'
/// `targets` arrays).
pub fn mesh_target_names(mesh: &Value) -> Vec<String> {
    mesh.get("extras")
        .and_then(|extras| extras.get("targetNames"))
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .map(|name| name.as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The morph target count of a mesh: the declared name count when
/// names exist, else the first primitive's `targets` length.
pub fn mesh_target_count(mesh: &Value) -> usize {
    let names = mesh_target_names(mesh);
    if !names.is_empty() {
        return names.len();
    }
    mesh.get("primitives")
        .and_then(Value::as_array)
        .and_then(|primitives| primitives.first())
        .and_then(|primitive| primitive.get("targets"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

/// A copy of the JSON document with the two keys the exporter is
/// allowed to change removed, for deep-equality comparison.
pub fn json_outside_extensions(json: &Value) -> Value {
    let mut clone = json.clone();
    if let Some(root) = clone.as_object_mut() {
        root.remove("extensions");
        root.remove("extensionsUsed");
    }
    clone
}

/// A copy of the JSON value with every non-integer number
/// rewritten through its `f64` value, for comparison. Number
/// equality in serde_json values can be lexical (the
/// `arbitrary_precision` build another workspace member
/// enables), and exporters spell the same float differently —
/// zero-padded exponents (`3.259629011154175e-09`) vs
/// serde_json's writer (`3.259629011154175e-9`). Byte
/// preservation is a claim about values, not decimal
/// spellings, so comparisons canonicalize first. Value
/// exactness itself is guaranteed separately: the crate
/// enables serde_json's `float_roundtrip` feature, without
/// which the parser's fast path lands 1 ulp off on hard
/// decimals and a round-trip silently changes geometry.
/// Integers are exact and pass through untouched.
pub fn canonical_numbers(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical_numbers).collect()),
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                value.clone()
            } else if let Some(float) = number.as_f64().and_then(serde_json::Number::from_f64) {
                Value::Number(float)
            } else {
                value.clone()
            }
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), canonical_numbers(item)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// The first JSON path at which two values differ, as a
/// human-readable line (`$.nodes[15].translation[2]: A vs B`),
/// or `None` when they are equal. Validation failures name
/// their diff; this is how they find it.
pub fn first_diff_path(a: &Value, b: &Value) -> Option<String> {
    fn walk(a: &Value, b: &Value, path: &str) -> Option<String> {
        match (a, b) {
            (Value::Object(ma), Value::Object(mb)) => {
                for (key, va) in ma {
                    match mb.get(key) {
                        None => return Some(format!("{path}.{key}: missing on the right")),
                        Some(vb) => {
                            if let Some(found) = walk(va, vb, &format!("{path}.{key}")) {
                                return Some(found);
                            }
                        }
                    }
                }
                for key in mb.keys() {
                    if !ma.contains_key(key) {
                        return Some(format!("{path}.{key}: missing on the left"));
                    }
                }
                None
            }
            (Value::Array(aa), Value::Array(ab)) => {
                if aa.len() != ab.len() {
                    return Some(format!("{path}: array length {} vs {}", aa.len(), ab.len()));
                }
                for (index, (va, vb)) in aa.iter().zip(ab.iter()).enumerate() {
                    if let Some(found) = walk(va, vb, &format!("{path}[{index}]")) {
                        return Some(found);
                    }
                }
                None
            }
            _ => {
                if a != b {
                    Some(format!("{path}: {a} vs {b}"))
                } else {
                    None
                }
            }
        }
    }
    walk(a, b, "$")
}

/// Node parent indices from the `children` lists (`None` for
/// roots). Used by the spring chain ancestry check.
pub fn node_parents(json: &Value) -> Vec<Option<usize>> {
    let nodes = json
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut parents = vec![None; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for child in children {
                if let Some(child) = child.as_u64() {
                    if let Some(slot) = parents.get_mut(child as usize) {
                        *slot = Some(index);
                    }
                }
            }
        }
    }
    parents
}

/// Whether `ancestor` is a strict ancestor of `node` in the
/// hierarchy the parent table describes.
pub fn is_ancestor(parents: &[Option<usize>], ancestor: usize, node: usize) -> bool {
    let mut cursor = parents.get(node).copied().flatten();
    while let Some(current) = cursor {
        if current == ancestor {
            return true;
        }
        cursor = parents.get(current).copied().flatten();
    }
    false
}

/// The root object as a mutable map, asserting the shape
/// `parse_glb` already validated (a GLB JSON document is an
/// object by construction of the parse).
pub fn root_map_mut(json: &mut Value) -> &mut Map<String, Value> {
    json.as_object_mut()
        .expect("GLB JSON document is an object")
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn pad_to_four(bytes: &mut Vec<u8>, pad: u8) {
    while bytes.len() % 4 != 0 {
        bytes.push(pad);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{census, is_ancestor, node_parents, parse_glb, write_glb};
    use crate::error::VrmError;

    /// A written GLB parses back to the same census.
    #[test]
    fn written_glb_round_trips_through_parse() {
        let json = json!({
            "asset": {"version": "2.0"},
            "nodes": [{"name": "Hips"}, {"name": "Head"}],
            "meshes": [],
        });
        let bytes = write_glb(&json, &[1, 2, 3, 4]).expect("write");
        let parsed = parse_glb(&bytes).expect("parse");
        let report = census(&parsed);
        assert_eq!(report.node_count, 2);
        assert_eq!(
            report.node_names,
            vec!["Hips".to_string(), "Head".to_string()]
        );
        assert_eq!(report.bin_bytes, 4);
    }

    /// A non-GLB input fails by name, not by panic.
    #[test]
    fn junk_input_is_bad_magic() {
        let err = parse_glb(b"not a glb at all").expect_err("must fail");
        assert!(matches!(err, VrmError::GlbBadMagic));
    }

    /// Regression (D-748 proving): the same float under two
    /// decimal spellings must canonicalize equal. Guards the
    /// float_roundtrip requirement — without exact parsing
    /// this pair can land 1 ulp apart inside a document.
    #[test]
    fn number_spellings_canonicalize_equal() {
        let a: serde_json::Value = serde_json::from_str("3.259629011154175e-09").expect("a parses");
        let b: serde_json::Value = serde_json::from_str("3.259629011154175e-9").expect("b parses");
        assert_eq!(a.as_f64(), b.as_f64(), "parse consistency: {a:?} vs {b:?}");
        assert_eq!(super::canonical_numbers(&a), super::canonical_numbers(&b));
    }

    /// Ancestry walks the parent table.
    #[test]
    fn ancestry_follows_children() {
        let json = json!({"nodes": [{"children": [1]}, {"children": [2]}, {}]});
        let parents = node_parents(&json);
        assert!(is_ancestor(&parents, 0, 2));
        assert!(is_ancestor(&parents, 1, 2));
        assert!(!is_ancestor(&parents, 2, 0));
        assert!(!is_ancestor(&parents, 0, 0));
    }
}
