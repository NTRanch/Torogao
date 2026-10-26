//! Playroom sessions and preference sets, ported from the
//! D `torogao.cli.playroom` module.

use serde_json::{Map, Value, json};

use torogao_engine::Puppet;

use crate::controllers::resolve_controllers;
use crate::extdoc::ExtView;
use crate::fluids::{hit_areas_from_ext, simulate_fluid};
use crate::packs::pack_str;
use crate::paths::{find_data_file, playroom_sets_dir};
use crate::preview::{PreviewOptions, run_physics_preview};
use crate::scenes::{load_scene_doc, scene_face_expression};
use crate::util::jget;
use crate::walk::Walk;
use torogao_format::jsonfmt::to_d_json;

pub const PLAYROOM_FORMAT: &str = "torogao-playroom/1";
pub const PREFSET_FORMAT: &str = "torogao-preference-set/1";
pub const TG_PLAYROOM_FORMAT: &str = "TG-PLAYROOM-FORMAT";
pub const TG_PREFSET_FORMAT: &str = "TG-PREFSET-FORMAT";
pub const TG_WARDROBE_UNKNOWN_ARTICLE: &str = "TG-WARDROBE-UNKNOWN-ARTICLE";
pub const TG_WARDROBE_UNKNOWN_PART: &str = "TG-WARDROBE-UNKNOWN-PART";
pub const WARDROBE_FORMAT: &str = "torogao-wardrobe/1";

pub fn vocal_modes() -> [&'static str; 4] {
    ["moans", "screams", "dirty-talk", "stifled"]
}

pub fn new_session(project: &str, pack: &str, scene: &str, activity: &str) -> Value {
    json!({
        "activity": activity,
        "character": project,
        "controllers": {},
        "drivers": true,
        "expressionState": "neutral",
        "format": PLAYROOM_FORMAT,
        "pack": pack,
        "physicsTuning": {},
        "scene": scene,
        "variant": "",
        "vocalLane": {"intensity": 0.5, "mode": "moans"},
    })
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

pub fn save_session(path: &str, session: &Value) -> Result<(), String> {
    let dir = parent_dir(path);
    if !dir.is_empty() && !std::path::Path::new(&dir).exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, format!("{}\n", to_d_json(session))).map_err(|e| e.to_string())
}

pub fn write_session_direct(path: &str, session: &Value) -> Result<(), String> {
    std::fs::write(path, format!("{}\n", to_d_json(session))).map_err(|e| e.to_string())
}

pub fn load_session(path: &str) -> Result<Value, String> {
    if !std::path::Path::new(path).exists() {
        return Err(format!("playroom session not found: {path}"));
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("playroom session unreadable: {e}"))?;
    let session: Value =
        serde_json::from_str(&text).map_err(|e| format!("playroom session unreadable: {e}"))?;
    if pack_str(&session, "format") != PLAYROOM_FORMAT {
        return Err(format!(
            "file is not a {PLAYROOM_FORMAT} session ({TG_PLAYROOM_FORMAT})"
        ));
    }
    Ok(session)
}

pub fn session_fields() -> [&'static str; 7] {
    [
        "scene",
        "activity",
        "expressionState",
        "variant",
        "vocalMode",
        "vocalIntensity",
        "drivers",
    ]
}

pub fn apply_session_field(session: &mut Value, field: &str, value: &str) -> Option<String> {
    match field {
        "scene" | "activity" | "expressionState" | "variant" => {
            session[field] = json!(value);
            None
        }
        "drivers" => {
            if value != "true" && value != "false" {
                return Some("drivers takes true or false".to_string());
            }
            session["drivers"] = json!(value == "true");
            None
        }
        "vocalMode" => {
            if !vocal_modes().contains(&value) {
                return Some(format!(
                    "vocal mode '{value}' is not one of {}",
                    vocal_modes().join(", ")
                ));
            }
            let lane = jget(session, "vocalLane").clone();
            let mut lane = if lane.is_object() {
                lane
            } else {
                json!({"intensity": 0.5})
            };
            lane["mode"] = json!(value);
            session["vocalLane"] = lane;
            None
        }
        "vocalIntensity" => {
            let Ok(v) = value.parse::<f64>() else {
                return Some("vocal intensity is not a number".to_string());
            };
            if v < 0.0 || v > 1.0 {
                return Some("vocal intensity must be in [0, 1]".to_string());
            }
            let lane = jget(session, "vocalLane").clone();
            let mut lane = if lane.is_object() {
                lane
            } else {
                json!({"mode": "moans"})
            };
            lane["intensity"] = json!(v);
            session["vocalLane"] = lane;
            None
        }
        _ => Some(format!(
            "unknown session field '{field}' (settable: {}; controller values go through prefset/session documents, not field edits)",
            session_fields().join(", ")
        )),
    }
}

pub fn prefset_dir(explicit: &str) -> String {
    if !explicit.is_empty() {
        return explicit.to_string();
    }
    playroom_sets_dir().unwrap_or_default()
}

pub fn prefset_path(dir: &str, name: &str) -> String {
    format!("{dir}/{name}.json")
}

pub fn list_prefsets(dir: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if dir.is_empty() {
        return out;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".json") {
            out.push(stem.to_string());
        }
    }
    out.sort();
    out
}

pub fn load_prefset(dir: &str, name: &str) -> Result<Value, String> {
    let path = prefset_path(dir, name);
    if !std::path::Path::new(&path).exists() {
        return Err(format!("preference set '{name}' not found"));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("preference set '{name}' is unreadable: {e}"))?;
    let set: Value = serde_json::from_str(&text)
        .map_err(|e| format!("preference set '{name}' is unreadable: {e}"))?;
    if pack_str(&set, "format") != PREFSET_FORMAT {
        return Err(format!(
            "file is not a {PREFSET_FORMAT} set ({TG_PREFSET_FORMAT})"
        ));
    }
    Ok(set)
}

pub fn load_wardrobe(pack: &str) -> Result<Value, String> {
    let Some(path) = find_data_file(
        "TOROGAO_PACKS_DIR",
        "packs",
        &format!("{pack}-wardrobe.json"),
    ) else {
        return Err(format!("no wardrobe ships with pack '{pack}'"));
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("wardrobe for pack '{pack}' is unreadable: {e}"))?;
    let wardrobe: Value = serde_json::from_str(&text)
        .map_err(|e| format!("wardrobe for pack '{pack}' is unreadable: {e}"))?;
    if pack_str(&wardrobe, "format") != WARDROBE_FORMAT {
        return Err(format!("wardrobe file is not {WARDROBE_FORMAT}"));
    }
    Ok(wardrobe)
}

pub fn compose_playroom(
    session: &Value,
    puppet: &mut Puppet,
    walk: &Walk,
    ext_view: &ExtView,
    steps: i32,
) -> Result<Value, String> {
    let _ = walk;
    let track = json!({
        "format": "torogao-input-track/1",
        "fps": 60,
        "inputs": {},
        "steps": steps,
    });
    let mut options = PreviewOptions {
        ext: ext_view.doc.clone(),
        state_name: pack_str(session, "expressionState"),
        variant_id: pack_str(session, "variant"),
        ..Default::default()
    };
    if jget(session, "drivers") == &Value::Bool(true) {
        options.has_drivers = true;
    }
    let controllers_doc = json!({"values": jget(session, "controllers")});
    let (resolved, problems) = resolve_controllers(&ext_view.doc, &controllers_doc);
    if !problems.is_empty() {
        return Err(problems[0].clone());
    }
    if !resolved.is_empty() {
        options.has_controllers = true;
        options.controller_values = resolved;
    }
    let character = pack_str(session, "character");
    let curves = run_physics_preview(puppet, &track, 0, &character, &options)?;
    let mut snapshot = json!({
        "activity": pack_str(session, "activity"),
        "character": character,
        "expressionState": options.state_name,
        "format": "torogao-playroom-snapshot/1",
        "scene": pack_str(session, "scene"),
        "variant": options.variant_id,
        "vocalLane": jget(session, "vocalLane").clone(),
    });
    let mut fired: Vec<String> = Vec::new();
    if !options.state_name.is_empty() {
        fired.push(options.state_name.clone());
    }
    let scene_name = pack_str(session, "scene");
    if !scene_name.is_empty() {
        if let Ok(scene) = load_scene_doc(&scene_name) {
            let face_expr = scene_face_expression(&scene);
            let areas = hit_areas_from_ext(&ext_view.doc);
            let mut runs: Vec<Value> = Vec::new();
            let fluids = jget(&scene, "fluids").clone();
            if let Some(emitters) = fluids.as_array() {
                for emitter in emitters {
                    let run = simulate_fluid(emitter, &areas, f64::from(steps) / 60.0, &face_expr);
                    if let Some(contacts) = jget(&run, "faceContacts").as_array() {
                        for contact in contacts {
                            let name = pack_str(contact, "expression");
                            if !name.is_empty() && !fired.contains(&name) {
                                fired.push(name);
                            }
                        }
                    }
                    runs.push(run);
                }
            }
            snapshot["fluidRuns"] = Value::Array(runs);
        }
    }
    snapshot["expressionsFired"] =
        Value::Array(fired.iter().map(|f| Value::String(f.clone())).collect());
    snapshot["series"] = json!(curves.series.len() as i64);
    snapshot["steps"] = json!(curves.steps());
    Ok(snapshot)
}

pub fn apply_prefset(
    session: &mut Value,
    set: &Value,
    wardrobe: &Value,
    part_names: &[String],
) -> Option<String> {
    let wardrobe_states = jget(set, "wardrobe").clone();
    if let Some(states) = wardrobe_states.as_array() {
        for entry in states {
            let article = pack_str(entry, "article");
            let state = pack_str(entry, "state");
            if state != "worn" && state != "displaced" && state != "removed" {
                return Some(format!(
                    "wardrobe state for '{article}' must be worn, displaced or removed"
                ));
            }
            if wardrobe.is_object() {
                let mut definition: Option<&Value> = None;
                if let Some(articles) = jget(wardrobe, "articles").as_array() {
                    for candidate in articles {
                        if pack_str(candidate, "id") == article {
                            definition = Some(candidate);
                            break;
                        }
                    }
                }
                let Some(definition) = definition else {
                    return Some(format!(
                        "{TG_WARDROBE_UNKNOWN_ARTICLE}: set names article '{article}', which the pack wardrobe does not define"
                    ));
                };
                if let Some(parts) = jget(definition, "parts").as_array() {
                    for part in parts {
                        let Some(part) = part.as_str() else { continue };
                        if !part_names.iter().any(|pn| pn == part) {
                            return Some(format!(
                                "{TG_WARDROBE_UNKNOWN_PART}: article '{article}' binds part '{part}', which the character does not have"
                            ));
                        }
                    }
                }
            }
        }
        session["wardrobe"] = Value::Array(states.clone());
    }
    let morphs = jget(set, "morphs");
    if morphs.is_object() {
        session["morphs"] = morphs.clone();
    }
    let appearance = jget(set, "appearance");
    if appearance.is_object() {
        session["appearance"] = appearance.clone();
    }
    let controllers = jget(set, "controllers");
    if let Some(controllers) = controllers.as_object() {
        let existing = jget(session, "controllers").clone();
        let mut target: Map<String, Value> = match existing.as_object() {
            Some(m) => m.clone(),
            None => Map::new(),
        };
        for (key, value) in controllers {
            target.insert(key.clone(), value.clone());
        }
        session["controllers"] = Value::Object(target);
    }
    None
}
