//! The Character Creator (creator.d port): lane pack +
//! base template + presets + a character record become
//! a NEW project carrying the pack's governed states.
//! Consent is validated against the pack's own consent
//! model before anything is written.

use crate::extdoc::{write_project_with_ext, EXT_FORMAT};
use crate::packs::{pack_expressions, pack_str};
use crate::templates::instantiate_template;
use crate::util::jget;
use crate::writer::CHARACTER_FORMAT;
use serde_json::{json, Value};

/// TG-CREATOR-CONSENT.
pub const TG_CREATOR_CONSENT: &str = "TG-CREATOR-CONSENT";
/// TG-CREATOR-IDENTITY.
pub const TG_CREATOR_IDENTITY: &str = "TG-CREATOR-IDENTITY";

/// One creation problem: a stable code plus a named
/// field.
#[derive(Debug, Clone)]
pub struct CreatorProblem {
    /// Stable problem code.
    pub code: &'static str,
    /// The record field at fault.
    pub field: String,
    /// Human-readable detail.
    pub message: String,
}

impl CreatorProblem {
    /// The problem as a JSON document.
    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "field": self.field,
            "message": self.message,
        })
    }
}

/// Validates the identity record against the pack:
/// format, name, anchors, and the pack's consent model.
/// An empty result means the identity may be created.
pub fn check_identity(identity: &Value, pack: &Value) -> Vec<CreatorProblem> {
    let mut problems = Vec::new();
    if pack_str(identity, "format") != CHARACTER_FORMAT {
        problems.push(CreatorProblem {
            code: TG_CREATOR_IDENTITY,
            field: "format".to_string(),
            message: format!("identity record is not a {CHARACTER_FORMAT} document"),
        });
        return problems;
    }
    if pack_str(identity, "name").is_empty() {
        problems.push(CreatorProblem {
            code: TG_CREATOR_IDENTITY,
            field: "name".to_string(),
            message: "identity record carries no character name".to_string(),
        });
    }
    if !jget(identity, "anchors").is_object() {
        problems.push(CreatorProblem {
            code: TG_CREATOR_IDENTITY,
            field: "anchors".to_string(),
            message: "identity record carries no anchors object".to_string(),
        });
    }

    let consent = jget(identity, "consent");
    let model = jget(pack, "consentModel");
    let required = jget(model, "requiredFields");
    if !consent.is_object() {
        problems.push(CreatorProblem {
            code: TG_CREATOR_CONSENT,
            field: "consent".to_string(),
            message: "identity record carries no consent object; the pack's consent model requires one".to_string(),
        });
        return problems;
    }
    if pack_str(consent, "schema") != pack_str(model, "schema") {
        problems.push(CreatorProblem {
            code: TG_CREATOR_CONSENT,
            field: "consent.schema".to_string(),
            message: "consent schema does not match the pack's consent model".to_string(),
        });
    }
    if let Some(fields) = required.as_object() {
        for (field, want) in fields {
            let got = jget(consent, field);
            let ok = *want == Value::Bool(true) && *got == Value::Bool(true);
            if !ok {
                problems.push(CreatorProblem {
                    code: TG_CREATOR_CONSENT,
                    field: format!("consent.{field}"),
                    message: format!(
                        "consent field '{field}' must be true under the pack's consent model"
                    ),
                });
            }
        }
    }
    problems
}

/// Creates a character project. On success returns the
/// creation summary; identity problems come back in
/// `problems`, operational failures as the error string.
pub fn create_character(
    identity: &Value,
    pack: &Value,
    presets: &[String],
    out_path: &str,
) -> Result<Value, (Vec<CreatorProblem>, String)> {
    let problems = check_identity(identity, pack);
    if !problems.is_empty() {
        return Err((problems, String::new()));
    }

    let tmp_path = format!("{out_path}.creator-tmp.inx");
    let inst = instantiate_template("base-humanoid", presets, &tmp_path)
        .map_err(|e| (Vec::new(), format!("template instantiation failed: {e}")))?;

    let lane = pack_str(pack, "lane");
    let rating = if lane == "adult" { "adult" } else { "all-ages" };
    let mut states = Vec::new();
    for expression in pack_expressions(pack) {
        states.push(json!({
            "name": pack_str(&expression, "name"),
            "recipe": jget(&expression, "defaultRecipe").clone(),
        }));
    }
    let pack_controllers = jget(pack, "defaultControllers");
    let controllers = if pack_controllers.is_array() {
        pack_controllers.clone()
    } else {
        json!([])
    };
    let ext = json!({
        "format": EXT_FORMAT,
        "meta": {
            "pack": pack_str(pack, "name"),
            "lane": lane,
            "rating": rating,
            "basis": "lane pack default at creation (create-character)",
        },
        "character": identity.clone(),
        "states": states.clone(),
        "variants": [],
        "blendshapes": [],
        "controllers": controllers,
        "trackingClaims": ["face-standard"],
        "consumer": { "hitAreas": [] },
    });

    if let Some(parent) = std::path::Path::new(out_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let write_result = write_project_with_ext(&tmp_path, out_path, &ext);
    if std::path::Path::new(&tmp_path).exists() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    let _ = inst;
    write_result.map_err(|e| (Vec::new(), e))?;

    Ok(json!({
        "character": pack_str(identity, "name"),
        "lane": lane,
        "out": out_path,
        "pack": pack_str(pack, "name"),
        "presets": presets,
        "rating": rating,
        "result": "created",
        "states": states.len(),
        "trackingClaims": ["face-standard"],
    }))
}
