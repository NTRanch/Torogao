//! The project census (inspect_.d) and the payload-level
//! project-info document (projectinfo.d) — one
//! implementation behind both the CLI subcommands and
//! the registry tools, so the surfaces cannot drift.

use crate::util::f64_of_f32;
use crate::walk::{Walk, is_drawable, is_part, kind_name};
use serde_json::Value;
use torogao_engine::Puppet;
use torogao_engine::param::BindingValues;
use torogao_format::NodeKind;

fn jv2(x: f32, y: f32) -> Value {
    Value::Array(vec![f64_of_f32(x), f64_of_f32(y)])
}

/// Builds the structured census for a loaded puppet.
pub fn census(puppet: &Puppet, walk: &Walk) -> Value {
    let meta = serde_json::json!({
        "artist": puppet.meta.artist,
        "name": puppet.meta.name,
        "rigger": puppet.meta.rigger,
        "version": puppet.meta.version,
    });

    // -- nodes
    let mut node_list = Vec::new();
    let mut by_type: Vec<(String, usize)> = Vec::new();
    let mut part_count = 0usize;
    let mut drawable_count = 0usize;
    let mut deformer_count = 0usize;
    for w in &walk.nodes {
        let node = &puppet.nodes.nodes[w.node];
        let type_name = kind_name(node.kind);
        match by_type.iter_mut().find(|(name, _)| name == type_name) {
            Some(entry) => entry.1 += 1,
            None => by_type.push((type_name.to_string(), 1)),
        }
        if is_part(node.kind) {
            part_count += 1;
        }
        if is_drawable(node.kind) {
            drawable_count += 1;
        }
        if node.kind == NodeKind::MeshGroup {
            deformer_count += 1;
        }
        let parent_uuid = w.parent.map(|parent_walk| {
            puppet.nodes.nodes[walk.nodes[parent_walk].node].uuid
        });
        node_list.push(serde_json::json!({
            "children": w.children.len(),
            "enabled": node.enabled,
            "name": node.name,
            "parent": parent_uuid.map_or(Value::Null, Value::from),
            "type": type_name,
            "uuid": node.uuid,
            "zsort": (node.zsort + node.offset_sort) as f64,
        }));
    }
    let mut by_type_doc = serde_json::Map::new();
    for (name, count) in by_type {
        by_type_doc.insert(name, Value::from(count));
    }
    let nodes = serde_json::json!({
        "by_type": Value::Object(by_type_doc),
        "list": node_list,
        "total": walk.nodes.len(),
    });

    // -- parameters
    let mut param_list = Vec::new();
    let mut binding_total = 0usize;
    for param in &puppet.params {
        let mut bindings = Vec::new();
        for binding in &param.bindings {
            binding_total += 1;
            let (node_name, node_uuid) = match binding.node {
                Some(idx) => (
                    Value::String(puppet.nodes.nodes[idx].name.clone()),
                    puppet.nodes.nodes[idx].uuid,
                ),
                None => (Value::Null, binding.node_ref),
            };
            bindings.push(serde_json::json!({
                "kind": match binding.values {
                    BindingValues::Deform(_) => "deformation",
                    BindingValues::Scalar(_) => "value",
                },
                "node": node_uuid,
                "node_name": node_name,
                "param_name": binding.param_name,
                "set_count": binding.set_count(),
            }));
        }
        param_list.push(serde_json::json!({
            "axis_points": [param.axis_points[0].len(), param.axis_points[1].len()],
            "bindings": bindings,
            "defaults": jv2(param.defaults.x, param.defaults.y),
            "is_vec2": param.is_vec2,
            "max": jv2(param.max.x, param.max.y),
            "min": jv2(param.min.x, param.min.y),
            "name": param.name,
            "uuid": param.uuid,
        }));
    }
    let parameters = serde_json::json!({
        "bindings_total": binding_total,
        "list": param_list,
        "total": puppet.params.len(),
    });

    // -- textures
    let mut slot_list = Vec::new();
    let mut total_bytes = 0usize;
    for (index, slot) in puppet.texture_slots.iter().enumerate() {
        total_bytes += slot.bytes;
        slot_list.push(serde_json::json!({
            "bytes": slot.bytes,
            "format": slot.format,
            "index": index,
        }));
    }
    let textures = serde_json::json!({
        "list": slot_list,
        "slots": puppet.texture_slots.len(),
        "total_bytes": total_bytes,
    });

    // -- drivers / physics
    let mut driver_list = Vec::new();
    for driver_idx in &puppet.drivers {
        let node = &puppet.nodes.nodes[*driver_idx];
        let mut entry = serde_json::json!({
            "node": node.name,
            "type": kind_name(node.kind),
        });
        if let Some(state) = &node.physics {
            let drives = state
                .param
                .map(|param_idx| Value::String(puppet.params[param_idx].name.clone()))
                .unwrap_or(Value::Null);
            entry["drives"] = drives;
            entry["model"] = Value::String(if state.spring {
                "SpringPendulum".to_string()
            } else {
                "Pendulum".to_string()
            });
            entry["map_mode"] = Value::String(
                match state.map_mode {
                    1 => "XY",
                    2 => "LengthAngle",
                    3 => "YX",
                    _ => "AngleLength",
                }
                .to_string(),
            );
        }
        driver_list.push(entry);
    }
    let drivers = serde_json::json!({
        "list": driver_list,
        "total": puppet.drivers.len(),
    });

    // -- animations
    let mut anim_list = Vec::new();
    for (name, animation) in &puppet.animations {
        anim_list.push(serde_json::json!({
            "additive": animation.additive,
            "keyframes": animation.keyframe_count(),
            "lanes": animation.lanes.len(),
            "length": animation.length,
            "name": name,
            "timestep": animation.timestep as f64,
        }));
    }
    let animations = serde_json::json!({
        "list": anim_list,
        "total": puppet.animations.len(),
    });

    let ext_sections: Vec<Value> = puppet
        .ext_data
        .iter()
        .map(|(name, _)| Value::String(name.clone()))
        .collect();

    serde_json::json!({
        "animations": animations,
        "automation": puppet.automations.len(),
        "counts": {
            "drawables": drawable_count,
            "meshgroup_deformers": deformer_count,
            "parts": part_count,
        },
        "drivers": drivers,
        "ext_sections": ext_sections,
        "meta": meta,
        "nodes": nodes,
        "parameters": parameters,
        "textures": textures,
    })
}

/// The project-info document for a file. Reads only the
/// INP container (no puppet load). Errors on an unreadable
/// file or an overrunning payload length.
pub fn project_info_doc(project_path: &str) -> Result<Value, String> {
    let buffer = std::fs::read(project_path).map_err(|err| err.to_string())?;
    let mut doc = serde_json::json!({
        "file": project_path,
        "sizeBytes": buffer.len(),
    });
    const MAGIC: &[u8] = b"TRNSRTS\0";
    if buffer.len() < 12 || &buffer[0..8] != MAGIC {
        doc["container"] = Value::String("unknown".to_string());
        doc["payload"] = Value::Bool(false);
        return Ok(doc);
    }
    doc["container"] = Value::String("INP".to_string());

    let payload_len = u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]) as usize;
    if 12 + payload_len > buffer.len() {
        return Err("INP payload length overruns file".to_string());
    }
    let payload: Value =
        match serde_json::from_slice(&buffer[12..12 + payload_len]) {
            Ok(value) => value,
            Err(_) => {
                doc["payload"] = Value::Bool(false);
                return Ok(doc);
            }
        };
    doc["payload"] = Value::Bool(true);
    if payload.is_object() {
        if let Some(meta) = payload.get("meta") {
            doc["meta"] = meta.clone();
        }
        let mut keys: Vec<&String> = payload
            .as_object()
            .expect("object checked")
            .keys()
            .collect();
        keys.sort();
        doc["payloadKeys"] = Value::Array(
            keys.iter()
                .map(|key| Value::String((*key).clone()))
                .collect(),
        );
    }
    let mut counts = serde_json::Map::new();
    if let Some(Value::Array(anims)) = payload.get("animations") {
        counts.insert("animations".to_string(), Value::from(anims.len()));
    }
    match payload.get("physics") {
        Some(Value::Array(items)) => {
            counts.insert("physics".to_string(), Value::from(items.len()));
        }
        Some(Value::Object(map)) => {
            counts.insert("physics".to_string(), Value::from(map.len()));
        }
        _ => {}
    }
    if let Some(Value::Object(param)) = payload.get("param") {
        counts.insert(
            "parameterSections".to_string(),
            Value::from(param.len()),
        );
    }
    doc["counts"] = Value::Object(counts);

    // Texture trailer probe — faithful to the reference,
    // whose 8-byte slice compared against the 9-byte
    // "TEX_SECT\0" literal never matches: `present` is
    // always false and no count is ever reported.
    doc["textures"] = serde_json::json!({"present": false});
    Ok(doc)
}
