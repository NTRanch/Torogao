//! GLB container census and 3D expression-map governance,
//! ported from the D `torogao.cli.glb` module.

use serde_json::{json, Value};

use crate::packs::pack_str;
use crate::util::jget;

pub const EXPR_MAP_3D_FORMAT: &str = "torogao-expression-map-3d/1";

pub struct GlbInfo {
    pub animation_count: i64,
    pub gltf: Value,
    pub mesh_count: i64,
    pub morph_count: i64,
    pub morph_targets: Vec<String>,
    pub node_names: Vec<String>,
    pub skin_names: Vec<String>,
}

fn read_u32le(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
}

pub fn load_glb(path: &str) -> Result<GlbInfo, String> {
    let bytes = std::fs::read(path)
        .map_err(|_| format!("TG-GLB-MALFORMED: cannot read '{path}'"))?;
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return Err(format!(
            "TG-GLB-MALFORMED: '{path}' is not a GLB container (bad magic)"
        ));
    }
    if read_u32le(&bytes, 4) != 2 {
        return Err("TG-GLB-MALFORMED: GLB version must be 2".to_string());
    }
    let json_length = read_u32le(&bytes, 12) as usize;
    let json_type = read_u32le(&bytes, 16);
    if json_type != 0x4E4F_534A || 20 + json_length > bytes.len() {
        return Err("TG-GLB-MALFORMED: first chunk is not the JSON chunk".to_string());
    }
    let gltf: Value = serde_json::from_slice(&bytes[20..20 + json_length])
        .map_err(|e| format!("TG-GLB-MALFORMED: glTF JSON unparseable: {e}"))?;
    let mut info = GlbInfo {
        animation_count: 0,
        gltf: gltf.clone(),
        mesh_count: 0,
        morph_count: 0,
        morph_targets: Vec::new(),
        node_names: Vec::new(),
        skin_names: Vec::new(),
    };
    if let Some(meshes) = jget(&gltf, "meshes").as_array() {
        info.mesh_count = meshes.len() as i64;
        for mesh in meshes {
            let prims = jget(mesh, "primitives");
            let target_count = match prims.as_array().and_then(|p| p.first()) {
                Some(first) => count_of(&jget(first, "targets")),
                None => 0,
            };
            info.morph_count += target_count;
            if let Some(names) = jget(&jget(mesh, "extras"), "targetNames").as_array() {
                for (i, name) in names.iter().enumerate() {
                    if i as i64 >= target_count {
                        break;
                    }
                    let Some(name) = name.as_str() else { continue };
                    if !info.morph_targets.iter().any(|t| t == name) {
                        info.morph_targets.push(name.to_string());
                    }
                }
            }
        }
    }
    if let Some(nodes) = jget(&gltf, "nodes").as_array() {
        for node in nodes {
            let name = pack_str(node, "name");
            if !name.is_empty() {
                info.node_names.push(name);
            }
        }
    }
    if let Some(skins) = jget(&gltf, "skins").as_array() {
        for skin in skins {
            info.skin_names.push(pack_str(skin, "name"));
        }
    }
    info.animation_count = count_of(&jget(&gltf, "animations"));
    Ok(info)
}

fn count_of(v: &Value) -> i64 {
    v.as_array().map_or(0, |a| a.len() as i64)
}

pub fn glb_census(path: &str, info: &GlbInfo) -> Value {
    json!({
        "animations": info.animation_count,
        "file": path,
        "meshes": info.mesh_count,
        "morphPairs": info.morph_count,
        "morphTargetCount": info.morph_targets.len() as i64,
        "morphTargets": info.morph_targets,
        "nodeNames": info.node_names,
        "nodes": info.node_names.len() as i64,
        "skins": info.skin_names,
    })
}

pub fn check_expression_map(
    map: &Value,
    info: &GlbInfo,
    governed_names: &[String],
) -> Vec<String> {
    let mut problems = Vec::new();
    if pack_str(map, "format") != EXPR_MAP_3D_FORMAT {
        problems.push(format!(
            "TG-GLB-MALFORMED: expression map format is not '{EXPR_MAP_3D_FORMAT}'"
        ));
        return problems;
    }
    let expressions = jget(map, "expressions");
    let Some(expressions) = expressions.as_object() else {
        problems.push("TG-EXPR-MISSING: map carries no expressions object".to_string());
        return problems;
    };
    for name in governed_names {
        let entry = expressions.get(name);
        let Some(entry) = entry else {
            problems.push(format!(
                "TG-EXPR-MISSING: governed expression '{name}' has no 3D mapping"
            ));
            continue;
        };
        let morphs = jget(entry, "morphs");
        let Some(morphs) = morphs.as_object().filter(|m| !m.is_empty()) else {
            problems.push(format!(
                "TG-EXPR-MISSING: expression '{name}' maps to no morphs"
            ));
            continue;
        };
        for (morph, weight) in morphs {
            if !info.morph_targets.iter().any(|t| t == morph) {
                problems.push(format!(
                    "TG-GLB-UNKNOWN-MORPH: expression '{name}' names morph '{morph}', which the model does not have"
                ));
            }
            let Some(w) = weight.as_f64() else {
                problems.push(format!(
                    "TG-EXPR-RANGE: morph '{morph}' weight is not a number"
                ));
                continue;
            };
            if w < 0.0 || w > 1.0 {
                problems.push(format!(
                    "TG-EXPR-RANGE: morph '{morph}' weight is outside [0, 1]"
                ));
            }
        }
    }
    problems
}

pub fn resolve_expression(map: &Value, name: &str) -> Result<Value, String> {
    let expressions = jget(map, "expressions");
    let Some(expressions) = expressions.as_object() else {
        return Err("TG-EXPR-MISSING: map carries no expressions".to_string());
    };
    let Some(entry) = expressions.get(name) else {
        return Err(format!(
            "TG-EXPR-MISSING: governed expression '{name}' has no 3D mapping"
        ));
    };
    let morphs = jget(entry, "morphs");
    if !morphs.is_object() {
        return Err(format!(
            "TG-EXPR-MISSING: expression '{name}' maps to no morphs"
        ));
    }
    Ok(morphs.clone())
}
