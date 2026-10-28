//! The write gate: every authoring op is decided by
//! `torogao-author`'s AUTH-D2 `decide()` — the real gate, not
//! a local re-implementation.
//!
//! The session holds a mirror of the live rig as an authoring
//! `Project`: parts carry the scene nodes' ids and visibility,
//! parameters carry the rig's declared ranges and defaults,
//! and the neutral pose carries the values the session has
//! posed away from default. An interop op is translated to the
//! authoring verb that owns the same question, `decide()` runs
//! against the mirror, and an admitted op is then stepped into
//! the mirror through `apply::step` — so the mirror and the
//! session state can never drift apart on an admitted op.
//!
//! Routing, per op kind:
//!
//! - `param_set` → `param_pose_default_set` (existence, range,
//!   and the Apply/NoOp verdict are the author's).
//! - `node_set_visible` → `part_visibility_set` (existence and
//!   verdict are the author's).
//! - `node_set_transform` → refused `TG-AUTH-UNKNOWN-KIND`.
//!   The authoring manifest has no verb that sets a node's
//!   local transform — node transforms are rig data in the
//!   authoring model, not pose — so the op is refused by name,
//!   the manifest's own discipline for unimplemented verbs,
//!   never silently ignored or locally approximated.
//! - `selection_set` / `viewport_set` → no authoring verb owns
//!   them; they are session-local by the design's own AUTH
//!   mapping, and the session layer validates them (duplicate
//!   selection → `TG-AUTH-CONFLICT`, missing node →
//!   `TG-AUTH-UNKNOWN-TARGET`).
//!
//! The mirror is a gate, never a record: it is not serialised,
//! and its header carries no stamp or consent — the two routed
//! verbs never consult the header, and inventing a consent
//! record for a real character would be worse than an empty
//! one.

use crate::op::Op;
use crate::refusal::{R, Refusal};
use crate::state::EditorState;
use std::collections::{BTreeMap, BTreeSet};
use torogao_author::decide::{Observed, Verdict};
use torogao_author::model::{Header, Parameter, Part, PoseEntry, Project};
use torogao_author::op::{PoseInput, VisibilityInput};

/// What the gate concluded about an op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    /// `decide()` admitted the op and it changes the mirror.
    Apply,
    /// `decide()` admitted the op; it changes nothing.
    NoOp,
    /// No authoring verb owns this op kind; the session layer
    /// validates it under the session-local rules.
    SessionLocal,
}

/// The authoring-project mirror the gate decides against.
#[derive(Debug, Clone)]
pub struct GateMirror {
    project: Project,
    part_ids: BTreeMap<u32, String>,
    param_ids: BTreeMap<String, String>,
}

/// Derives a valid authoring id for `name`: the name itself
/// when it already is one and is still free, else a sanitised
/// form with a deterministic suffix. Authoring ids are
/// `[A-Za-z0-9_-]{1,64}` bytes (`decide::check_id`).
fn author_id(name: &str, seed: &str, taken: &mut BTreeSet<String>) -> String {
    if torogao_author::decide::check_id(name, "id").is_ok() && !taken.contains(name) {
        taken.insert(name.to_string());
        return name.to_string();
    }
    // The sanitised base is pure ASCII, so byte lengths are
    // char lengths and slicing at a byte offset is safe.
    let mut base: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if base.is_empty() {
        base.push('x');
    }
    let mut suffix = format!("-{seed}");
    let mut n = 2;
    loop {
        let keep = 64usize.saturating_sub(suffix.len()).max(1);
        let candidate = format!("{}{suffix}", &base[..keep.min(base.len())]);
        if !taken.contains(&candidate) {
            taken.insert(candidate.clone());
            return candidate;
        }
        suffix = format!("-{seed}-{n}");
        n += 1;
    }
}

impl GateMirror {
    /// Builds the mirror from a genesis state. `param_defaults`
    /// gives each projected parameter's rig-declared default
    /// (the authoring model's `default`); parameters posed
    /// away from their default in the genesis state enter the
    /// mirror's neutral pose.
    pub fn build(genesis: &EditorState, param_defaults: &BTreeMap<String, f32>) -> GateMirror {
        let mut taken: BTreeSet<String> = BTreeSet::new();
        let mut part_ids: BTreeMap<u32, String> = BTreeMap::new();
        let mut nodes: Vec<_> = genesis.scene.nodes.iter().collect();
        nodes.sort_by_key(|n| n.id);
        for node in &nodes {
            let id = author_id(&node.name, &node.id.to_string(), &mut taken);
            part_ids.insert(node.id, id);
        }
        let mut param_ids: BTreeMap<String, String> = BTreeMap::new();
        for (index, name) in genesis.params.keys().enumerate() {
            let id = author_id(name, &format!("p{index}"), &mut taken);
            param_ids.insert(name.clone(), id);
        }
        let parts: Vec<Part> = nodes
            .iter()
            .map(|node| Part {
                id: part_ids[&node.id].clone(),
                parent: node.parent.map(|p| part_ids[&p].clone()),
                deformer: None,
                art: None,
                mesh: None,
                clip: None,
                draw: None,
                visible: node.visible,
                completion: None,
                keyforms: None,
            })
            .collect();
        let parameters: Vec<Parameter> = genesis
            .params
            .iter()
            .map(|(name, p)| Parameter {
                id: param_ids[name].clone(),
                class: "live-rig".to_string(),
                min: f64::from(p.min),
                default: f64::from(param_defaults[name]),
                max: f64::from(p.max),
            })
            .collect();
        let mut pose: Vec<PoseEntry> = genesis
            .params
            .iter()
            .filter(|(name, p)| p.value != param_defaults[*name])
            .map(|(name, p)| PoseEntry {
                parameter: param_ids[name].clone(),
                value: f64::from(p.value),
            })
            .collect();
        pose.sort_by(|a, b| a.parameter.cmp(&b.parameter));
        let mut project = Project {
            header: Header {
                name: author_id(&genesis.project.name, "project", &mut taken),
                model_format: "torogao-rig-model".to_string(),
                ..Header::default()
            },
            parameters,
            parts,
            pose,
            ..Project::default()
        };
        // The authoring model keeps every collection id-sorted;
        // its lookups are binary searches.
        project.parameters.sort_by(|a, b| a.id.cmp(&b.id));
        project.parts.sort_by(|a, b| a.id.cmp(&b.id));
        GateMirror {
            project,
            part_ids,
            param_ids,
        }
    }

    /// Translates an interop op to its authoring verb.
    /// Session-local kinds return `None`; `node_set_transform`
    /// is refused here by name (see the module docs).
    fn author_op(&self, op: &Op) -> R<Option<torogao_author::op::Op>> {
        match op {
            Op::ParamSet { param, value } => {
                let id = self.param_ids.get(param).ok_or_else(|| {
                    Refusal::new("TG-AUTH-UNKNOWN-TARGET", format!("no param named {param}"))
                })?;
                Ok(Some(torogao_author::op::Op::ParamPoseDefaultSet(
                    PoseInput {
                        parameter: id.clone(),
                        value: f64::from(*value),
                    },
                )))
            }
            Op::NodeSetVisible { node, visible } => {
                let id = self.part_ids.get(node).ok_or_else(|| {
                    Refusal::new("TG-AUTH-UNKNOWN-TARGET", format!("no node with id {node}"))
                })?;
                Ok(Some(torogao_author::op::Op::PartVisibilitySet(
                    VisibilityInput {
                        part: id.clone(),
                        visible: *visible,
                    },
                )))
            }
            Op::NodeSetTransform { .. } => Err(Refusal::new(
                "TG-AUTH-UNKNOWN-KIND",
                "node_set_transform has no AUTH-D2 verb: the authoring manifest \
                 carries no node-transform setter (node transforms are rig data, \
                 not pose), so the op is refused by name, not approximated",
            )),
            Op::SelectionSet { .. } | Op::ViewportSet { .. } => Ok(None),
        }
    }

    /// Runs the real `decide()` for an op. A refusal from the
    /// authoring gate passes through with its `TG-AUTH-*` code
    /// and message unchanged.
    pub fn decide(&self, op: &Op) -> R<GateVerdict> {
        let Some(author_op) = self.author_op(op)? else {
            return Ok(GateVerdict::SessionLocal);
        };
        match torogao_author::decide::decide(Some(&self.project), &author_op, &Observed::Nothing) {
            Ok(Verdict::Apply) => Ok(GateVerdict::Apply),
            Ok(Verdict::NoOp) => Ok(GateVerdict::NoOp),
            Err(r) => Err(Refusal::new(r.code(), r.message.clone())),
        }
    }

    /// Steps an admitted op into the mirror through the
    /// author's own `apply::step`, keeping the mirror in
    /// lockstep with the session state. Session-local ops have
    /// no mirror footprint. A refusal here is impossible for an
    /// op `decide` just admitted; it is reported as a chain
    /// break, not retried or overridden.
    pub fn commit(&mut self, op: &Op) -> R<()> {
        let Some(author_op) = self.author_op(op)? else {
            return Ok(());
        };
        let project = std::mem::take(&mut self.project);
        match torogao_author::apply::step(Some(project), &author_op, &Observed::Nothing) {
            Ok(stepped) => {
                self.project = stepped.project;
                Ok(())
            }
            Err(r) => Err(Refusal::new(
                "TG-SES-LOG-CHAIN",
                format!(
                    "gate mirror refused an admitted op at commit ({}: {})",
                    r.code(),
                    r.message
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Binding, STATE_FORMAT};
    use crate::state::{Node, NodeKind, ParamState, ProjectMeta, Scene, Transform, Viewport};
    use std::collections::BTreeMap;

    fn genesis() -> (EditorState, BTreeMap<String, f32>) {
        let mut params = BTreeMap::new();
        params.insert(
            "ParamAngleX".to_string(),
            ParamState {
                value: 0.0,
                min: -30.0,
                max: 30.0,
            },
        );
        let mut defaults = BTreeMap::new();
        defaults.insert("ParamAngleX".to_string(), 0.0);
        (
            EditorState {
                format: STATE_FORMAT.to_string(),
                project: ProjectMeta {
                    name: "TestRig".to_string(),
                    source: "unit".to_string(),
                },
                scene: Scene {
                    name: "TestRig".to_string(),
                    nodes: vec![
                        Node {
                            id: 10,
                            name: "Root".to_string(),
                            parent: None,
                            kind: NodeKind::Node,
                            visible: true,
                            zsort: 0.0,
                            transform: Transform {
                                tx: 0.0,
                                ty: 0.0,
                                rot: 0.0,
                                sx: 1.0,
                                sy: 1.0,
                            },
                        },
                        Node {
                            id: 20,
                            name: "Face".to_string(),
                            parent: Some(10),
                            kind: NodeKind::Part,
                            visible: true,
                            zsort: 1.0,
                            transform: Transform {
                                tx: 0.0,
                                ty: 0.0,
                                rot: 0.0,
                                sx: 1.0,
                                sy: 1.0,
                            },
                        },
                    ],
                },
                selection: vec![],
                viewport: Viewport {
                    cam_x: 0.0,
                    cam_y: 0.0,
                    zoom: 1.0,
                },
                params,
                bindings: vec![Binding {
                    param: "ParamAngleX".to_string(),
                    node: 20,
                    property: "angle".to_string(),
                    kind: "value".to_string(),
                }],
                uncapturable: vec![],
            },
            defaults,
        )
    }

    #[test]
    fn out_of_range_param_is_refused_by_the_real_decide() {
        let (st, defaults) = genesis();
        let gate = GateMirror::build(&st, &defaults);
        let err = gate
            .decide(&Op::ParamSet {
                param: "ParamAngleX".to_string(),
                value: 45.0,
            })
            .unwrap_err();
        assert_eq!(err.code, "TG-AUTH-RANGE");
        assert!(err.message.contains("is outside parameter"));
    }

    #[test]
    fn in_range_param_decides_apply_then_noop_after_commit() {
        let (st, defaults) = genesis();
        let mut gate = GateMirror::build(&st, &defaults);
        let op = Op::ParamSet {
            param: "ParamAngleX".to_string(),
            value: 12.5,
        };
        assert_eq!(gate.decide(&op), Ok(GateVerdict::Apply));
        gate.commit(&op).unwrap();
        // The author's verdict flips to NoOp once the pose
        // already holds the value — evidence the mirror moved.
        assert_eq!(gate.decide(&op), Ok(GateVerdict::NoOp));
    }

    #[test]
    fn unknown_param_and_node_are_refused_unknown_target() {
        let (st, defaults) = genesis();
        let gate = GateMirror::build(&st, &defaults);
        let err = gate
            .decide(&Op::ParamSet {
                param: "ParamNope".to_string(),
                value: 1.0,
            })
            .unwrap_err();
        assert_eq!(err.code, "TG-AUTH-UNKNOWN-TARGET");
        let err = gate
            .decide(&Op::NodeSetVisible {
                node: 999,
                visible: false,
            })
            .unwrap_err();
        assert_eq!(err.code, "TG-AUTH-UNKNOWN-TARGET");
    }

    #[test]
    fn node_transform_is_refused_by_name() {
        let (st, defaults) = genesis();
        let gate = GateMirror::build(&st, &defaults);
        let err = gate
            .decide(&Op::NodeSetTransform {
                node: 20,
                tx: 1.0,
                ty: 2.0,
                rot: 0.0,
                sx: 1.0,
                sy: 1.0,
            })
            .unwrap_err();
        assert_eq!(err.code, "TG-AUTH-UNKNOWN-KIND");
        assert!(err.message.contains("no AUTH-D2 verb"));
    }

    #[test]
    fn visibility_decides_through_the_author() {
        let (st, defaults) = genesis();
        let mut gate = GateMirror::build(&st, &defaults);
        let op = Op::NodeSetVisible {
            node: 20,
            visible: false,
        };
        assert_eq!(gate.decide(&op), Ok(GateVerdict::Apply));
        gate.commit(&op).unwrap();
        assert_eq!(gate.decide(&op), Ok(GateVerdict::NoOp));
    }

    #[test]
    fn session_local_kinds_are_named_not_decided() {
        let (st, defaults) = genesis();
        let gate = GateMirror::build(&st, &defaults);
        assert_eq!(
            gate.decide(&Op::SelectionSet { nodes: vec![20] }),
            Ok(GateVerdict::SessionLocal)
        );
        assert_eq!(
            gate.decide(&Op::ViewportSet {
                cam_x: 0.0,
                cam_y: 0.0,
                zoom: 1.0
            }),
            Ok(GateVerdict::SessionLocal)
        );
    }

    #[test]
    fn names_that_are_not_authoring_ids_get_deterministic_ids() {
        let (mut st, mut defaults) = genesis();
        st.params.insert(
            "Head:: Yaw-Pitch".to_string(),
            ParamState {
                value: 0.0,
                min: -1.0,
                max: 1.0,
            },
        );
        defaults.insert("Head:: Yaw-Pitch".to_string(), 0.0);
        let gate = GateMirror::build(&st, &defaults);
        // The op still decides through the author under the
        // derived id.
        assert_eq!(
            gate.decide(&Op::ParamSet {
                param: "Head:: Yaw-Pitch".to_string(),
                value: 0.5,
            }),
            Ok(GateVerdict::Apply)
        );
        let err = gate
            .decide(&Op::ParamSet {
                param: "Head:: Yaw-Pitch".to_string(),
                value: 2.0,
            })
            .unwrap_err();
        assert_eq!(err.code, "TG-AUTH-RANGE");
    }
}
