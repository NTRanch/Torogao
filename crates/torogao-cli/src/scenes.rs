//! Scene documents and validation (structural + grounded),
//! ported from the D `torogao.cli.scenes` module.

use std::collections::HashSet;

use serde_json::{json, Value};

use torogao_engine::Puppet;

use crate::criteria::Finding;
use crate::effects::{puppet_param_names, validate_effects, TG_FX_BINDING};
use crate::extdoc::node_names;
use crate::packs::pack_str;
use crate::paths::find_data_file;
use crate::util::jget;
use crate::walk::Walk;

pub const SCENE_FORMAT: &str = "torogao-scene/1";
pub const TG_SCENE_CONSENT: &str = "TG-SCENE-CONSENT";
pub const TG_SCENE_DUPLICATE: &str = "TG-SCENE-DUPLICATE";
pub const TG_SCENE_EMITTER: &str = "TG-SCENE-EMITTER";
pub const TG_SCENE_FORMAT: &str = "TG-SCENE-FORMAT";
pub const TG_SCENE_PACK: &str = "TG-SCENE-PACK";
pub const TG_SCENE_TYPE: &str = "TG-SCENE-TYPE";
pub const TG_SCENE_UNKNOWN_PARAM: &str = "TG-SCENE-UNKNOWN-PARAM";
pub const TG_SCENE_UNKNOWN_PART: &str = "TG-SCENE-UNKNOWN-PART";

pub fn load_scene_doc(path_or_name: &str) -> Result<Value, String> {
    let mut path = path_or_name.to_string();
    if !std::path::Path::new(&path).exists() {
        match find_data_file("TOROGAO_SCENES_DIR", "scenes", &format!("{path_or_name}.json")) {
            Some(p) => path = p.to_string_lossy().into_owned(),
            None => return Err(format!("scene '{path_or_name}' not found")),
        }
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("scene '{path_or_name}' is unreadable: {e}"))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("scene '{path_or_name}' is unreadable: {e}"))
}

fn count_of(v: &Value) -> i64 {
    v.as_array().map_or(0, |a| a.len() as i64)
}

pub fn validate_scene(scene: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    if !scene.is_object() {
        findings.push(Finding::new(
            TG_SCENE_TYPE,
            "error",
            "scene",
            "scene document is not a JSON object",
        ));
        return findings;
    }
    if pack_str(scene, "format") != SCENE_FORMAT {
        findings.push(Finding::new(
            TG_SCENE_FORMAT,
            "error",
            "scene.format",
            format!("format is not '{SCENE_FORMAT}'"),
        ));
        return findings;
    }
    if pack_str(scene, "name").is_empty() {
        findings.push(Finding::new(
            TG_SCENE_TYPE,
            "error",
            "scene.name",
            "scene requires a name",
        ));
    }
    if pack_str(scene, "pack").is_empty() {
        findings.push(Finding::new(
            TG_SCENE_TYPE,
            "error",
            "scene.pack",
            "scene requires the pack it belongs to",
        ));
    }
    let plates = jget(&jget(scene, "background"), "plates");
    if let Some(plates) = plates.as_array() {
        for (i, plate) in plates.iter().enumerate() {
            if pack_str(plate, "file").is_empty() {
                findings.push(Finding::new(
                    TG_SCENE_TYPE,
                    "error",
                    format!("background.plates[{i}].file"),
                    "background plate names no file",
                ));
            }
        }
    }
    let mut ids: HashSet<String> = HashSet::new();
    for what in ["fixtures", "props", "fluids"] {
        let list = jget(scene, what);
        let Some(list) = list.as_array() else { continue };
        for (i, entry) in list.iter().enumerate() {
            let id = pack_str(entry, "id");
            let at = format!("{what}[{i}]");
            if id.is_empty() {
                findings.push(Finding::new(
                    TG_SCENE_TYPE,
                    "error",
                    format!("{at}.id"),
                    format!("{what} entry requires an id"),
                ));
                continue;
            }
            if ids.contains(&id) {
                findings.push(Finding::new(
                    TG_SCENE_DUPLICATE,
                    "error",
                    format!("{at}.id"),
                    format!("duplicate scene id '{id}'"),
                ));
            }
            ids.insert(id);
        }
    }
    let fluids = jget(scene, "fluids");
    if let Some(fluids) = fluids.as_array() {
        for (i, emitter) in fluids.iter().enumerate() {
            let at = format!("fluids[{i}]");
            let behavior = pack_str(emitter, "behavior");
            if behavior != "spray" && behavior != "milk" && behavior != "saliva" {
                findings.push(Finding::new(
                    TG_SCENE_EMITTER,
                    "error",
                    format!("{at}.behavior"),
                    format!(
                        "unknown fluid behavior '{behavior}' (spray, milk, saliva)"
                    ),
                ));
            }
            let source = jget(emitter, "source");
            if !source.is_object() || pack_str(source, "part").is_empty() {
                findings.push(Finding::new(
                    TG_SCENE_EMITTER,
                    "error",
                    format!("{at}.source"),
                    "fluid emitter requires a source part",
                ));
            }
        }
    }
    let fx = jget(scene, "effects");
    if fx.is_object() {
        for f in validate_effects(fx, None) {
            findings.push(Finding::new(
                f.code,
                f.severity,
                format!("effects.{}", f.subject),
                f.message.clone(),
            ));
        }
    }
    findings
}

pub fn validate_scene_grounded(
    scene: &Value,
    puppet: &Puppet,
    walk: &Walk,
    pack: &Value,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let part_set = node_names(puppet, walk);
    let param_names = puppet_param_names(puppet);
    let param_set: HashSet<&String> = param_names.iter().collect();
    let scene_pack = pack_str(scene, "pack");
    let pack_name = pack_str(pack, "name");
    if !scene_pack.is_empty() && !pack_name.is_empty() && scene_pack != pack_name {
        findings.push(Finding::new(
            TG_SCENE_PACK,
            "error",
            "scene.pack",
            format!(
                "scene belongs to pack '{scene_pack}' but the project is validated against pack '{pack_name}'"
            ),
        ));
    }
    let fixtures = jget(scene, "fixtures");
    if let Some(fixtures) = fixtures.as_array() {
        for (i, fixture) in fixtures.iter().enumerate() {
            let anchors = jget(fixture, "anchors");
            let Some(anchors) = anchors.as_array() else {
                continue;
            };
            for (j, anchor) in anchors.iter().enumerate() {
                let part = pack_str(anchor, "part");
                if !part.is_empty() && !part_set.contains(&part) {
                    findings.push(Finding::new(
                        TG_SCENE_UNKNOWN_PART,
                        "error",
                        format!("fixtures[{i}].anchors[{j}].part"),
                        format!("anchor part '{part}' is not a node of the project"),
                    ));
                }
            }
        }
    }
    let props = jget(scene, "props");
    if let Some(props) = props.as_array() {
        for (i, prop) in props.iter().enumerate() {
            let hooks = jget(prop, "hooks");
            let Some(hooks) = hooks.as_array() else { continue };
            for (j, hook) in hooks.iter().enumerate() {
                let param = pack_str(hook, "parameter");
                if !param.is_empty() && !param_set.contains(&param) {
                    findings.push(Finding::new(
                        TG_SCENE_UNKNOWN_PARAM,
                        "error",
                        format!("props[{i}].hooks[{j}].parameter"),
                        format!("hook parameter '{param}' does not exist on the project"),
                    ));
                }
            }
        }
    }
    let fluids = jget(scene, "fluids");
    if let Some(fluids) = fluids.as_array() {
        for (i, emitter) in fluids.iter().enumerate() {
            let part = pack_str(&jget(emitter, "source"), "part");
            if !part.is_empty() && !part_set.contains(&part) {
                findings.push(Finding::new(
                    TG_SCENE_UNKNOWN_PART,
                    "error",
                    format!("fluids[{i}].source.part"),
                    format!("emitter source part '{part}' is not a node of the project"),
                ));
            }
        }
    }
    let fx = jget(scene, "effects");
    if fx.is_object() {
        for f in validate_effects(fx, Some(&param_names)) {
            if f.code == TG_FX_BINDING {
                findings.push(Finding::new(
                    f.code,
                    f.severity,
                    format!("effects.{}", f.subject),
                    f.message.clone(),
                ));
            }
        }
    }
    let required = jget(&jget(pack, "consentModel"), "requiredFields");
    let characters = jget(scene, "characters");
    if let Some(characters) = characters.as_array() {
        for (i, entry) in characters.iter().enumerate() {
            let at = format!("characters[{i}]");
            let consent = jget(entry, "consent");
            if !consent.is_object() {
                findings.push(Finding::new(
                    TG_SCENE_CONSENT,
                    "error",
                    format!("{at}.consent"),
                    "placed character carries no consent record",
                ));
                continue;
            }
            if let Some(required) = required.as_object() {
                for (field, want) in required {
                    let got = jget(consent, field);
                    let ok = match want {
                        Value::Bool(true) => got == &Value::Bool(true),
                        Value::String(w) => got.as_str() == Some(w.as_str()),
                        _ => false,
                    };
                    if !ok {
                        findings.push(Finding::new(
                            TG_SCENE_CONSENT,
                            "error",
                            format!("{at}.consent.{field}"),
                            format!(
                                "consent record fails the pack consent model: field '{field}' must be {}",
                                torogao_format::jsonfmt::to_d_json(want)
                            ),
                        ));
                    }
                }
            }
        }
    }
    findings
}

pub fn scene_face_expression(scene: &Value) -> String {
    pack_str(&jget(scene, "reactionProfile"), "faceContact")
}

pub fn scene_info(scene: &Value) -> Value {
    let fx = jget(scene, "effects");
    json!({
        "characters": count_of(&jget(scene, "characters")),
        "effects": if fx.is_object() { count_of(&jget(fx, "effects")) } else { 0 },
        "fixtures": count_of(&jget(scene, "fixtures")),
        "fluidEmitters": count_of(&jget(scene, "fluids")),
        "format": pack_str(scene, "format"),
        "name": pack_str(scene, "name"),
        "pack": pack_str(scene, "pack"),
        "plates": count_of(&jget(&jget(scene, "background"), "plates")),
        "props": count_of(&jget(scene, "props")),
    })
}
