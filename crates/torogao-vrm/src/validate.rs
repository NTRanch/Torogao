//! Read-back validation: the crate's own reader re-parses an
//! exported VRM and censuses it — humanoid completeness,
//! expression binds, spring well-formedness, and (when a profile
//! is supplied) meta equal to the profile field-for-field.
//! Problems accumulate and are reported together, each named.

use serde_json::Value;

use crate::bones::{BONE_DEFS, torogao_for_vrm};
use crate::error::{VrmError, VrmResult};
use crate::export::VRM_SPEC_VERSION;
use crate::expressions::VRM_PRESET_NAMES;
use crate::glb::{Census, census, is_ancestor, mesh_target_count, node_parents, parse_glb};
use crate::profile::{VrmProfile, compare_meta};

/// The summary a successful validation returns.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidationReport {
    /// The file's structural census.
    pub census: Census,
    /// Custom expression names, sorted.
    pub custom_expressions: Vec<String>,
    /// Humanoid bones found, as VRM names, sorted.
    pub humanoid_bones: Vec<String>,
    /// Meta `name`, for display.
    pub meta_name: String,
    /// Preset expression names found, sorted.
    pub preset_expressions: Vec<String>,
    /// Spring chains found (0 when no spring block).
    pub spring_chains: usize,
}

/// Validate VRM bytes. With a profile, meta must equal it
/// exactly; without one, meta is checked only for the fields
/// the VRM spec itself requires.
pub fn validate_vrm(bytes: &[u8], profile: Option<&VrmProfile>) -> VrmResult<ValidationReport> {
    let glb = parse_glb(bytes)?;
    let mut problems: Vec<String> = Vec::new();
    let report_census = census(&glb);
    let parents = node_parents(&glb.json);
    let nodes_len = report_census.node_count;

    let extensions = glb.json.get("extensions");
    let vrm = extensions.and_then(|ext| ext.get("VRMC_vrm"));
    let Some(vrm) = vrm else {
        return Err(VrmError::ValidationFailed {
            problems: vec!["extensions.VRMC_vrm is absent".to_string()],
        });
    };
    if vrm.get("specVersion").and_then(Value::as_str) != Some(VRM_SPEC_VERSION) {
        problems.push("VRMC_vrm.specVersion is not \"1.0\"".to_string());
    }
    let used = glb
        .json
        .get("extensionsUsed")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !used.iter().any(|entry| entry.as_str() == Some("VRMC_vrm")) {
        problems.push("extensionsUsed does not list VRMC_vrm".to_string());
    }

    let meta_name = check_meta(vrm, profile, &mut problems);
    let humanoid_bones = check_humanoid(vrm, &glb.json, nodes_len, &mut problems);
    let (preset_expressions, custom_expressions) = check_expressions(vrm, &glb.json, &mut problems);
    let spring_chains = check_spring(extensions, &parents, nodes_len, &mut problems);

    if !problems.is_empty() {
        return Err(VrmError::ValidationFailed { problems });
    }
    Ok(ValidationReport {
        census: report_census,
        custom_expressions,
        humanoid_bones,
        meta_name,
        preset_expressions,
        spring_chains,
    })
}

/// Meta: profile equality when a profile is supplied, else the
/// spec-required fields. Returns the meta name for the report.
fn check_meta(vrm: &Value, profile: Option<&VrmProfile>, problems: &mut Vec<String>) -> String {
    let Some(meta) = vrm.get("meta") else {
        problems.push("VRMC_vrm.meta is absent".to_string());
        return String::new();
    };
    if let Some(profile) = profile {
        if let Err(err) = compare_meta(meta, profile) {
            problems.push(err.to_string());
        }
    } else {
        for field in ["name", "authors", "licenseUrl"] {
            if meta.get(field).is_none() {
                problems.push(format!("meta.{field} is absent (required by the VRM spec)"));
            }
        }
    }
    meta.get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Humanoid: required bones present, indices in range, and the
/// node at each index actually resolves to the claimed bone.
fn check_humanoid(
    vrm: &Value,
    json: &Value,
    nodes_len: usize,
    problems: &mut Vec<String>,
) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let bones = vrm
        .get("humanoid")
        .and_then(|humanoid| humanoid.get("humanBones"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if bones.is_empty() {
        problems.push("humanoid.humanBones is absent or empty".to_string());
        return found;
    }
    for def in BONE_DEFS.iter().filter(|def| def.required) {
        if !bones.contains_key(def.vrm) {
            problems.push(format!("required humanoid bone `{}` is absent", def.vrm));
        }
    }
    let nodes = json
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (vrm_name, entry) in &bones {
        found.push(vrm_name.clone());
        let Some(node) = entry.get("node").and_then(Value::as_u64) else {
            problems.push(format!("humanoid bone `{vrm_name}` carries no node index"));
            continue;
        };
        if node as usize >= nodes_len {
            problems.push(format!(
                "humanoid bone `{vrm_name}` node {node} is out of range"
            ));
            continue;
        }
        let node_name = nodes[node as usize]
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("");
        let resolved = crate::bones::resolve_source_node(node_name);
        if torogao_for_vrm(vrm_name).is_some() && resolved != torogao_for_vrm(vrm_name) {
            problems.push(format!(
                "humanoid bone `{vrm_name}` points at node {node} (`{node_name}`), which resolves to {resolved:?}"
            ));
        }
    }
    found.sort_unstable();
    found
}

/// Expressions: preset keys must be real preset names, and
/// every morph bind must address a real target.
fn check_expressions(
    vrm: &Value,
    json: &Value,
    problems: &mut Vec<String>,
) -> (Vec<String>, Vec<String>) {
    let mut preset_names: Vec<String> = Vec::new();
    let mut custom_names: Vec<String> = Vec::new();
    let Some(expressions) = vrm.get("expressions") else {
        return (preset_names, custom_names);
    };
    let meshes = json
        .get("meshes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let nodes = json
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (block, is_preset) in [("preset", true), ("custom", false)] {
        let Some(entries) = expressions.get(block).and_then(Value::as_object) else {
            continue;
        };
        for (name, expression) in entries {
            if is_preset {
                if !VRM_PRESET_NAMES.contains(&name.as_str()) {
                    problems.push(format!("`{name}` is not a VRM preset expression name"));
                }
                preset_names.push(name.clone());
            } else {
                custom_names.push(name.clone());
            }
            check_binds(name, expression, &nodes, &meshes, problems);
        }
    }
    preset_names.sort_unstable();
    custom_names.sort_unstable();
    (preset_names, custom_names)
}

/// The binds of one expression.
fn check_binds(
    name: &str,
    expression: &Value,
    nodes: &[Value],
    meshes: &[Value],
    problems: &mut Vec<String>,
) {
    let Some(binds) = expression.get("morphTargetBinds").and_then(Value::as_array) else {
        return;
    };
    for bind in binds {
        let node = bind.get("node").and_then(Value::as_u64);
        let index = bind.get("index").and_then(Value::as_u64);
        let (Some(node), Some(index)) = (node, index) else {
            problems.push(format!(
                "expression `{name}` carries a bind without node/index"
            ));
            continue;
        };
        let Some(node_value) = nodes.get(node as usize) else {
            problems.push(format!(
                "expression `{name}` bind node {node} is out of range"
            ));
            continue;
        };
        let Some(mesh_index) = node_value.get("mesh").and_then(Value::as_u64) else {
            problems.push(format!(
                "expression `{name}` bind node {node} carries no mesh"
            ));
            continue;
        };
        let Some(mesh) = meshes.get(mesh_index as usize) else {
            problems.push(format!(
                "expression `{name}` bind mesh {mesh_index} is out of range"
            ));
            continue;
        };
        if index as usize >= mesh_target_count(mesh) {
            problems.push(format!(
                "expression `{name}` bind index {index} exceeds the mesh's target count"
            ));
        }
        if let Some(weight) = bind.get("weight").and_then(Value::as_f64) {
            if !(0.0..=1.0).contains(&weight) {
                problems.push(format!(
                    "expression `{name}` bind weight {weight} outside 0.0..=1.0"
                ));
            }
        }
    }
}

/// Spring block: well-formedness against the same rules the
/// exporter enforces (indices in range, chain ancestry).
fn check_spring(
    extensions: Option<&Value>,
    parents: &[Option<usize>],
    nodes_len: usize,
    problems: &mut Vec<String>,
) -> usize {
    let Some(spring) = extensions.and_then(|ext| ext.get("VRMC_springBone")) else {
        return 0;
    };
    if spring.get("specVersion").and_then(Value::as_str) != Some(VRM_SPEC_VERSION) {
        problems.push("VRMC_springBone.specVersion is not \"1.0\"".to_string());
    }
    let colliders_len = spring
        .get("colliders")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if let Some(colliders) = spring.get("colliders").and_then(Value::as_array) {
        for (i, collider) in colliders.iter().enumerate() {
            check_node_ref(
                collider.get("node"),
                nodes_len,
                &format!("collider {i}"),
                problems,
            );
            if collider.get("shape").is_none() {
                problems.push(format!("collider {i} carries no shape"));
            }
        }
    }
    let groups_len = spring
        .get("colliderGroups")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if let Some(groups) = spring.get("colliderGroups").and_then(Value::as_array) {
        for (i, group) in groups.iter().enumerate() {
            if let Some(members) = group.get("colliders").and_then(Value::as_array) {
                for member in members {
                    if member.as_u64().is_some_and(|m| m as usize >= colliders_len) {
                        problems.push(format!(
                            "collider group {i} references collider {member} of {colliders_len}"
                        ));
                    }
                }
            }
        }
    }
    let springs = spring
        .get("springs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (chain_index, chain) in springs.iter().enumerate() {
        let joints = chain
            .get("joints")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut resolved: Vec<usize> = Vec::new();
        for (joint_index, joint) in joints.iter().enumerate() {
            let label = format!("spring {chain_index} joint {joint_index}");
            check_node_ref(joint.get("node"), nodes_len, &label, problems);
            if let Some(node) = joint.get("node").and_then(Value::as_u64) {
                resolved.push(node as usize);
            }
            for field in [
                "hitRadius",
                "stiffness",
                "gravityPower",
                "gravityDir",
                "dragForce",
            ] {
                if joint.get(field).is_none() {
                    problems.push(format!("{label} carries no {field}"));
                }
            }
        }
        for (joint_index, node) in resolved.iter().enumerate().skip(1) {
            let has_ancestor = resolved[..joint_index]
                .iter()
                .any(|earlier| is_ancestor(parents, *earlier, *node));
            if !has_ancestor {
                problems.push(format!(
                    "spring {chain_index} joint {joint_index} has no ancestor earlier in the chain"
                ));
            }
        }
        if let Some(groups) = chain.get("colliderGroups").and_then(Value::as_array) {
            for group in groups {
                if group.as_u64().is_some_and(|g| g as usize >= groups_len) {
                    problems.push(format!(
                        "spring {chain_index} references collider group {group} of {groups_len}"
                    ));
                }
            }
        }
    }
    springs.len()
}

/// A node reference must exist and be in range.
fn check_node_ref(node: Option<&Value>, nodes_len: usize, label: &str, problems: &mut Vec<String>) {
    match node.and_then(Value::as_u64) {
        Some(node) if (node as usize) < nodes_len => {}
        Some(node) => problems.push(format!("{label} node {node} is out of range")),
        None => problems.push(format!("{label} carries no node index")),
    }
}
