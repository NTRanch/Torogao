//! The v1 structural criteria (criteria.d): one mechanism
//! behind validate, the export gate, and tutorials.

use crate::util::jget;
use crate::walk::{Walk, is_drawable, is_part};
use serde_json::Value;
use std::collections::HashSet;
use torogao_engine::Puppet;

/// TG-BINDING-MISSING-NODE.
pub const TG_BINDING_NODE: &str = "TG-BINDING-MISSING-NODE";
/// TG-BINDING-MISSING-PARAM.
pub const TG_BINDING_PARAM: &str = "TG-BINDING-MISSING-PARAM";
/// TG-DUPLICATE-NAME.
pub const TG_DUPLICATE_NAME: &str = "TG-DUPLICATE-NAME";
/// TG-EMPTY-PART.
pub const TG_EMPTY_PART: &str = "TG-EMPTY-PART";
/// TG-MISSING-TEXTURE.
pub const TG_MISSING_TEXTURE: &str = "TG-MISSING-TEXTURE";
/// TG-PARAM-NO-BINDINGS.
pub const TG_PARAM_NO_BINDINGS: &str = "TG-PARAM-NO-BINDINGS";

/// One validation finding.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// Stable finding code (TG- prefix).
    pub code: &'static str,
    /// "error" or "warning".
    pub severity: &'static str,
    /// Node path or parameter name.
    pub subject: String,
    /// Human-readable message.
    pub message: String,
}

impl Finding {
    /// Constructs a finding.
    pub fn new(
        code: &'static str,
        severity: &'static str,
        subject: impl Into<String>,
        message: impl Into<String>,
    ) -> Finding {
        Finding {
            code,
            severity,
            subject: subject.into(),
            message: message.into(),
        }
    }

    /// The JSON form (keys emit alphabetically).
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "code": self.code,
            "message": self.message,
            "severity": self.severity,
            "subject": self.subject,
        })
    }
}

/// Findings as a JSON array.
pub fn findings_json(findings: &[Finding]) -> Value {
    Value::Array(findings.iter().map(Finding::to_json).collect())
}

/// Whether any finding is error-severity.
pub fn any_error(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.severity == "error")
}

/// Runs the v1 criteria over a loaded puppet. The raw
/// payload is required for one check: the loader silently
/// drops bindings whose target node does not exist, so a
/// dangling binding is only observable in the payload.
pub fn validate_puppet(
    puppet: &Puppet,
    walk: &Walk,
    raw_payload: &Value,
    texture_slot_count: usize,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    // -- TG-EMPTY-PART + TG-MISSING-TEXTURE (per drawable)
    for (walk_idx, w) in walk.nodes.iter().enumerate() {
        let node = &puppet.nodes.nodes[w.node];
        if !is_drawable(node.kind) {
            continue;
        }
        let (vertex_count, index_count) = match &node.mesh {
            Some(mesh) => (mesh.vertices.len(), mesh.indices.len()),
            None => (0, 0),
        };
        if vertex_count == 0 || index_count == 0 {
            findings.push(Finding::new(
                TG_EMPTY_PART,
                "error",
                walk.path_of(puppet, walk_idx),
                format!(
                    "{} has no geometry ({vertex_count} vertices, {index_count} indices); it renders nothing",
                    crate::walk::kind_name(node.kind)
                ),
            ));
        }
        if is_part(node.kind) {
            if node.texture_ids.is_empty() {
                findings.push(Finding::new(
                    TG_MISSING_TEXTURE,
                    "warning",
                    walk.path_of(puppet, walk_idx),
                    "part references no texture slot",
                ));
            }
            for slot in &node.texture_ids {
                if *slot >= 0 && (*slot as usize) >= texture_slot_count {
                    findings.push(Finding::new(
                        TG_MISSING_TEXTURE,
                        "error",
                        walk.path_of(puppet, walk_idx),
                        format!(
                            "part references texture slot {slot} but the project embeds only {texture_slot_count} slot(s)"
                        ),
                    ));
                }
            }
        }
    }

    // -- Parameters
    let mut seen_param_names: HashSet<&str> = HashSet::new();
    for (param_idx, param) in puppet.params.iter().enumerate() {
        if !seen_param_names.insert(param.name.as_str()) {
            findings.push(Finding::new(
                TG_DUPLICATE_NAME,
                "error",
                param.name.clone(),
                "duplicate parameter name in the parameter list",
            ));
        }
        let is_driven = puppet.driven.contains_key(&param_idx);
        if param.bindings.is_empty() && !is_driven {
            findings.push(Finding::new(
                TG_PARAM_NO_BINDINGS,
                "warning",
                param.name.clone(),
                "parameter has no bindings and no driver; it cannot affect the puppet",
            ));
        }
        for binding in &param.bindings {
            let Some(target_idx) = binding.node else {
                findings.push(Finding::new(
                    TG_BINDING_NODE,
                    "error",
                    param.name.clone(),
                    format!(
                        "binding to node uuid {} did not resolve to a node",
                        binding.node_ref
                    ),
                ));
                continue;
            };
            let target = &puppet.nodes.nodes[target_idx];
            if binding.param_name == "deform" {
                if !is_drawable(target.kind) {
                    findings.push(Finding::new(
                        TG_BINDING_PARAM,
                        "error",
                        param.name.clone(),
                        format!(
                            "deformation binding targets '{}', which is not a drawable",
                            target.name
                        ),
                    ));
                }
            } else if !torogao_engine::param::node_has_param(
                &puppet.nodes,
                target_idx,
                &binding.param_name,
            ) {
                findings.push(Finding::new(
                    TG_BINDING_PARAM,
                    "error",
                    param.name.clone(),
                    format!(
                        "binding targets property '{}', which node '{}' does not provide",
                        binding.param_name, target.name
                    ),
                ));
            }
        }
    }

    // -- TG-DUPLICATE-NAME among siblings
    for (walk_idx, w) in walk.nodes.iter().enumerate() {
        let mut seen: HashSet<&str> = HashSet::new();
        for child_walk in &w.children {
            let child = &puppet.nodes.nodes[walk.nodes[*child_walk].node];
            if !seen.insert(child.name.as_str()) {
                findings.push(Finding::new(
                    TG_DUPLICATE_NAME,
                    "error",
                    walk.path_of(puppet, *child_walk),
                    format!(
                        "duplicate node name '{}' under '{}'",
                        child.name,
                        walk.path_of(puppet, walk_idx)
                    ),
                ));
            }
        }
    }

    // -- TG-BINDING-MISSING-NODE in the raw payload
    if raw_payload.is_object() {
        let mut node_uuids: HashSet<u32> = HashSet::new();
        collect_node_uuids(jget(raw_payload, "nodes"), &mut node_uuids);
        if let Value::Array(params) = jget(raw_payload, "param") {
            for param_value in params {
                let pname = match jget(param_value, "name") {
                    Value::String(name) => name.clone(),
                    _ => "<unnamed parameter>".to_string(),
                };
                if let Value::Array(bindings) = jget(param_value, "bindings") {
                    for binding_value in bindings {
                        let node_ref = jget(binding_value, "node");
                        let Some(uuid) = integer_of(node_ref) else {
                            continue;
                        };
                        if !node_uuids.contains(&uuid) {
                            findings.push(Finding::new(
                                TG_BINDING_NODE,
                                "error",
                                pname.clone(),
                                format!(
                                    "payload binding references node uuid {uuid}, which is not in the node tree (the loader drops it silently)"
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    findings
}

/// The integer value of a JSON value when it is an integer
/// (never a float), mirroring the reference's type test.
fn integer_of(value: &Value) -> Option<u32> {
    match value {
        Value::Number(n) if n.is_i64() || n.is_u64() => Some(n.as_u64()? as u32),
        _ => None,
    }
}

fn collect_node_uuids(node: &Value, acc: &mut HashSet<u32>) {
    if !node.is_object() {
        return;
    }
    if let Some(uuid) = integer_of(jget(node, "uuid")) {
        acc.insert(uuid);
    }
    if let Value::Array(children) = jget(node, "children") {
        for child in children {
            collect_node_uuids(child, acc);
        }
    }
}
