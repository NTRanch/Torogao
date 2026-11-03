//! The exporter: Torogao GLB + vrm-profile/1 → VRM 1.0.
//!
//! The exporter writes exactly three things into the source
//! document: `extensions.VRMC_vrm`, `extensions.VRMC_springBone`
//! (only when a spring record is supplied), and the
//! `extensionsUsed` entries naming them. Before returning, it
//! re-parses its own output and proves the census and the
//! outside-extensions JSON are identical to the source — the
//! read-back pattern, applied inside the export itself.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::bones::{BONE_DEFS, bone_def, resolve_source_node};
use crate::error::{VrmError, VrmResult};
use crate::expressions::{ExpressionMap, build_expressions, morph_addresses};
use crate::glb::{
    canonical_numbers, census, first_diff_path, json_outside_extensions, parse_glb, root_map_mut,
    write_glb,
};
use crate::profile::VrmProfile;
use crate::spring::SpringSpec;

/// The `VRMC_vrm` spec version this crate emits.
pub const VRM_SPEC_VERSION: &str = "1.0";

/// The optional inputs to an export. The profile is not here:
/// it is a separate argument so its absence is unrepresentable
/// as anything but the explicit `None` the exporter refuses.
#[derive(Clone, Copy, Default)]
pub struct ExportOptions<'a> {
    /// The governed expression map, when the character has one.
    pub expressions: Option<&'a ExpressionMap>,
    /// The spring record, already resolved against the source
    /// model by the caller's [`SpringSpec::load`] — or built via
    /// [`crate::prepare_spring`] against these exact GLB bytes.
    pub spring: Option<&'a SpringSpec>,
}

/// Export a VRM 1.0 file. `profile` is required: `None` is the
/// loud refusal ([`VrmError::MissingProfile`]), and a checked
/// profile guarantees every permission field is written —
/// no field is ever defaulted here.
pub fn export_vrm(
    glb_bytes: &[u8],
    profile: Option<&VrmProfile>,
    options: &ExportOptions<'_>,
) -> VrmResult<Vec<u8>> {
    let profile = profile.ok_or(VrmError::MissingProfile)?;
    let source = parse_glb(glb_bytes)?;
    let human_bones = resolve_humanoid(&source.json)?;
    let addresses = morph_addresses(&source.json)?;
    let built = build_expressions(options.expressions, &addresses)?;

    let mut vrm = Map::new();
    vrm.insert("specVersion".to_string(), json!(VRM_SPEC_VERSION));
    vrm.insert("meta".to_string(), profile.meta_json());
    vrm.insert("humanoid".to_string(), json!({ "humanBones": human_bones }));
    if !built.preset.is_empty() || !built.custom.is_empty() {
        let mut expressions = Map::new();
        if !built.preset.is_empty() {
            expressions.insert("preset".to_string(), Value::Object(built.preset));
        }
        if !built.custom.is_empty() {
            expressions.insert("custom".to_string(), Value::Object(built.custom));
        }
        vrm.insert("expressions".to_string(), Value::Object(expressions));
    }

    let mut json = source.json.clone();
    let mut extension_names: Vec<&str> = vec!["VRMC_vrm"];
    {
        let root = root_map_mut(&mut json);
        let extensions = root
            .entry("extensions".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let extensions = extensions
            .as_object_mut()
            .ok_or_else(|| VrmError::GlbJsonParse {
                detail: "root `extensions` is not an object".to_string(),
            })?;
        extensions.insert("VRMC_vrm".to_string(), Value::Object(vrm));
        if let Some(spring) = options.spring {
            extensions.insert("VRMC_springBone".to_string(), spring.to_json());
            extension_names.push("VRMC_springBone");
        }
        let used = root
            .entry("extensionsUsed".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let used = used.as_array_mut().ok_or_else(|| VrmError::GlbJsonParse {
            detail: "root `extensionsUsed` is not an array".to_string(),
        })?;
        for name in extension_names {
            if !used.iter().any(|entry| entry.as_str() == Some(name)) {
                used.push(json!(name));
            }
        }
    }

    let out_bytes = write_glb(&json, &source.bin)?;
    prove_byte_preservation(glb_bytes, &out_bytes)?;
    Ok(out_bytes)
}

/// Resolve the source's nodes into the VRM `humanBones` object:
/// VRM name → `{node}`. Every required bone must resolve.
fn resolve_humanoid(json: &Value) -> VrmResult<Map<String, Value>> {
    let nodes = json
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut by_torogao: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        let Some(name) = node.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(torogao) = resolve_source_node(name) else {
            continue;
        };
        if torogao == "Root" {
            continue;
        }
        if by_torogao.insert(torogao, index).is_some() {
            return Err(VrmError::DuplicateBone {
                torogao: torogao.to_string(),
            });
        }
    }
    let mut missing: Vec<String> = Vec::new();
    for def in BONE_DEFS.iter().filter(|def| def.required) {
        if !by_torogao.contains_key(def.torogao) {
            missing.push(def.vrm.to_string());
        }
    }
    if !missing.is_empty() {
        missing.sort_unstable();
        return Err(VrmError::MissingRequiredBones { bones: missing });
    }
    let mut human_bones = Map::new();
    for (torogao, node) in &by_torogao {
        let def = bone_def(torogao).expect("resolved bones are all in the map");
        human_bones.insert(def.vrm.to_string(), json!({ "node": node }));
    }
    Ok(human_bones)
}

/// The exporter's internal read-back: the output must census
/// identically to the source and differ nowhere outside the
/// extension blocks.
fn prove_byte_preservation(source_bytes: &[u8], out_bytes: &[u8]) -> VrmResult<()> {
    let source = parse_glb(source_bytes)?;
    let out = parse_glb(out_bytes)?;
    if census(&source) != census(&out) {
        return Err(VrmError::CensusMismatch {
            detail: "output census differs from source census".to_string(),
        });
    }
    let source_outside = canonical_numbers(&json_outside_extensions(&source.json));
    let out_outside = canonical_numbers(&json_outside_extensions(&out.json));
    if source_outside != out_outside {
        let diff = first_diff_path(&source_outside, &out_outside)
            .unwrap_or_else(|| "values differ".to_string());
        return Err(VrmError::CensusMismatch {
            detail: format!("output JSON differs from source outside the extension blocks: {diff}"),
        });
    }
    Ok(())
}
