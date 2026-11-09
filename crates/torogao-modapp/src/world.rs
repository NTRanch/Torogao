//! Graph navigation and state-table resolution over the
//! measured tables: pure functions, no session state.
//!
//! Two measured behaviors live here. `resolve_state` is
//! DialogueAction.GetMiaState: the Default row with the named
//! node's delta merged over it. `route` is the PathsMap quick-
//! travel path: the Default-rooted paths to the current node
//! and to the target, joined at their first common node.

use std::collections::BTreeSet;

use crate::dynamics::{Dynamics, ModappError, ResolvedState};

/// Depth bound for graph walks. The measured graph's longest
/// root path is 6 nodes; the bound exists so a corrupt table
/// cannot drive unbounded recursion, not as a claim about the
/// source.
pub const ROUTE_DEPTH_MAX: usize = 32;

/// Merge the Default state row with a node's delta. Unknown
/// ids (including INTRO, which carries no state row) are
/// typed refusals.
pub fn resolve_state(dynm: &Dynamics, node_id: &str) -> Result<ResolvedState, ModappError> {
    let base = dynm.state_delta("Default")?;
    let delta = dynm.state_delta(node_id)?;
    // The loader validates the Default row complete, so these
    // unwraps are on loader-established facts, not on input.
    let mut resolved = ResolvedState {
        body: base.body.clone().expect("validated Default body"),
        face: base.face.clone().expect("validated Default face"),
        extras: base.extras.clone().expect("validated Default extras"),
        position: base.position.expect("validated Default position"),
        scale: base.scale.expect("validated Default scale"),
        face_position: base.face_position.expect("validated Default face position"),
        schedule: dynm.schedule().to_vec(),
    };
    if let Some(body) = &delta.body {
        resolved.body = body.clone();
    }
    if let Some(face) = &delta.face {
        resolved.face = face.clone();
    }
    if let Some(extras) = &delta.extras {
        resolved.extras = extras.clone();
    }
    if let Some(position) = delta.position {
        resolved.position = position;
    }
    if let Some(scale) = delta.scale {
        resolved.scale = scale;
    }
    if let Some(face_position) = delta.face_position {
        resolved.face_position = face_position;
    }
    Ok(resolved)
}

/// The path of node ids from Default down to `target`
/// (inclusive), following child edges. The graph is a DAG;
/// the search is depth-first in table order with a visited
/// set, matching the source's first-found path behavior.
pub fn root_path(dynm: &Dynamics, target: &str) -> Result<Vec<String>, ModappError> {
    dynm.node(target)?;
    let mut visited = BTreeSet::new();
    let mut path = Vec::new();
    if search(dynm, "Default", target, &mut visited, &mut path, 0)? {
        Ok(path)
    } else {
        Err(ModappError::UnreachableNode {
            id: target.to_string(),
        })
    }
}

fn search(
    dynm: &Dynamics,
    at: &str,
    target: &str,
    visited: &mut BTreeSet<String>,
    path: &mut Vec<String>,
    depth: usize,
) -> Result<bool, ModappError> {
    if depth > ROUTE_DEPTH_MAX {
        return Err(ModappError::InvalidStep {
            reason: "route search depth above bound",
        });
    }
    if !visited.insert(at.to_string()) {
        return Ok(false);
    }
    path.push(at.to_string());
    if at == target {
        return Ok(true);
    }
    let children = dynm.node(at)?.children.clone();
    for child in children {
        if search(dynm, &child, target, visited, path, depth + 1)? {
            return Ok(true);
        }
    }
    path.pop();
    Ok(false)
}

/// The quick-travel route from `current` to `target`: the
/// Default-rooted path to `current`, walked from the root,
/// joined onto the Default-rooted path to `target` at their
/// first common node (PathsMap.SelectNode's construction).
/// The route includes Default and `target`; its length is
/// what the per-node price multiplies.
pub fn route(dynm: &Dynamics, current: &str, target: &str) -> Result<Vec<String>, ModappError> {
    let path_a = root_path(dynm, current)?;
    let path_b = root_path(dynm, target)?;
    // The source reverses the current node's path and walks
    // from the current node back toward the root, joining
    // the target's path at the first common node.
    let mut out: Vec<String> = Vec::new();
    for element in path_a.iter().rev() {
        out.push(element.clone());
        if let Some(matched) = path_b.iter().position(|e| e == element) {
            for next in &path_b[matched + 1..] {
                out.push(next.clone());
            }
            return Ok(out);
        }
    }
    // Both paths start at Default, so a common element always
    // exists in a validated table; reaching here means the
    // invariant broke, which is an internal fault, not input.
    Err(ModappError::UnreachableNode {
        id: target.to_string(),
    })
}

/// The nodes of the graph visible from a seen set, per the
/// PathsMap rule: a node is visible if it is seen or its
/// parent is seen (children of seen nodes are shown).
pub fn visible_nodes(dynm: &Dynamics, seen: &BTreeSet<String>) -> BTreeSet<String> {
    let mut visible = BTreeSet::new();
    for node in dynm.nodes() {
        if seen.contains(&node.id) {
            visible.insert(node.id.clone());
            for child in &node.children {
                visible.insert(child.clone());
            }
        }
    }
    visible
}

/// Explored count for the map's progress label:
/// (seen − 1) against (nodes − 1) over the non-splitter
/// graph this table carries (INTRO excluded by the source's
/// own accounting: it counts holder children minus one).
pub fn explored_counts(dynm: &Dynamics, seen: &BTreeSet<String>) -> (usize, usize) {
    let seen_count = seen.len().saturating_sub(1);
    let total = dynm.node_count().saturating_sub(2);
    (seen_count, total)
}
