//! `torogao-vrm-spring/1`: the spring-chain input record, and
//! its emission as `VRMC_springBone`.
//!
//! Spring data is an input to export, never something the
//! exporter invents: when no record is supplied, the export
//! carries no `VRMC_springBone` extension at all. Every joint
//! parameter is a required field of the record — the VRM spec's
//! defaults (stiffness 1.0, dragForce 0.5, and kin) are exactly
//! the kind of silent default this crate refuses to lean on.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::bones::{is_torogao_bone_name, is_vrm_bone_name, resolve_source_node};
use crate::error::{VrmError, VrmResult};
use crate::glb::is_ancestor;

/// The spring record's format token, carried in its `id` field.
pub const SPRING_ID: &str = "torogao-vrm-spring/1";

/// Largest spring document this crate will parse, in bytes.
pub const SPRING_BYTES_MAX: usize = 65_536;

/// A collider shape, sphere or capsule (the spec's `oneOf`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub enum RawShape {
    /// A capsule: head offset, radius, tail offset.
    Capsule {
        /// Head center, in node space.
        offset: [f32; 3],
        /// Radius in metres.
        radius: f32,
        /// Tail center, in node space.
        tail: [f32; 3],
    },
    /// A sphere: center offset and radius.
    Sphere {
        /// Center, in node space.
        offset: [f32; 3],
        /// Radius in metres.
        radius: f32,
    },
}

/// One collider: a node plus its shape. Both fields required.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RawCollider {
    /// The node name the collider rides on (torogao or source
    /// rig name).
    pub node: Option<String>,
    /// The collider shape.
    pub shape: Option<RawShape>,
}

/// One collider group: a name plus indices into the record's
/// collider list.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RawColliderGroup {
    /// Indices into the record's `colliders` list.
    pub colliders: Option<Vec<usize>>,
    /// The group's name (optional in the spec; stated when the
    /// record carries one).
    pub name: Option<String>,
}

/// One spring joint. Every physical parameter is required.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RawJoint {
    /// Resistance to velocity (spec range 0.0..=1.0).
    pub drag_force: Option<f32>,
    /// Gravity direction, a 3-vector.
    pub gravity_dir: Option<[f32; 3]>,
    /// Gravitational acceleration scale (>= 0).
    pub gravity_power: Option<f32>,
    /// Collision sphere radius in metres (>= 0).
    pub hit_radius: Option<f32>,
    /// The bone name (torogao or source rig name).
    pub node: Option<String>,
    /// Return-to-pose force (>= 0).
    pub stiffness: Option<f32>,
}

/// One spring chain: an ordered joint list. Except for the
/// first joint, every joint must have an ancestor earlier in
/// the list (the spec's chain rule).
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RawChain {
    /// The center node name, when the chain declares one.
    pub center: Option<String>,
    /// Indices into the record's `collider_groups` list.
    pub collider_groups: Option<Vec<usize>>,
    /// The joints, root first.
    pub joints: Vec<RawJoint>,
    /// The chain's name.
    pub name: Option<String>,
}

/// The raw spring record.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RawSpring {
    /// The chains. At least one.
    pub chains: Vec<RawChain>,
    /// The colliders (may be absent when no chain uses any).
    pub colliders: Option<Vec<RawCollider>>,
    /// The collider groups (may be absent likewise).
    pub collider_groups: Option<Vec<RawColliderGroup>>,
    /// Format token; must equal [`SPRING_ID`].
    pub id: Option<String>,
}

/// A checked, node-resolved spring record, ready to emit.
#[derive(Clone, Debug, PartialEq)]
pub struct SpringSpec {
    /// The checked record.
    raw: RawSpring,
    /// Resolved collider node indices, parallel to `colliders`.
    collider_nodes: Vec<usize>,
    /// Resolved joint node indices, per chain.
    joint_nodes: Vec<Vec<usize>>,
    /// Resolved center node indices, per chain.
    center_nodes: Vec<Option<usize>>,
}

/// What the resolver needs from the model: name → node index,
/// and the node parent table for the chain rule.
pub struct NodeIndex<'a> {
    /// Node names in document order.
    pub names: &'a [String],
    /// Node parent table.
    pub parents: &'a [Option<usize>],
}

impl NodeIndex<'_> {
    /// Resolve a spring record name to a node index. The name is
    /// read in the torogao/source name spaces; a VRM-space name
    /// is called out as such, and any other miss is unresolved.
    pub fn resolve(&self, name: &str) -> VrmResult<usize> {
        if let Some(torogao) = resolve_source_node(name) {
            for (index, node_name) in self.names.iter().enumerate() {
                if resolve_source_node(node_name) == Some(torogao) {
                    return Ok(index);
                }
            }
            return Err(VrmError::UnresolvedNode {
                name: name.to_string(),
            });
        }
        if is_vrm_bone_name(name) && !is_torogao_bone_name(name) {
            return Err(VrmError::UnknownBoneName {
                name: name.to_string(),
            });
        }
        if let Some(index) = self.names.iter().position(|node_name| node_name == name) {
            return Ok(index);
        }
        Err(VrmError::UnresolvedNode {
            name: name.to_string(),
        })
    }
}

impl SpringSpec {
    /// Parse a spring record (RON). Bounded by
    /// [`SPRING_BYTES_MAX`].
    pub fn parse(text: &str) -> VrmResult<RawSpring> {
        if text.len() > SPRING_BYTES_MAX {
            return Err(VrmError::SpringParse {
                detail: format!(
                    "document is {} bytes, over the {SPRING_BYTES_MAX}-byte bound",
                    text.len()
                ),
            });
        }
        crate::profile::ron_options()
            .from_str(text)
            .map_err(|err| VrmError::SpringParse {
                detail: err.to_string(),
            })
    }

    /// Parse and resolve a spring record against a model.
    pub fn load(text: &str, index: &NodeIndex<'_>) -> VrmResult<SpringSpec> {
        let raw = Self::parse(text)?;
        Self::check(raw, index)
    }

    /// Resolve every name, check every required field and every
    /// spec range, and verify the chain ancestry rule.
    pub fn check(raw: RawSpring, index: &NodeIndex<'_>) -> VrmResult<SpringSpec> {
        if let Some(id) = &raw.id {
            if id != SPRING_ID {
                return Err(VrmError::SpringParse {
                    detail: format!("spring record id is `{id}`, not {SPRING_ID}"),
                });
            }
        } else {
            return Err(VrmError::SpringParse {
                detail: format!("spring record carries no id; expected {SPRING_ID}"),
            });
        }
        if raw.chains.is_empty() {
            return Err(VrmError::SpringParse {
                detail: "spring record carries no chains".to_string(),
            });
        }
        let collider_total = raw.colliders.as_ref().map_or(0, Vec::len);
        let mut collider_nodes = Vec::with_capacity(collider_total);
        for (position, collider) in raw.colliders.iter().flatten().enumerate() {
            let name = collider.node.clone().ok_or_else(|| VrmError::SpringParse {
                detail: format!("collider {position} carries no node"),
            })?;
            if collider.shape.is_none() {
                return Err(VrmError::SpringParse {
                    detail: format!("collider {position} carries no shape"),
                });
            }
            collider_nodes.push(index.resolve(&name)?);
        }
        let group_total = raw.collider_groups.as_ref().map_or(0, Vec::len);
        for (position, group) in raw.collider_groups.iter().flatten().enumerate() {
            let members = group
                .colliders
                .clone()
                .ok_or_else(|| VrmError::SpringParse {
                    detail: format!("collider group {position} carries no collider list"),
                })?;
            for member in members {
                if member >= collider_total {
                    return Err(VrmError::SpringParse {
                        detail: format!(
                            "collider group {position} references collider {member} of {collider_total}"
                        ),
                    });
                }
            }
        }
        let mut joint_nodes = Vec::with_capacity(raw.chains.len());
        let mut center_nodes = Vec::with_capacity(raw.chains.len());
        for (chain_index, chain) in raw.chains.iter().enumerate() {
            let chain_name = chain
                .name
                .clone()
                .unwrap_or_else(|| format!("chain {chain_index}"));
            let mut resolved = Vec::with_capacity(chain.joints.len());
            for (joint_index, joint) in chain.joints.iter().enumerate() {
                resolved.push(check_joint(joint, &chain_name, joint_index, index)?);
            }
            for (joint_index, node) in resolved.iter().enumerate().skip(1) {
                let has_ancestor = resolved[..joint_index]
                    .iter()
                    .any(|earlier| is_ancestor(index.parents, *earlier, *node));
                if !has_ancestor {
                    return Err(VrmError::SpringChainOrder {
                        bone: chain.joints[joint_index].node.clone().unwrap_or_default(),
                        chain: chain_name.clone(),
                    });
                }
            }
            if let Some(groups) = &chain.collider_groups {
                for group in groups {
                    if *group >= group_total {
                        return Err(VrmError::SpringParse {
                            detail: format!(
                                "chain `{chain_name}` references collider group {group} of {group_total}"
                            ),
                        });
                    }
                }
            }
            center_nodes.push(match &chain.center {
                Some(name) => Some(index.resolve(name)?),
                None => None,
            });
            joint_nodes.push(resolved);
        }
        Ok(SpringSpec {
            raw,
            collider_nodes,
            joint_nodes,
            center_nodes,
        })
    }

    /// Emit the `VRMC_springBone` extension value.
    pub fn to_json(&self) -> Value {
        let mut root = Map::new();
        root.insert("specVersion".to_string(), json!("1.0"));
        if !self.collider_nodes.is_empty() {
            let colliders: Vec<Value> = self
                .raw
                .colliders
                .iter()
                .flatten()
                .zip(self.collider_nodes.iter())
                .map(|(collider, node)| collider_json(collider, *node))
                .collect();
            root.insert("colliders".to_string(), Value::Array(colliders));
        }
        if let Some(groups) = &self.raw.collider_groups {
            if !groups.is_empty() {
                let values: Vec<Value> = groups
                    .iter()
                    .map(|group| {
                        let mut object = Map::new();
                        if let Some(name) = &group.name {
                            object.insert("name".to_string(), json!(name));
                        }
                        object.insert(
                            "colliders".to_string(),
                            json!(group.colliders.clone().unwrap_or_default()),
                        );
                        Value::Object(object)
                    })
                    .collect();
                root.insert("colliderGroups".to_string(), Value::Array(values));
            }
        }
        let springs: Vec<Value> = self
            .raw
            .chains
            .iter()
            .zip(self.joint_nodes.iter())
            .zip(self.center_nodes.iter())
            .map(|((chain, nodes), center)| chain_json(chain, nodes, *center))
            .collect();
        root.insert("springs".to_string(), Value::Array(springs));
        Value::Object(root)
    }
}

/// Check one joint's fields and ranges; return its node index.
fn check_joint(
    joint: &RawJoint,
    chain_name: &str,
    joint_index: usize,
    index: &NodeIndex<'_>,
) -> VrmResult<usize> {
    let owner = format!("chain `{chain_name}` joint {joint_index}");
    let name = joint.node.clone().ok_or_else(|| VrmError::SpringParse {
        detail: format!("{owner} carries no node"),
    })?;
    let node = index.resolve(&name).map_err(|err| match err {
        // A bone-vocabulary name the model does not carry is a
        // dangling joint; anything else keeps its own diagnosis.
        VrmError::UnresolvedNode { name: missing } if resolve_source_node(&missing).is_some() => {
            VrmError::DanglingSpringJoint {
                bone: missing,
                chain: chain_name.to_string(),
            }
        }
        other => other,
    })?;
    let required: [(&str, Option<f32>); 4] = [
        ("hit_radius", joint.hit_radius),
        ("stiffness", joint.stiffness),
        ("gravity_power", joint.gravity_power),
        ("drag_force", joint.drag_force),
    ];
    for (field, value) in required {
        let value = value.ok_or_else(|| VrmError::SpringParse {
            detail: format!("{owner} carries no {field}"),
        })?;
        let in_range = match field {
            "drag_force" => (0.0..=1.0).contains(&value),
            _ => value >= 0.0,
        };
        if !in_range || !value.is_finite() {
            return Err(VrmError::SpringValueOutOfRange {
                field: field.to_string(),
                owner,
                value: f64::from(value),
            });
        }
    }
    if joint.gravity_dir.is_none() {
        return Err(VrmError::SpringParse {
            detail: format!("{owner} carries no gravity_dir"),
        });
    }
    Ok(node)
}

/// One collider's extension JSON.
fn collider_json(collider: &RawCollider, node: usize) -> Value {
    let shape = collider.shape.expect("shape checked present");
    let shape_json = match shape {
        RawShape::Capsule {
            offset,
            radius,
            tail,
        } => json!({
            "capsule": {"offset": offset, "radius": radius, "tail": tail}
        }),
        RawShape::Sphere { offset, radius } => json!({
            "sphere": {"offset": offset, "radius": radius}
        }),
    };
    json!({"node": node, "shape": shape_json})
}

/// One chain's extension JSON, all joint parameters explicit.
fn chain_json(chain: &RawChain, nodes: &[usize], center: Option<usize>) -> Value {
    let mut object = Map::new();
    if let Some(name) = &chain.name {
        object.insert("name".to_string(), json!(name));
    }
    let joints: Vec<Value> = chain
        .joints
        .iter()
        .zip(nodes.iter())
        .map(|(joint, node)| {
            json!({
                "node": node,
                "hitRadius": joint.hit_radius.expect("checked present"),
                "stiffness": joint.stiffness.expect("checked present"),
                "gravityPower": joint.gravity_power.expect("checked present"),
                "gravityDir": joint.gravity_dir.expect("checked present"),
                "dragForce": joint.drag_force.expect("checked present"),
            })
        })
        .collect();
    object.insert("joints".to_string(), Value::Array(joints));
    if let Some(groups) = &chain.collider_groups {
        if !groups.is_empty() {
            object.insert("colliderGroups".to_string(), json!(groups));
        }
    }
    if let Some(center) = center {
        object.insert("center".to_string(), json!(center));
    }
    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use super::{NodeIndex, SpringSpec};
    use crate::error::VrmError;

    /// Three nodes in a line: 0 → 1 → 2.
    fn index() -> (Vec<String>, Vec<Option<usize>>) {
        (
            vec![
                "mixamorig:Hips".to_string(),
                "mixamorig:Spine".to_string(),
                "mixamorig:Head".to_string(),
            ],
            vec![None, Some(0), Some(1)],
        )
    }

    const CHAIN: &str = r#"(
        id: "torogao-vrm-spring/1",
        chains: [(
            name: "spine test",
            joints: [
                (node: "Spine", hit_radius: 0.1, stiffness: 1.0, gravity_power: 1.0, gravity_dir: (0.0, -1.0, 0.0), drag_force: 0.5),
                (node: "Head", hit_radius: 0.1, stiffness: 1.0, gravity_power: 1.0, gravity_dir: (0.0, -1.0, 0.0), drag_force: 0.5),
            ],
        )],
    )"#;

    #[test]
    fn chain_resolves_and_emits() {
        let (names, parents) = index();
        let index = NodeIndex {
            names: &names,
            parents: &parents,
        };
        let spec = SpringSpec::load(CHAIN, &index).expect("chain loads");
        let json = spec.to_json();
        assert_eq!(json["specVersion"], "1.0");
        assert_eq!(json["springs"][0]["joints"][1]["node"], 2);
    }

    #[test]
    fn dangling_joint_is_named() {
        let (names, parents) = index();
        let index = NodeIndex {
            names: &names,
            parents: &parents,
        };
        let text = CHAIN.replace("\"Head\"", "\"LeftHand\"");
        let err = SpringSpec::load(&text, &index).expect_err("must refuse");
        assert!(matches!(err, VrmError::DanglingSpringJoint { .. }));
    }

    #[test]
    fn vrm_space_name_is_called_out() {
        let (names, parents) = index();
        let index = NodeIndex {
            names: &names,
            parents: &parents,
        };
        let text = CHAIN.replace("\"Head\"", "\"leftHand\"");
        let err = SpringSpec::load(&text, &index).expect_err("must refuse");
        assert!(matches!(err, VrmError::UnknownBoneName { .. }));
    }

    #[test]
    fn out_of_range_drag_is_refused() {
        let (names, parents) = index();
        let index = NodeIndex {
            names: &names,
            parents: &parents,
        };
        let text = CHAIN.replace("drag_force: 0.5", "drag_force: 1.5");
        let err = SpringSpec::load(&text, &index).expect_err("must refuse");
        assert!(matches!(err, VrmError::SpringValueOutOfRange { .. }));
    }
}
