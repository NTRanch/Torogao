//! The payload document and its kind-tagged records.
//!
//! INP payloads are trees of records discriminated by a `type`
//! string: node records (`Node`, `Part`, `MeshGroup`,
//! `SimplePhysics`, and the composite kinds the later port
//! phases consume). Those records are decoded through ONE
//! site — [`decode_node_record`] — so the kind match lives in
//! exactly one place in the estate. A kind this port does not
//! know is a typed refusal naming the kind
//! (`TG-UNKNOWN-RECORD-KIND`), never a silent substitution;
//! records of known kinds keep their full raw document, so
//! decoding is lossless for every field the engine reads
//! later.

use serde_json::Value;

use crate::error::{FormatResult, TorogaoError};
use crate::value::{jget, jstr};

/// Stable code for a kind-tagged record whose kind is not
/// known to this port.
pub const TG_UNKNOWN_RECORD_KIND: &str = "TG-UNKNOWN-RECORD-KIND";

/// The kind of a node record, from its `type` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A plain transform node.
    Node,
    /// A drawable part.
    Part,
    /// A mesh-group (warp) node.
    MeshGroup,
    /// A simple-physics driver node.
    SimplePhysics,
    /// A dynamic composite node (decoded losslessly; the
    /// engine support lands with its port phase).
    DynamicComposite,
    /// A projectable node (decoded losslessly; the engine
    /// support lands with its port phase).
    Projectable,
}

impl NodeKind {
    /// The `type` string this kind carries in the payload.
    pub fn type_name(&self) -> &'static str {
        match self {
            NodeKind::Node => "Node",
            NodeKind::Part => "Part",
            NodeKind::MeshGroup => "MeshGroup",
            NodeKind::SimplePhysics => "SimplePhysics",
            NodeKind::DynamicComposite => "DynamicComposite",
            NodeKind::Projectable => "Projectable",
        }
    }
}

/// One decoded node record: its kind, identity, children, and
/// the full raw document (lossless — the engine reads its
/// kind-specific fields from `raw`).
#[derive(Debug, Clone)]
pub struct NodeRecord {
    /// The record's kind, decoded at the one site.
    pub kind: NodeKind,
    /// The node's uuid.
    pub uuid: u32,
    /// The node's name.
    pub name: String,
    /// Child records, in payload order.
    pub children: Vec<NodeRecord>,
    /// The complete raw record object.
    pub raw: Value,
}

/// THE kind-decode site for node records: the only `type`
/// match on node records in the crate. An unknown kind is a
/// typed refusal naming the kind; a missing or non-string
/// `type` is structural damage, reported as a malformed
/// container by the load boundary that wraps this call.
pub fn decode_node_record(raw: &Value) -> FormatResult<NodeRecord> {
    let kind_text = match jget(raw, "type") {
        Value::String(text) => text.clone(),
        _ => {
            return Err(TorogaoError::new(
                crate::container::TG_CONTAINER_MALFORMED,
                "project could not be loaded: malformed INP container \
                 (node record carries no type tag)",
            ));
        }
    };
    let kind = match kind_text.as_str() {
        "Node" => NodeKind::Node,
        "Part" => NodeKind::Part,
        "MeshGroup" => NodeKind::MeshGroup,
        "SimplePhysics" => NodeKind::SimplePhysics,
        "DynamicComposite" => NodeKind::DynamicComposite,
        "Projectable" => NodeKind::Projectable,
        other => {
            return Err(TorogaoError::new(
                TG_UNKNOWN_RECORD_KIND,
                format!("node record kind '{other}' is not known to this port"),
            ));
        }
    };
    let uuid = match jget(raw, "uuid") {
        Value::Number(number) => number.as_u64().unwrap_or(0) as u32,
        _ => 0,
    };
    let mut children = Vec::new();
    if let Value::Array(items) = jget(raw, "children") {
        for item in items {
            children.push(decode_node_record(item)?);
        }
    }
    Ok(NodeRecord {
        kind,
        uuid,
        name: jstr(raw, "name").to_string(),
        children,
        raw: raw.clone(),
    })
}

/// A decoded payload document: the node tree decoded through
/// the one kind site; the remaining sections carried as raw
/// documents for the engine and the census.
#[derive(Debug, Clone)]
pub struct PayloadDoc {
    /// The `meta` object (name, version, artist, rigger).
    pub meta: Value,
    /// The root node record.
    pub root: NodeRecord,
    /// The `param` array, raw, in payload order.
    pub params: Vec<Value>,
    /// The `physics` object (gravity, pixels per meter), or
    /// null when absent.
    pub physics: Value,
    /// The `animations` section, raw (array or name-keyed
    /// object, as authored).
    pub animations: Value,
    /// The `automation` array, raw, in payload order.
    pub automation: Vec<Value>,
    /// Any other top-level sections (extension data the core
    /// format carries opaquely), by key.
    pub extra: Vec<(String, Value)>,
}

/// Decodes a payload document from its parsed JSON value.
pub fn decode_payload(payload: &Value) -> FormatResult<PayloadDoc> {
    let root_raw = jget(payload, "nodes");
    if !root_raw.is_object() {
        return Err(TorogaoError::new(
            crate::container::TG_CONTAINER_MALFORMED,
            "project could not be loaded: malformed INP container \
             (payload carries no node tree)",
        ));
    }
    let root = decode_node_record(root_raw)?;
    let params = match jget(payload, "param") {
        Value::Array(items) => items.clone(),
        _ => Vec::new(),
    };
    let automation = match jget(payload, "automation") {
        Value::Array(items) => items.clone(),
        _ => Vec::new(),
    };
    let mut extra = Vec::new();
    if let Value::Object(map) = payload {
        for (key, value) in map {
            if !matches!(
                key.as_str(),
                "meta" | "nodes" | "param" | "physics" | "animations" | "automation"
            ) {
                extra.push((key.clone(), value.clone()));
            }
        }
    }
    Ok(PayloadDoc {
        meta: jget(payload, "meta").clone(),
        root,
        params,
        physics: jget(payload, "physics").clone(),
        animations: jget(payload, "animations").clone(),
        automation,
        extra,
    })
}

#[cfg(test)]
mod tests {
    use super::{NodeKind, TG_UNKNOWN_RECORD_KIND, decode_node_record, decode_payload};
    use serde_json::json;

    #[test]
    fn known_kinds_decode_through_the_one_site() {
        for (text, kind) in [
            ("Node", NodeKind::Node),
            ("Part", NodeKind::Part),
            ("MeshGroup", NodeKind::MeshGroup),
            ("SimplePhysics", NodeKind::SimplePhysics),
            ("DynamicComposite", NodeKind::DynamicComposite),
            ("Projectable", NodeKind::Projectable),
        ] {
            let record = decode_node_record(&json!({"type": text, "uuid": 7, "name": "n"}))
                .expect("known kind decodes");
            assert_eq!(record.kind, kind);
            assert_eq!(record.uuid, 7);
        }
    }

    #[test]
    fn unknown_kind_is_a_typed_refusal_naming_the_kind() {
        let err = decode_node_record(&json!({"type": "Welding", "uuid": 1, "name": "w"}))
            .expect_err("unknown kind refuses");
        assert_eq!(err.code(), TG_UNKNOWN_RECORD_KIND);
        assert!(
            err.message().contains("Welding"),
            "message: {}",
            err.message()
        );
    }

    #[test]
    fn payload_decodes_tree_and_sections() {
        let payload = json!({
            "meta": {"name": "T"},
            "nodes": {"type": "Node", "uuid": 1, "name": "Root",
                      "children": [{"type": "Part", "uuid": 2, "name": "Face"}]},
            "param": [{"name": "Angle"}],
            "physics": {"gravity": 9.8},
            "vendor-x": {"a": 1}
        });
        let doc = decode_payload(&payload).expect("payload decodes");
        assert_eq!(doc.root.children.len(), 1);
        assert_eq!(doc.root.children[0].kind, NodeKind::Part);
        assert_eq!(doc.params.len(), 1);
        assert_eq!(doc.extra.len(), 1);
        assert_eq!(doc.extra[0].0, "vendor-x");
    }
}
