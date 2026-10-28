//! `project_inspect` (the census computed from a project) and
//! `expression_test` (an expression's evaluated state).
//!
//! The census is an output of authoring, in the shape the brief's
//! §Item 4 names: parts and cut, deformer tree, parameter table
//! with classes and 2-axis data, mesh density, draw bands, masks,
//! swaps, expression inventory, physics. Every list is in id order
//! (draw bands in band/index order), so the census of a project is
//! a pure function of its bytes. Tree walks are bounded by the
//! collection size, so a hand-edited cyclic file cannot hang them.

use crate::model::{AssetKind, Blend, Completion, Deformer, DeformerKind, Keyforms, Project};
use crate::refusal::{Refusal, RefusalClass};
use crate::validate::lattice_keys;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The census schema token.
pub const CENSUS_FORMAT: &str = "torogao-author-census/1";

fn depth_of(
    start: Option<&str>,
    bound: usize,
    parent_of: impl Fn(&str) -> Option<String>,
) -> usize {
    let mut depth = 0;
    let mut current = start.map(str::to_string);
    while let Some(id) = current {
        depth += 1;
        if depth > bound {
            break;
        }
        current = parent_of(&id);
    }
    depth
}

fn keyed_on<F>(keyforms: Option<&Keyforms<F>>) -> Value {
    keyforms.map_or(Value::Array(Vec::new()), |k| json!(k.axes))
}

fn deformer_axes(deformer: &Deformer) -> Value {
    match &deformer.kind {
        DeformerKind::Rotation(r) => keyed_on(r.keyforms.as_ref()),
        DeformerKind::Warp(w) => keyed_on(w.keyforms.as_ref()),
    }
}

/// The full census of a project.
pub fn census(project: &Project) -> Value {
    json!({
        "assets": assets_census(project),
        "census": CENSUS_FORMAT,
        "deformers": deformers_census(project),
        "draw": draw_census(project),
        "expressions": expressions_census(project),
        "lattices": lattices_census(project),
        "masks": project.parts.iter().filter_map(|part| part.clip.as_ref().map(|clip| json!({
            "inverted": clip.inverted,
            "part": part.id,
            "source": clip.source,
        }))).collect::<Vec<_>>(),
        "mesh_density": mesh_census(project),
        "parameters": parameters_census(project),
        "parts": parts_census(project),
        "physics": {"groups": 0, "note": "physics authoring arrives in slice A2"},
        "project": project.header.name,
        "stamp": {
            "lane": project.header.stamp.lane,
            "pack": project.header.stamp.pack,
            "rating": project.header.stamp.rating,
        },
        "swaps": swaps_census(project),
    })
}

fn parts_census(project: &Project) -> Value {
    let bound = project.parts.len();
    let parent_of = |id: &str| project.part(id).and_then(|p| p.parent.clone());
    let tree: Vec<Value> = project
        .parts
        .iter()
        .map(|part| {
            json!({
                "children": project.parts.iter().filter(|p| p.parent.as_deref() == Some(&part.id)).count(),
                "deformer": part.deformer,
                "depth": depth_of(part.parent.as_deref(), bound, parent_of),
                "drawable": part.mesh.is_some(),
                "id": part.id,
                "parent": part.parent,
                "visible": part.visible,
            })
        })
        .collect();
    let completion = |want: Completion| {
        project
            .parts
            .iter()
            .filter(|p| p.completion == Some(want))
            .count()
    };
    json!({
        "art_bound": project.parts.iter().filter(|p| p.art.is_some()).count(),
        "count": project.parts.len(),
        "cut": {
            "clip_bases": project.parts.iter().filter(|p| p.clip.is_some()).count(),
            "completion_not_needed": completion(Completion::NotNeeded),
            "completion_painted": completion(Completion::Painted),
            "separated_layers": project.assets.iter().filter(|a| a.kind == AssetKind::PsdLayer).count(),
        },
        "depth_max": project.parts.iter().map(|p| depth_of(p.parent.as_deref(), bound, parent_of)).max().unwrap_or(0),
        "drawable": project.parts.iter().filter(|p| p.mesh.is_some()).count(),
        "roots": project.parts.iter().filter(|p| p.parent.is_none()).map(|p| p.id.clone()).collect::<Vec<_>>(),
        "tree": tree,
    })
}

fn deformers_census(project: &Project) -> Value {
    let bound = project.deformers.len();
    let parent_of = |id: &str| project.deformer(id).and_then(|d| d.parent.clone());
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    let tree: Vec<Value> = project
        .deformers
        .iter()
        .map(|deformer| {
            *by_kind.entry(deformer.kind_name()).or_default() += 1;
            json!({
                "children": project.deformers.iter().filter(|d| d.parent.as_deref() == Some(&deformer.id)).count(),
                "deforms": project.parts.iter().filter(|p| p.deformer.as_deref() == Some(&deformer.id)).map(|p| p.id.clone()).collect::<Vec<_>>(),
                "depth": depth_of(deformer.parent.as_deref(), bound, parent_of),
                "id": deformer.id,
                "keyed_on": deformer_axes(deformer),
                "kind": deformer.kind_name(),
                "parent": deformer.parent,
            })
        })
        .collect();
    let glues: Vec<Value> = project
        .glues
        .iter()
        .map(|glue| json!({"a": glue.a, "b": glue.b, "id": glue.id, "pairs": glue.pairs.len(), "weight": glue.weight}))
        .collect();
    json!({
        "by_kind": by_kind,
        "chain_depth_max": project.deformers.iter().map(|d| depth_of(d.parent.as_deref(), bound, parent_of)).max().unwrap_or(0),
        "count": project.deformers.len(),
        "glues": glues,
        "roots": project.deformers.iter().filter(|d| d.parent.is_none()).map(|d| d.id.clone()).collect::<Vec<_>>(),
        "tree": tree,
    })
}

/// Every (target, axes) lattice in the project, parts first.
fn lattice_axes(project: &Project) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for part in &project.parts {
        if let Some(k) = &part.keyforms {
            out.push((part.id.clone(), k.axes.clone()));
        }
    }
    for deformer in &project.deformers {
        if let Some(axes) = deformer.axes() {
            out.push((deformer.id.clone(), axes.to_vec()));
        }
    }
    out
}

fn parameters_census(project: &Project) -> Value {
    let lattices = lattice_axes(project);
    let mut by_class: BTreeMap<&str, usize> = BTreeMap::new();
    let table: Vec<Value> = project
        .parameters
        .iter()
        .map(|parameter| {
            *by_class.entry(parameter.class.as_str()).or_default() += 1;
            let keyed: Vec<&str> = lattices
                .iter()
                .filter(|(_, axes)| axes.contains(&parameter.id))
                .map(|(target, _)| target.as_str())
                .collect();
            let mut partners: Vec<&str> = lattices
                .iter()
                .filter(|(_, axes)| axes.len() == 2 && axes.contains(&parameter.id))
                .flat_map(|(_, axes)| axes.iter().map(String::as_str))
                .filter(|axis| *axis != parameter.id)
                .collect();
            partners.sort_unstable();
            partners.dedup();
            json!({
                "class": parameter.class,
                "default": parameter.default,
                "grid_partners": partners,
                "id": parameter.id,
                "keyed_targets": keyed,
                "links": [],
                "max": parameter.max,
                "min": parameter.min,
                "neutral": project.neutral_value(parameter),
            })
        })
        .collect();
    let grids: Vec<Value> = lattices
        .iter()
        .filter(|(_, axes)| axes.len() == 2)
        .map(|(target, axes)| json!({"axes": axes, "target": target}))
        .collect();
    json!({
        "by_class": by_class,
        "count": project.parameters.len(),
        "grids": grids,
        "links_note": "param_link_define is conditional on AUTH-D3; no links in A1",
        "table": table,
    })
}

fn mesh_census(project: &Project) -> Value {
    let parts: Vec<Value> = project
        .parts
        .iter()
        .filter_map(|part| {
            part.mesh.as_ref().map(|mesh| {
                json!({"id": part.id, "triangles": mesh.indices.len() / 3, "vertices": mesh.vertices.len()})
            })
        })
        .collect();
    let meshes = project.parts.iter().filter_map(|p| p.mesh.as_ref());
    let (vertices, triangles) = meshes.fold((0, 0), |(v, t), mesh| {
        (v + mesh.vertices.len(), t + mesh.indices.len() / 3)
    });
    json!({"parts": parts, "total_triangles": triangles, "total_vertices": vertices})
}

fn draw_census(project: &Project) -> Value {
    let mut bands: BTreeMap<u32, Vec<(u32, String)>> = BTreeMap::new();
    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for part in &project.parts {
        if let Some(draw) = &part.draw {
            bands
                .entry(draw.band)
                .or_default()
                .push((draw.index, part.id.clone()));
            if let Some(group) = &draw.group {
                groups.entry(group.as_str()).or_default().push(&part.id);
            }
        }
    }
    let bands: Vec<Value> = bands
        .into_iter()
        .map(|(band, mut members)| {
            members.sort();
            json!({"band": band, "parts": members.into_iter().map(|(_, id)| id).collect::<Vec<_>>()})
        })
        .collect();
    let unbanded: Vec<&str> = project
        .parts
        .iter()
        .filter(|p| p.mesh.is_some() && p.draw.is_none())
        .map(|p| p.id.as_str())
        .collect();
    json!({"bands": bands, "groups": groups, "unbanded_drawables": unbanded})
}

fn swaps_census(project: &Project) -> Value {
    let sets: Vec<Value> = project
        .variant_sets
        .iter()
        .map(|set| {
            let visible: Vec<&str> = set
                .variants
                .iter()
                .filter(|id| project.part(id).is_some_and(|p| p.visible))
                .map(String::as_str)
                .collect();
            json!({"id": set.id, "variants": set.variants, "visible": visible})
        })
        .collect();
    json!({
        "hidden_by_default": project.parts.iter().filter(|p| !p.visible).map(|p| p.id.clone()).collect::<Vec<_>>(),
        "variant_sets": sets,
    })
}

fn blend_name(blend: Blend) -> &'static str {
    match blend {
        Blend::Additive => "additive",
        Blend::Override => "override",
    }
}

fn expressions_census(project: &Project) -> Value {
    let table: Vec<Value> = project
        .expressions
        .iter()
        .map(|expression| {
            json!({
                "blend": blend_name(expression.blend),
                "id": expression.id,
                "parameters": expression.values.iter().map(|row| row.parameter.clone()).collect::<Vec<_>>(),
                "part_swaps": expression.visibility.iter().map(|row| json!({"part": row.part, "visible": row.visible})).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({"count": project.expressions.len(), "table": table})
}

fn lattice_entry<F>(target: &str, keyforms: &Keyforms<F>) -> Value {
    let keys = lattice_keys(keyforms);
    let expected: usize = keys.iter().map(Vec::len).product();
    json!({
        "axes": keyforms.axes,
        "complete": keys.iter().all(|axis| axis.len() >= 2) && keyforms.forms.len() == expected,
        "forms": keyforms.forms.len(),
        "keys": keys,
        "target": target,
    })
}

fn lattices_census(project: &Project) -> Vec<Value> {
    let mut out = Vec::new();
    for part in &project.parts {
        if let Some(k) = &part.keyforms {
            out.push(lattice_entry(&part.id, k));
        }
    }
    for deformer in &project.deformers {
        match &deformer.kind {
            DeformerKind::Rotation(r) => {
                out.extend(r.keyforms.as_ref().map(|k| lattice_entry(&deformer.id, k)))
            }
            DeformerKind::Warp(w) => {
                out.extend(w.keyforms.as_ref().map(|k| lattice_entry(&deformer.id, k)))
            }
        }
    }
    out
}

fn assets_census(project: &Project) -> Value {
    json!({
        "count": project.assets.len(),
        "png": project.assets.iter().filter(|a| a.kind == AssetKind::Png).count(),
        "psd_layers": project.assets.iter().filter(|a| a.kind == AssetKind::PsdLayer).count(),
        "with_content_bounds": project.assets.iter().filter(|a| a.content.is_some()).count(),
    })
}

/// `expression_test`: the parameter values and part visibility
/// while one expression is active over the neutral pose. Additive
/// rows add then clamp to range; override rows replace. The golden
/// still the manifest also names needs a renderer (RP3); A1 reports
/// the evaluated state only, and says so in the output.
pub fn expression_test(project: &Project, id: &str) -> Result<Value, Refusal> {
    let Some(expression) = project.expression(id) else {
        return Err(Refusal::new(
            RefusalClass::UnknownTarget,
            format!("expression '{id}' does not exist"),
        ));
    };
    let parameters: Vec<Value> = project
        .parameters
        .iter()
        .map(|parameter| {
            let neutral = project.neutral_value(parameter);
            let row = expression
                .values
                .iter()
                .find(|row| row.parameter == parameter.id);
            let value = match (row, expression.blend) {
                (None, _) => neutral,
                (Some(row), Blend::Override) => row.value,
                (Some(row), Blend::Additive) => {
                    (neutral + row.value).clamp(parameter.min, parameter.max)
                }
            };
            json!({"id": parameter.id, "value": value})
        })
        .collect();
    let visibility: Vec<Value> = project
        .parts
        .iter()
        .map(|part| {
            let row = expression.visibility.iter().find(|row| row.part == part.id);
            json!({"part": part.id, "visible": row.map_or(part.visible, |row| row.visible)})
        })
        .collect();
    Ok(json!({
        "blend": blend_name(expression.blend),
        "expression": expression.id,
        "parameters": parameters,
        "render": "not in A1: the golden still needs the RP3 renderer (slice A3)",
        "visibility": visibility,
    }))
}
