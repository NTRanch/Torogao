//! The editor-state record (`torogao-editor-state/1`): the
//! session's snapshot of the shell's scene, selection, and
//! parameters. Carried over from the Slice-1 harness unchanged;
//! the schema is the record contract.

use crate::refusal::{R, Refusal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const STATE_FORMAT: &str = "torogao-editor-state/1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectMeta {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub tx: f32,
    pub ty: f32,
    pub rot: f32,
    pub sx: f32,
    pub sy: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Node,
    Part,
    SimplePhysics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: u32,
    pub name: String,
    pub parent: Option<u32>,
    pub kind: NodeKind,
    pub visible: bool,
    pub zsort: f32,
    pub transform: Transform,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub name: String,
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamState {
    pub value: f32,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub param: String,
    pub node: u32,
    pub property: String,
    pub kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub cam_x: f32,
    pub cam_y: f32,
    pub zoom: f32,
}

/// The editor-state record. `viewport` is session-local: it is part of the
/// live state but is never recorded and never enters the state hash
/// (DESIGN §4.2). `uncapturable` lists paths of state elements a host knows
/// it cannot canonically capture; compile refuses while it is non-empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorState {
    pub format: String,
    pub project: ProjectMeta,
    pub scene: Scene,
    pub selection: Vec<u32>,
    pub viewport: Viewport,
    pub params: BTreeMap<String, ParamState>,
    pub bindings: Vec<Binding>,
    pub uncapturable: Vec<String>,
}

impl EditorState {
    pub fn node(&self, id: u32) -> Option<&Node> {
        self.scene.nodes.iter().find(|n| n.id == id)
    }

    pub(crate) fn node_mut(&mut self, id: u32) -> Option<&mut Node> {
        self.scene.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Strict load-time validation. Any violation is a typed refusal; a
    /// state that cannot be validated is never partially loaded.
    pub fn validate(&self) -> R<()> {
        if self.format != STATE_FORMAT {
            return Err(Refusal::new(
                "TG-SES-UNKNOWN-FIELD",
                format!("format must be {STATE_FORMAT}, got {:?}", self.format),
            ));
        }
        let mut floats: Vec<(&str, f32)> = Vec::new();
        for n in &self.scene.nodes {
            floats.push(("node.zsort", n.zsort));
            floats.push(("transform.tx", n.transform.tx));
            floats.push(("transform.ty", n.transform.ty));
            floats.push(("transform.rot", n.transform.rot));
            floats.push(("transform.sx", n.transform.sx));
            floats.push(("transform.sy", n.transform.sy));
        }
        for (name, p) in &self.params {
            floats.push(("param.value", p.value));
            floats.push(("param.min", p.min));
            floats.push(("param.max", p.max));
            if p.min > p.max {
                return Err(Refusal::new(
                    "TG-AUTH-RANGE",
                    format!("param {name}: min {} > max {}", p.min, p.max),
                ));
            }
            if p.value < p.min || p.value > p.max {
                return Err(Refusal::new(
                    "TG-AUTH-RANGE",
                    format!(
                        "param {name}: value {} outside [{}, {}]",
                        p.value, p.min, p.max
                    ),
                ));
            }
        }
        floats.push(("viewport.cam_x", self.viewport.cam_x));
        floats.push(("viewport.cam_y", self.viewport.cam_y));
        floats.push(("viewport.zoom", self.viewport.zoom));
        for (what, v) in floats {
            if !v.is_finite() {
                return Err(Refusal::new(
                    "TG-SES-NONFINITE",
                    format!("{what} is not finite: {v}"),
                ));
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for n in &self.scene.nodes {
            if !ids.insert(n.id) {
                return Err(Refusal::new(
                    "TG-AUTH-CONFLICT",
                    format!("duplicate node id {}", n.id),
                ));
            }
        }
        for n in &self.scene.nodes {
            if let Some(p) = n.parent {
                if !ids.contains(&p) {
                    return Err(Refusal::new(
                        "TG-AUTH-DANGLING",
                        format!("node {} has missing parent {p}", n.id),
                    ));
                }
            }
        }
        for id in &self.selection {
            if !ids.contains(id) {
                return Err(Refusal::new(
                    "TG-AUTH-UNKNOWN-TARGET",
                    format!("selection names missing node {id}"),
                ));
            }
        }
        for b in &self.bindings {
            if !ids.contains(&b.node) {
                return Err(Refusal::new(
                    "TG-AUTH-DANGLING",
                    format!(
                        "binding for param {} names missing node {}",
                        b.param, b.node
                    ),
                ));
            }
            if !self.params.contains_key(&b.param) {
                return Err(Refusal::new(
                    "TG-AUTH-DANGLING",
                    format!("binding names missing param {}", b.param),
                ));
            }
        }
        Ok(())
    }

    /// Load a state from its JSON session-seed form (the harness fixture
    /// shape). Strict: unknown fields refuse `TG-SES-UNKNOWN-FIELD`.
    pub fn from_json(text: &str) -> R<EditorState> {
        let st: EditorState = serde_json::from_str(text).map_err(|e| {
            let code = if e.to_string().contains("unknown field") {
                "TG-SES-UNKNOWN-FIELD"
            } else {
                "TG-SES-SCHEMA"
            };
            Refusal::new(code, format!("state JSON: {e}"))
        })?;
        st.validate()?;
        Ok(st)
    }
}
