//! Typed mutation ops: the only writes the session surface
//! accepts. The wire form is one JSON object with an `op` kind
//! field; parsing is strict — an undeclared field refuses.

use crate::refusal::{R, Refusal};
use serde::{Deserialize, Serialize};

/// A typed mutation op. The wire form is one JSON object with an `op` kind
/// field; parsing is strict — an undeclared field refuses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    ParamSet {
        param: String,
        value: f32,
    },
    NodeSetVisible {
        node: u32,
        visible: bool,
    },
    NodeSetTransform {
        node: u32,
        tx: f32,
        ty: f32,
        rot: f32,
        sx: f32,
        sy: f32,
    },
    SelectionSet {
        nodes: Vec<u32>,
    },
    ViewportSet {
        cam_x: f32,
        cam_y: f32,
        zoom: f32,
    },
}

impl Op {
    pub fn from_json(v: &serde_json::Value) -> R<Op> {
        let kind = v
            .get("op")
            .and_then(|k| k.as_str())
            .ok_or_else(|| Refusal::new("TG-AUTH-UNKNOWN-KIND", "op has no string `op` kind"))?
            .to_string();
        match kind.as_str() {
            "param_set" | "node_set_visible" | "node_set_transform" | "selection_set"
            | "viewport_set" => serde_json::from_value(v.clone())
                .map_err(|e| Refusal::new("TG-SES-UNKNOWN-FIELD", format!("op {kind}: {e}"))),
            other => Err(Refusal::new(
                "TG-AUTH-UNKNOWN-KIND",
                format!("op kind {other:?} is not in the registry"),
            )),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("op serialization is total")
    }

    /// The op's float payloads, for the ingress finiteness
    /// check (`TG-SES-NONFINITE`).
    pub(crate) fn floats(&self) -> Vec<f32> {
        match self {
            Op::ParamSet { value, .. } => vec![*value],
            Op::NodeSetTransform {
                tx,
                ty,
                rot,
                sx,
                sy,
                ..
            } => vec![*tx, *ty, *rot, *sx, *sy],
            Op::ViewportSet { cam_x, cam_y, zoom } => vec![*cam_x, *cam_y, *zoom],
            _ => vec![],
        }
    }
}
