//! The export group's read that lives in the author crate
//! (BRIEF-AUTHORING, export group; landed by the G-10
//! completion): `export_manifest` — provenance, consent/lane
//! stamps, and the required-constructs list, computed from the
//! project record alone.
//!
//! The file-writing export acts are not here: `export_bundle`
//! and `export_native` are descoped from this surface by
//! recorded ruling (`op::DESCOPED`) — bundle assembly is the
//! CLI's export face and INP emission is torogao-format's
//! writer; this crate authors the records both consume.

use serde_json::{Value, json};

use crate::model::Project;

/// The constructs a reader must be able to evaluate to read
/// this project, as (construct name, present?) rows in
/// canonical order. A construct is a section family of the
/// project record.
fn constructs(project: &Project) -> Vec<(&'static str, bool)> {
    vec![
        ("assets", !project.assets.is_empty()),
        ("audio_bindings", !project.audio_binds.is_empty()),
        ("audio_tracks", !project.audio_tracks.is_empty()),
        ("deformers", !project.deformers.is_empty()),
        ("expressions", !project.expressions.is_empty()),
        ("game", project.game.is_some()),
        ("motions", !project.motions.is_empty()),
        ("parameters", !project.parameters.is_empty()),
        ("parts", !project.parts.is_empty()),
        ("physics_groups", !project.physics_groups.is_empty()),
        ("save_schema", project.save_schema.is_some()),
        ("variant_sets", !project.variant_sets.is_empty()),
    ]
}

/// Whether the project uses a construct family the pre-port
/// (D) tooling cannot evaluate: the AUTH-G game sections and
/// the A2/A3 families were authored after that tooling froze,
/// so a project using any of them is stamped Rust-lane-only —
/// named, never silently misreadable (the brief's rule).
fn rust_lane_only(project: &Project) -> bool {
    project.game.is_some()
        || project.save_schema.is_some()
        || project.clock.is_some()
        || !project.rng_streams.is_empty()
        || !project.physics_groups.is_empty()
        || !project.motions.is_empty()
        || !project.audio_tracks.is_empty()
        || !project.audio_binds.is_empty()
}

/// The `export_manifest` answer document for a loaded project.
pub fn export_manifest_document(project: &Project) -> Value {
    let header = &project.header;
    let present: Vec<&str> = constructs(project)
        .into_iter()
        .filter(|(_, present)| *present)
        .map(|(name, _)| name)
        .collect();
    json!({
        "consent": {
            "adult": header.consent.adult,
            "schema": header.consent.schema,
            "wants_selected_scenes": header.consent.wants_selected_scenes,
        },
        "counts": {
            "assets": project.assets.len(),
            "audio_bindings": project.audio_binds.len(),
            "audio_tracks": project.audio_tracks.len(),
            "deformers": project.deformers.len(),
            "expressions": project.expressions.len(),
            "motions": project.motions.len(),
            "parameters": project.parameters.len(),
            "parts": project.parts.len(),
            "physics_groups": project.physics_groups.len(),
            "variant_sets": project.variant_sets.len(),
        },
        "evaluation": if rust_lane_only(project) { "rust-lane-only" } else { "d-evaluable" },
        "model_format": header.model_format,
        "name": header.name,
        "required_constructs": present,
        "result": "ok",
        "stamp": {
            "basis": header.stamp.basis,
            "lane": header.stamp.lane,
            "pack": header.stamp.pack,
            "rating": header.stamp.rating,
        },
    })
}
