//! `project_diff`: a structural diff of two projects, by
//! collection and id. Records are compared whole; the diff names
//! which ids were added, removed, or changed, never field-level
//! edits (the op log is the record of how a change was made).

use crate::model::Project;
use serde_json::{Value, json};

fn collection<T: PartialEq>(left: &[T], right: &[T], key: impl Fn(&T) -> &str) -> Value {
    let find = |items: &[T], id: &str| items.iter().find(|item| key(item) == id).map(|_| ());
    let added: Vec<&str> = right
        .iter()
        .map(&key)
        .filter(|id| find(left, id).is_none())
        .collect();
    let removed: Vec<&str> = left
        .iter()
        .map(&key)
        .filter(|id| find(right, id).is_none())
        .collect();
    let changed: Vec<&str> = left
        .iter()
        .filter(|item| {
            right
                .iter()
                .find(|other| key(other) == key(item))
                .is_some_and(|other| other != *item)
        })
        .map(&key)
        .collect();
    json!({"added": added, "changed": changed, "removed": removed})
}

/// The structural diff of `left` → `right`.
pub fn diff(left: &Project, right: &Project) -> Value {
    json!({
        "collections": {
            "assets": collection(&left.assets, &right.assets, |a| &a.id),
            "deformers": collection(&left.deformers, &right.deformers, |d| &d.id),
            "expressions": collection(&left.expressions, &right.expressions, |e| &e.id),
            "glues": collection(&left.glues, &right.glues, |g| &g.id),
            "parameters": collection(&left.parameters, &right.parameters, |p| &p.id),
            "parts": collection(&left.parts, &right.parts, |p| &p.id),
            "pose": collection(&left.pose, &right.pose, |e| &e.parameter),
            "variant_sets": collection(&left.variant_sets, &right.variant_sets, |s| &s.id),
        },
        "header_changed": left.header != right.header,
        "identical": left == right,
    })
}
