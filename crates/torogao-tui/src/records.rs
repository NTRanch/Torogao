//! Record families over the authored project model.
//!
//! The browser shows the collections `project.ron` holds today:
//! header, assets, parts, deformers, parameters, pose,
//! expressions, glues, variant sets. Families the format gains
//! later (the Phase 2 game families: dialogue, characters,
//! state, threads) join by adding a builder here — the browser,
//! detail view, and edit-prefill machinery are family-agnostic.
//!
//! Editing never writes a record directly. [`prefill`] maps a
//! record to the authoring verb whose upsert edits it, and the
//! verb runs through `torogao_author`'s decide() pipeline — the
//! same semantics the CLI and MCP faces carry.

use serde_json::{Value, json};
use torogao_author::model::{Blend, Project};

/// One browsable record: an id, a one-line summary, and the
/// detail lines the detail screen renders.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// The record's id within its family.
    pub id: String,
    /// One-line summary for the record list.
    pub summary: String,
    /// Detail lines for the detail screen.
    pub detail: Vec<String>,
}

/// One record family: a display name and its records, in the
/// project's canonical (id-sorted) order.
#[derive(Clone, Debug, PartialEq)]
pub struct Family {
    /// Display name (matches the project file's section names).
    pub name: &'static str,
    /// The family's records.
    pub records: Vec<Record>,
}

/// Builds every family from the loaded project, in display
/// order (the alphabetical order of the project file's own
/// sections, header first as the project's identity).
pub fn families(project: &Project) -> Vec<Family> {
    vec![
        header_family(project),
        assets_family(project),
        deformers_family(project),
        expressions_family(project),
        glues_family(project),
        parameters_family(project),
        parts_family(project),
        pose_family(project),
        variant_sets_family(project),
    ]
}

/// The authoring verb + inputs that edit this record in place,
/// where the core provides a set-verb for the record's mutable
/// state (pose values, part visibility).
///
/// Definition records (parameters, expressions, deformers, …)
/// deliberately return `None`: the core's definition verbs are
/// create-once — redefining an id with different inputs refuses
/// `TG-AUTH-CONFLICT` by design — so their edit path is
/// remove + define through the console's verb picker, and the
/// browser says so rather than offering an edit that must fail.
pub fn prefill(family: &str, record_id: &str, project: &Project) -> Option<(String, Value)> {
    match family {
        "pose" => {
            let entry = project
                .pose
                .iter()
                .find(|entry| entry.parameter == record_id)?;
            Some((
                "param_pose_default_set".to_string(),
                json!({"parameter": entry.parameter, "value": entry.value}),
            ))
        }
        "parts" => {
            let part = project.part(record_id)?;
            Some((
                "part_visibility_set".to_string(),
                json!({"part": part.id, "visible": part.visible}),
            ))
        }
        _ => None,
    }
}

fn header_family(project: &Project) -> Family {
    let header = &project.header;
    let record = Record {
        id: header.name.clone(),
        summary: format!(
            "{} · lane {} · rating {}",
            header.name, header.stamp.lane, header.stamp.rating
        ),
        detail: vec![
            format!("name: {}", header.name),
            format!("model format: {}", header.model_format),
            format!("stamp basis: {}", header.stamp.basis),
            format!("stamp lane: {}", header.stamp.lane),
            format!("stamp pack: {}", header.stamp.pack),
            format!("stamp rating: {}", header.stamp.rating),
            format!("consent schema: {}", header.consent.schema),
            format!("consent adult: {}", header.consent.adult),
            format!(
                "consent wants_selected_scenes: {}",
                header.consent.wants_selected_scenes
            ),
        ],
    };
    Family {
        name: "header",
        records: vec![record],
    }
}

fn assets_family(project: &Project) -> Family {
    let records = project
        .assets
        .iter()
        .map(|asset| Record {
            id: asset.id.clone(),
            summary: format!(
                "{} · {}x{} · {}",
                asset.file, asset.width, asset.height, asset.sha256
            ),
            detail: vec![
                format!("file: {}", asset.file),
                format!("sha256: {}", asset.sha256),
                format!("size: {}x{}", asset.width, asset.height),
                format!("license: {}", asset.provenance.license),
                format!("origin: {}", asset.provenance.origin),
                format!("source: {}", asset.provenance.source_name),
                format!("source sha256: {}", asset.provenance.source_sha256),
            ],
        })
        .collect();
    Family {
        name: "assets",
        records,
    }
}

fn deformers_family(project: &Project) -> Family {
    let records = project
        .deformers
        .iter()
        .map(|deformer| Record {
            id: deformer.id.clone(),
            summary: format!(
                "{} · parent {}",
                deformer.kind_name(),
                deformer.parent.as_deref().unwrap_or("-")
            ),
            detail: vec![
                format!("kind: {}", deformer.kind_name()),
                format!("parent: {}", deformer.parent.as_deref().unwrap_or("-")),
                format!(
                    "lattice axes: {}",
                    deformer
                        .axes()
                        .map(|axes| axes.join(", "))
                        .unwrap_or_else(|| "-".to_string())
                ),
            ],
        })
        .collect();
    Family {
        name: "deformers",
        records,
    }
}

fn expressions_family(project: &Project) -> Family {
    let records = project
        .expressions
        .iter()
        .map(|expression| {
            let mut detail = vec![format!(
                "blend: {}",
                match expression.blend {
                    Blend::Additive => "additive",
                    Blend::Override => "override",
                }
            )];
            for value in &expression.values {
                detail.push(format!("value {} = {}", value.parameter, value.value));
            }
            for row in &expression.visibility {
                detail.push(format!("visibility {} = {}", row.part, row.visible));
            }
            Record {
                id: expression.id.clone(),
                summary: format!(
                    "{} values · {} visibility rows",
                    expression.values.len(),
                    expression.visibility.len()
                ),
                detail,
            }
        })
        .collect();
    Family {
        name: "expressions",
        records,
    }
}

fn glues_family(project: &Project) -> Family {
    let records = project
        .glues
        .iter()
        .map(|glue| Record {
            id: glue.id.clone(),
            summary: format!(
                "{} <-> {} · {} pairs · weight {}",
                glue.a,
                glue.b,
                glue.pairs.len(),
                glue.weight
            ),
            detail: vec![
                format!("a: {}", glue.a),
                format!("b: {}", glue.b),
                format!("pairs: {}", glue.pairs.len()),
                format!("weight: {}", glue.weight),
            ],
        })
        .collect();
    Family {
        name: "glues",
        records,
    }
}

fn parameters_family(project: &Project) -> Family {
    let records = project
        .parameters
        .iter()
        .map(|parameter| Record {
            id: parameter.id.clone(),
            summary: format!(
                "{} · [{}..{}] · default {}",
                parameter.class, parameter.min, parameter.max, parameter.default
            ),
            detail: vec![
                format!("class: {}", parameter.class),
                format!("min: {}", parameter.min),
                format!("default: {}", parameter.default),
                format!("max: {}", parameter.max),
                format!("neutral (pose) value: {}", project.neutral_value(parameter)),
            ],
        })
        .collect();
    Family {
        name: "parameters",
        records,
    }
}

fn parts_family(project: &Project) -> Family {
    let records = project
        .parts
        .iter()
        .map(|part| Record {
            id: part.id.clone(),
            summary: format!(
                "parent {} · {} · {}",
                part.parent.as_deref().unwrap_or("-"),
                if part.mesh.is_some() {
                    "drawable"
                } else {
                    "node"
                },
                if part.visible { "visible" } else { "hidden" },
            ),
            detail: vec![
                format!("parent: {}", part.parent.as_deref().unwrap_or("-")),
                format!("deformer: {}", part.deformer.as_deref().unwrap_or("-")),
                format!(
                    "art asset: {}",
                    part.art.as_ref().map_or("-", |art| &art.asset)
                ),
                format!("mesh: {}", part.mesh.is_some()),
                format!("clip: {}", part.clip.is_some()),
                format!("visible: {}", part.visible),
            ],
        })
        .collect();
    Family {
        name: "parts",
        records,
    }
}

fn pose_family(project: &Project) -> Family {
    let records = project
        .pose
        .iter()
        .map(|entry| Record {
            id: entry.parameter.clone(),
            summary: format!("neutral value {}", entry.value),
            detail: vec![
                format!("parameter: {}", entry.parameter),
                format!("value: {}", entry.value),
            ],
        })
        .collect();
    Family {
        name: "pose",
        records,
    }
}

fn variant_sets_family(project: &Project) -> Family {
    let records = project
        .variant_sets
        .iter()
        .map(|set| Record {
            id: set.id.clone(),
            summary: format!(
                "{} variants: {}",
                set.variants.len(),
                set.variants.join(", ")
            ),
            detail: vec![format!("variants: {}", set.variants.join(", "))],
        })
        .collect();
    Family {
        name: "variant sets",
        records,
    }
}
