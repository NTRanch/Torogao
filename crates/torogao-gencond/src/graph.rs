//! The compiled form: a `torogao-gen-graph/1` graph (S-1).
//!
//! A graph is a flat map of string node ids to `{class_type,
//! inputs}`; an input is a scalar or a link `[node-id,
//! output-index]`. The graph's identity is the SHA-256 of the
//! canonical serialization (S-1 §3): keys sorted at every
//! level, compact separators. This module's canonical form is
//! computed over `serde_json::Value` with the workspace's
//! sorted-key object model (BTreeMap-ordered; the workspace
//! never enables `preserve_order`), so it is engine-independent.

use crate::error::GencondResult;
use crate::limits::GRAPH_NODES_MAX;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// A scalar or link input value in a node.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum GraphInput {
    /// A link to another node's output: `[node-id, index]`.
    Link(String, u32),
    /// A JSON scalar (string, number, boolean).
    Scalar(Value),
}

impl GraphInput {
    /// Builds a float input. Non-finite values never reach
    /// here: records and plans are validated before emission.
    pub fn float(value: f64) -> GraphInput {
        match serde_json::Number::from_f64(value) {
            Some(number) => GraphInput::Scalar(Value::Number(number)),
            None => GraphInput::Scalar(Value::Null),
        }
    }
}

/// One graph node: a registered class (S-6) plus its wired
/// inputs, in emission (alphabetical) order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GraphNode {
    /// The node's class.
    pub class_type: String,
    /// Inputs by name.
    pub inputs: BTreeMap<String, GraphInput>,
}

/// A compiled `torogao-gen-graph/1` graph.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GenGraph {
    /// Nodes by string id, in id order.
    pub nodes: BTreeMap<String, GraphNode>,
}

impl GenGraph {
    /// Appends a node, returning its id. Ids are sequential
    /// decimal strings from 1: deterministic by construction.
    pub fn push(
        &mut self,
        class_type: &str,
        inputs: Vec<(&str, GraphInput)>,
    ) -> GencondResult<String> {
        if self.nodes.len() >= GRAPH_NODES_MAX {
            return crate::error::refuse(
                crate::error::CODE_RANGE,
                "compiled graph exceeds the node bound",
            );
        }
        let id = (self.nodes.len() + 1).to_string();
        let map: BTreeMap<String, GraphInput> = inputs
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        self.nodes.insert(
            id.clone(),
            GraphNode {
                class_type: class_type.to_string(),
                inputs: map,
            },
        );
        Ok(id)
    }

    /// The graph as a JSON value (the S-1 wire form).
    pub fn to_value(&self) -> Value {
        let mut root = Map::new();
        for (id, node) in &self.nodes {
            let mut inputs = Map::new();
            for (key, input) in &node.inputs {
                inputs.insert(key.clone(), input_to_value(input));
            }
            let mut node_object = Map::new();
            node_object.insert(
                "class_type".to_string(),
                Value::String(node.class_type.clone()),
            );
            node_object.insert("inputs".to_string(), Value::Object(inputs));
            root.insert(id.clone(), Value::Object(node_object));
        }
        Value::Object(root)
    }

    /// The canonical serialization (S-1 §3): sorted keys at
    /// every level, compact separators.
    pub fn canonical_text(&self) -> String {
        serde_json::to_string(&self.to_value()).expect("a graph value always serializes")
    }

    /// The graph's content hash (S-1 §3): SHA-256 of the
    /// canonical serialization, lowercase hex.
    pub fn sha256(&self) -> String {
        let digest = Sha256::digest(self.canonical_text().as_bytes());
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

fn input_to_value(input: &GraphInput) -> Value {
    match input {
        GraphInput::Link(id, index) => json!([id, index]),
        GraphInput::Scalar(value) => value.clone(),
    }
}

/// A link input to `node`'s output `index`.
pub fn link(node: &str, index: u32) -> GraphInput {
    GraphInput::Link(node.to_string(), index)
}

/// A string scalar input.
pub fn text(value: &str) -> GraphInput {
    GraphInput::Scalar(Value::String(value.to_string()))
}

/// An integer scalar input.
pub fn int(value: i64) -> GraphInput {
    GraphInput::Scalar(Value::Number(value.into()))
}

/// An unsigned integer scalar input.
pub fn uint(value: u64) -> GraphInput {
    GraphInput::Scalar(Value::Number(value.into()))
}
