//! Rig templates (templates.d port): listing and
//! instantiation of puppet templates plus physics-chain
//! presets into standard INP files.

use crate::paths::{existing_data_dirs, find_data_file};
use crate::util::jget;
use serde_json::{json, Map, Value};
use torogao_format::container::{build_inp, TextureSlot};
use torogao_format::jsonfmt::to_d_json;

/// One listed template.
pub struct TemplateInfo {
    /// Template name.
    pub name: String,
    /// Template kind.
    pub kind: String,
    /// Display title.
    pub title: String,
    /// Description.
    pub description: String,
    /// File the template was read from.
    pub path: String,
}

/// The string field of an object, "" when absent or not a
/// string (the reference's getStr).
fn get_str(value: &Value, key: &str) -> String {
    jget(value, key).as_str().unwrap_or("").to_string()
}

/// The numeric field of an object, `fallback` when absent
/// or not numeric (the reference's getNum).
fn get_num(value: &Value, key: &str, fallback: f64) -> f64 {
    match jget(value, key) {
        Value::Number(n) => n.as_f64().unwrap_or(fallback),
        _ => fallback,
    }
}

/// Lists all templates visible in the search directories
/// (first occurrence of a name wins).
pub fn list_templates() -> Vec<TemplateInfo> {
    let mut result = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for dir in existing_data_dirs("TOROGAO_TEMPLATES_DIR", "templates") {
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false)
                    && path.extension().and_then(|e| e.to_str()) == Some("json")
                {
                    files.push(path);
                }
            }
        }
        files.sort();
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let Ok(doc) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if !doc.is_object() || get_str(&doc, "format") != "torogao-template/1" {
                continue;
            }
            let name = get_str(&doc, "name");
            if name.is_empty() || seen.contains(&name) {
                continue;
            }
            seen.push(name.clone());
            result.push(TemplateInfo {
                name,
                kind: get_str(&doc, "kind"),
                title: get_str(&doc, "title"),
                description: get_str(&doc, "description"),
                path: file.to_string_lossy().into_owned(),
            });
        }
    }
    result
}

/// Loads a named data document (template or preset); the
/// noun in the error wording is the caller's.
fn load_named(name: &str, noun: &str) -> Result<Value, String> {
    let Some(path) = find_data_file("TOROGAO_TEMPLATES_DIR", "templates", &format!("{name}.json"))
    else {
        return Err(format!("{noun} '{name}' not found"));
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("{noun} '{name}' is unreadable: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{noun} '{name}' is unreadable: {e}"))
}

/// Loads a template by name.
pub fn load_template(name: &str) -> Result<Value, String> {
    load_named(name, "template")
}

/// Loads a physics-chain preset by name (surge F-8: the
/// error wording names it a preset, not a template).
pub fn load_preset(name: &str) -> Result<Value, String> {
    load_named(name, "preset")
}

/// Depth-first search for a node by name; on success the
/// path holds the child indices from the tree root (empty
/// when the root itself matches).
fn find_node_path(node: &Value, name: &str, path: &mut Vec<usize>) -> bool {
    if get_str(node, "name") == name {
        return true;
    }
    if let Some(kids) = node.get("children").and_then(Value::as_array) {
        for (i, kid) in kids.iter().enumerate() {
            let mut sub = Vec::new();
            if find_node_path(kid, name, &mut sub) {
                path.push(i);
                path.extend(sub);
                return true;
            }
        }
    }
    false
}

/// Navigates to the node at `path` (mutable).
fn navigate<'a>(node: &'a mut Value, path: &[usize]) -> &'a mut Value {
    let mut cursor = node;
    for idx in path {
        cursor = &mut cursor["children"][*idx];
    }
    cursor
}

/// Whether the payload carries a parameter of this name.
fn has_param(payload: &Value, name: &str) -> bool {
    payload["param"]
        .as_array()
        .map(|params| params.iter().any(|p| get_str(p, "name") == name))
        .unwrap_or(false)
}

/// Counts a node tree.
fn count_nodes(node: &Value) -> usize {
    let mut n = 1;
    if let Some(kids) = node.get("children").and_then(Value::as_array) {
        for kid in kids {
            n += count_nodes(kid);
        }
    }
    n
}

/// A transform record at (x, y).
fn json_transform(x: f64, y: f64) -> Value {
    json!({
        "trans": [x, y, 0.0],
        "rot": [0.0, 0.0, 0.0],
        "scale": [1.0, 1.0],
    })
}

/// A bare node record.
fn json_node(uuid: i64, name: &str, kind: &str, x: f64, y: f64) -> Value {
    json!({
        "type": kind,
        "uuid": uuid,
        "name": name,
        "enabled": true,
        "zsort": 0,
        "transform": json_transform(x, y),
        "lockToRoot": false,
    })
}

/// A quad segment part record.
fn json_part(uuid: i64, name: &str, x: f64, y: f64, half_w: f64, half_h: f64) -> Value {
    let mut node = json_node(uuid, name, "Part", x, y);
    node["mesh"] = json!({
        "verts": [-half_w, -half_h, half_w, -half_h, -half_w, half_h, half_w, half_h],
        "uvs": [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        "indices": [0, 1, 2, 2, 1, 3],
        "origin": [0.0, 0.0],
    });
    node["textures"] = json!([0, 4294967295u64, 4294967295u64]);
    node["blend_mode"] = json!("Normal");
    node["opacity"] = json!(1.0);
    node["mask_threshold"] = json!(0.5);
    node["tint"] = json!([1.0, 1.0, 1.0]);
    node["screenTint"] = json!([0.0, 0.0, 0.0]);
    node["emissionStrength"] = json!(1.0);
    node
}

/// A simple-physics node record; the enum member names
/// are written verbatim, exactly as the reference writes
/// them.
fn json_physics(
    uuid: i64,
    name: &str,
    param_uuid: i64,
    length: f64,
    angle_damping: f64,
    output_scale: f64,
) -> Value {
    let mut node = json_node(uuid, name, "SimplePhysics", 0.0, 0.0);
    node["param"] = json!(param_uuid);
    node["model_type"] = json!("Pendulum");
    node["map_mode"] = json!("AngleLength");
    node["gravity"] = json!(1.0);
    node["length"] = json!(length);
    node["frequency"] = json!(1.0);
    node["angle_damping"] = json!(angle_damping);
    node["length_damping"] = json!(0.5);
    node["output_scale"] = json!([output_scale, output_scale]);
    node["local_only"] = json!(false);
    node
}

/// The rotation binding a driven parameter carries.
fn rotation_binding(node_uuid: i64) -> Value {
    json!({
        "node": node_uuid,
        "param_name": "transform.r.z",
        "values": [[-0.6], [0.0], [0.6]],
        "isSet": [[true], [true], [true]],
    })
}

/// A parameter driving one chain node.
fn driven_param(uuid: i64, name: &str, node_uuid: i64) -> Value {
    json!({
        "uuid": uuid,
        "name": name,
        "is_vec2": false,
        "min": [-1.0, 0.0],
        "max": [1.0, 0.0],
        "defaults": [0.0, 0.0],
        "axis_points": [[-1.0, 0.0, 1.0], [0.0]],
        "bindings": [rotation_binding(node_uuid)],
    })
}

/// The 1x1 transparent placeholder PNG.
const PLACEHOLDER_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 218, 99, 100, 96, 248, 95, 15, 0, 2,
    135, 1, 128, 235, 71, 186, 146, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// Packs a payload document and texture slots into INP
/// bytes (the reference's packInp).
pub fn pack_inp(payload: &Value, textures: &[Vec<u8>]) -> Vec<u8> {
    let slots: Vec<TextureSlot> = textures
        .iter()
        .map(|data| TextureSlot {
            tag: 0,
            data: data.clone(),
        })
        .collect();
    build_inp(to_d_json(payload).as_bytes(), &slots, &[])
}

/// Per-stage allocated uuids during instantiation.
#[derive(Default, Clone, Copy)]
struct StageIds {
    /// Segment part uuid.
    part: i64,
    /// Physics node uuid.
    phys: i64,
    /// Rotation node uuid (0 after the last stage).
    rot: i64,
    /// Segment anchor uuid (0 after the last stage).
    anchor: i64,
}

/// Instantiates a puppet template plus physics-chain
/// presets into an INP file at `out_path`; on success
/// returns the summary document.
pub fn instantiate_template(
    template_name: &str,
    preset_names: &[String],
    out_path: &str,
) -> Result<Value, String> {
    let base = load_template(template_name)?;
    if get_str(&base, "kind") != "puppet" {
        return Err(format!(
            "template '{template_name}' is not a puppet template"
        ));
    }

    let mut payload = Value::Object(Map::new());
    payload["meta"] = base["meta"].clone();
    payload["physics"] = base["physics"].clone();
    payload["param"] = base["parameters"].clone();
    let mut tree = base["nodes"].clone();

    let mut next_uuid: i64 = 9001;
    let mut alloc = move || {
        let id = next_uuid;
        next_uuid += 1;
        id
    };

    let mut applied: Vec<String> = Vec::new();
    for preset_name in preset_names {
        if applied.contains(preset_name) {
            return Err(format!("preset '{preset_name}' applied twice"));
        }
        let preset = load_preset(preset_name)?;
        if get_str(&preset, "kind") != "physics-chain" {
            return Err(format!(
                "preset '{preset_name}' is not a physics-chain preset"
            ));
        }
        let attach_name = get_str(&preset, "attachTo");
        let mut attach_path: Vec<usize> = Vec::new();
        if !find_node_path(&tree, &attach_name, &mut attach_path) {
            return Err(format!(
                "preset '{preset_name}': anchor node '{attach_name}' not found in template '{template_name}'"
            ));
        }
        if let Some(inputs) = preset["inputs"].as_array() {
            for input in inputs {
                let pname = get_str(input, "parameter");
                if !has_param(&payload, &pname) {
                    return Err(format!(
                        "preset '{preset_name}': input parameter '{pname}' is not present in template '{template_name}'"
                    ));
                }
            }
        }
        let stages = preset["stages"].as_array().cloned().unwrap_or_default();
        let outputs = preset["outputs"].as_array().cloned().unwrap_or_default();
        if stages.is_empty() || outputs.len() != stages.len() {
            return Err(format!(
                "preset '{preset_name}': outputs must cover every stage exactly once"
            ));
        }

        // Allocate the chain: per stage a segment part, a
        // physics node, and (except after the last stage)
        // a rotation node and a segment anchor below it.
        let mut ids: Vec<StageIds> = Vec::new();
        for i in 0..stages.len() {
            let mut id = StageIds {
                part: alloc(),
                phys: alloc(),
                ..Default::default()
            };
            if i + 1 < stages.len() {
                id.rot = alloc();
                id.anchor = alloc();
            }
            ids.push(id);
        }
        let mut param_uuids: Vec<i64> = Vec::new();
        for output in &outputs {
            let pname = get_str(output, "parameter");
            if has_param(&payload, &pname) {
                return Err(format!(
                    "parameter '{pname}' from preset '{preset_name}' already exists in the puppet"
                ));
            }
            param_uuids.push(alloc());
        }

        // Build the fragment bottom-up: the tail stage
        // first, wrapped by the previous stage's
        // rotation/anchor.
        let mut cursor_children: Option<Vec<Value>> = None;
        for i in (0..stages.len()).rev() {
            let stage = &stages[i];
            let seg_len = get_num(stage, "segmentLength", 30.0);
            let mut outp: Option<&Value> = None;
            for output in &outputs {
                if get_num(output, "stage", -1.0) as usize == i + 1 {
                    outp = Some(output);
                }
            }
            let Some(outp) = outp else {
                return Err(format!(
                    "preset '{preset_name}': no output for stage {}",
                    i + 1
                ));
            };
            let scale = get_num(jget(outp, "scale"), "value", 45.0);
            let ease = get_num(jget(stage, "easeOfSwinging"), "chosen", 0.0);
            let part = json_part(
                ids[i].part,
                &format!("{preset_name}Seg{}", i + 1),
                0.0,
                seg_len / 2.0,
                18.0,
                seg_len / 2.0,
            );
            let phys = json_physics(
                ids[i].phys,
                &format!("{preset_name}Phys{}", i + 1),
                param_uuids[i],
                seg_len,
                1.0 - ease,
                scale / 90.0,
            );
            let mut kids = vec![part, phys];
            if let Some(deeper) = cursor_children.take() {
                let mut anchor = json_node(
                    ids[i].anchor,
                    &format!("{preset_name}SegAnchor{}", i + 2),
                    "Node",
                    0.0,
                    seg_len,
                );
                anchor["children"] = Value::Array(deeper);
                let mut rot = json_node(
                    ids[i].rot,
                    &format!("{preset_name}Rot{}", i + 1),
                    "Node",
                    0.0,
                    0.0,
                );
                rot["children"] = Value::Array(vec![anchor]);
                kids.push(rot);
            }
            cursor_children = Some(kids);
        }
        {
            let attach = navigate(&mut tree, &attach_path);
            let mut attach_kids = attach["children"].as_array().cloned().unwrap_or_default();
            attach_kids.extend(cursor_children.take().unwrap_or_default());
            attach["children"] = Value::Array(attach_kids);
        }

        // Driven parameters bind their stage's rotation
        // node, or the tip segment part for the last stage.
        let mut params_arr = payload["param"].as_array().cloned().unwrap_or_default();
        for (i, output) in outputs.iter().enumerate() {
            let target = if i + 1 < stages.len() {
                ids[i].rot
            } else {
                ids[i].part
            };
            params_arr.push(driven_param(
                param_uuids[i],
                &get_str(output, "parameter"),
                target,
            ));
        }
        payload["param"] = Value::Array(params_arr);
        applied.push(preset_name.clone());
    }

    payload["nodes"] = tree;
    let bytes = pack_inp(&payload, &[PLACEHOLDER_PNG.to_vec()]);
    std::fs::write(out_path, bytes)
        .map_err(|e| format!("could not write instantiated project: {e}"))?;

    Ok(json!({
        "nodes": count_nodes(&payload["nodes"]),
        "out": out_path,
        "parameters": payload["param"].as_array().map_or(0, Vec::len),
        "presets": applied,
        "template": template_name,
    }))
}
