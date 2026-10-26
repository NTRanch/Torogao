//! Tutorial lessons, machine checks and the progress store,
//! ported from the D `torogao.cli.tutorials` module.

use serde_json::{Map, Value, json};

use torogao_engine::Puppet;

use crate::criteria::validate_puppet;
use crate::extdoc::{ExtView, expression_missing, tracking_coverage, validate_ext};
use crate::packs::{pack_str, pack_tracking_set};
use crate::paths::find_data_file;
use crate::preview::{PreviewOptions, run_physics_preview};
use crate::rigexport::RigExporter;
use crate::util::{fmt_g, jget};
use crate::walk::Walk;
use crate::wav::sha256_hex;

pub const LESSONS_FORMAT: &str = "torogao-lessons/1";
pub const PROGRESS_FORMAT: &str = "torogao-progress/1";
pub const TG_TUTORIAL_CHECK_FAILED: &str = "TG-TUTORIAL-CHECK-FAILED";
pub const TG_TUTORIAL_NO_LESSONS: &str = "TG-TUTORIAL-NO-LESSONS";
pub const TG_TUTORIAL_LESSON_UNKNOWN: &str = "TG-TUTORIAL-LESSON-UNKNOWN";

// -- Lessons data -------------------------------------------------

pub fn load_lessons(pack_name: &str, track: &str) -> Result<Value, String> {
    let mut path = find_data_file(
        "TOROGAO_PACKS_DIR",
        "packs",
        &format!("{pack_name}-lessons-{track}.json"),
    );
    if path.is_none() && track == "characters" {
        path = find_data_file(
            "TOROGAO_PACKS_DIR",
            "packs",
            &format!("{pack_name}-lessons.json"),
        );
    }
    let Some(path) = path else {
        return Err(format!("no lessons file for pack '{pack_name}'"));
    };
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("lessons file unreadable: {e}"))?;
    let doc: Value =
        serde_json::from_str(&text).map_err(|e| format!("lessons file unreadable: {e}"))?;
    if pack_str(&doc, "format") != LESSONS_FORMAT {
        return Err("lessons file format must be \"torogao-lessons/1\"".to_string());
    }
    Ok(doc)
}

pub fn lesson_list(lessons_doc: &Value) -> Vec<Value> {
    jget(lessons_doc, "lessons")
        .as_array()
        .cloned()
        .unwrap_or_default()
}

pub fn find_lesson(lessons_doc: &Value, id: &str) -> Value {
    for lesson in lesson_list(lessons_doc) {
        if pack_str(&lesson, "id") == id {
            return lesson;
        }
    }
    Value::Null
}

pub fn lessons_summary(lessons_doc: &Value) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    for lesson in lesson_list(lessons_doc) {
        let mut row = Map::new();
        row.insert("id".to_string(), json!(pack_str(&lesson, "id")));
        row.insert("title".to_string(), json!(pack_str(&lesson, "title")));
        row.insert("level".to_string(), json!(pack_str(&lesson, "level")));
        row.insert("xp".to_string(), jget(&lesson, "xp").clone());
        row.insert(
            "checkKind".to_string(),
            json!(pack_str(jget(&lesson, "check"), "kind")),
        );
        rows.push(Value::Object(row));
    }
    let mut doc = Map::new();
    doc.insert("format".to_string(), jget(lessons_doc, "format").clone());
    doc.insert("pack".to_string(), json!(pack_str(lessons_doc, "pack")));
    doc.insert("track".to_string(), json!(pack_str(lessons_doc, "track")));
    doc.insert("title".to_string(), json!(pack_str(lessons_doc, "title")));
    doc.insert("lessons".to_string(), Value::Array(rows));
    Value::Object(doc)
}

// -- Checks ---------------------------------------------------------

pub struct LessonCheckResult {
    pub passed: bool,
    pub code: String,
    pub evidence: Vec<String>,
}

fn fail(kind: &str, mut evidence: Vec<String>) -> LessonCheckResult {
    evidence.push(format!("check '{kind}' did not pass"));
    LessonCheckResult {
        passed: false,
        code: TG_TUTORIAL_CHECK_FAILED.to_string(),
        evidence,
    }
}

fn consent_recorded(ext: &Value) -> bool {
    let consent = jget(jget(ext, "character"), "consent");
    jget(consent, "adult") == &Value::Bool(true)
        && jget(consent, "wantsSelectedScenes") == &Value::Bool(true)
}

fn validates_evidence(
    puppet: &Puppet,
    walk: &Walk,
    payload: &Value,
    slot_count: usize,
) -> (Vec<String>, bool) {
    let mut findings = validate_puppet(puppet, walk, payload, slot_count);
    findings.extend(validate_ext(puppet, walk, payload));
    let errors = findings.iter().filter(|f| f.severity == "error").count();
    (
        vec![format!("{} findings, {} errors", findings.len(), errors)],
        errors == 0,
    )
}

fn consent_evidence(ext: &Value) -> (Vec<String>, bool) {
    let ok = consent_recorded(ext);
    (
        vec![if ok {
            "consent record present: adult and wantsSelectedScenes both true".to_string()
        } else {
            "consent record missing or incomplete".to_string()
        }],
        ok,
    )
}

fn expression_evidence(ext: &ExtView, pack: &Value) -> (Vec<String>, bool) {
    let missing = expression_missing(&ext.doc, pack);
    let mut ev = vec![format!(
        "{} governed expressions unimplemented",
        missing.len()
    )];
    for m in &missing {
        ev.push(format!("missing: {m}"));
    }
    (ev, missing.is_empty())
}

fn export_evidence(puppet: &Puppet, walk: &mut Walk, slot_count: usize) -> (Vec<String>, bool) {
    // RP1-D1: the port's exporter emits the "torogao-rig-model"
    // token; the check tests for the port token and names it.
    match RigExporter::run(puppet, walk, slot_count) {
        Ok(result) => {
            let fmt = jget(&result.model, "format");
            let ok = fmt.as_str() == Some(torogao_format::MODEL_FORMAT_TOKEN);
            (
                vec![format!(
                    "export built a {} model ({} conversion entries)",
                    if ok {
                        torogao_format::MODEL_FORMAT_TOKEN
                    } else {
                        "unexpected"
                    },
                    result.entries.len()
                )],
                ok,
            )
        }
        Err(msg) => (vec![format!("export failed: {msg}")], false),
    }
}

pub fn scene_has_feature(scene: &Value, feature: &str) -> bool {
    if feature == "ambient" {
        return jget(scene, "ambient").is_object();
    }
    if let Some(kind) = feature.strip_prefix("fixture:") {
        if let Some(list) = jget(scene, "fixtures").as_array() {
            return list.iter().any(|e| pack_str(e, "kind") == kind);
        }
        return false;
    }
    if let Some(kind) = feature.strip_prefix("prop:") {
        if let Some(list) = jget(scene, "props").as_array() {
            return list.iter().any(|e| pack_str(e, "kind") == kind);
        }
        return false;
    }
    if let Some(kind) = feature.strip_prefix("effect:") {
        if let Some(list) = jget(jget(scene, "effects"), "effects").as_array() {
            return list.iter().any(|e| pack_str(e, "kind") == kind);
        }
        return false;
    }
    if let Some(behavior) = feature.strip_prefix("fluid:") {
        if let Some(list) = jget(scene, "fluids").as_array() {
            return list.iter().any(|e| pack_str(e, "behavior") == behavior);
        }
        return false;
    }
    false
}

fn scene_validates_evidence(
    check: &Value,
    puppet: &Puppet,
    walk: &Walk,
    pack: &Value,
) -> (Vec<String>, bool) {
    let scene_name = pack_str(check, "scene");
    let scene = match crate::scenes::load_scene_doc(&scene_name) {
        Ok(s) => s,
        Err(e) => {
            return (
                vec![format!("scene '{scene_name}' could not be loaded: {e}")],
                false,
            );
        }
    };
    let mut findings = crate::scenes::validate_scene(&scene);
    findings.extend(crate::scenes::validate_scene_grounded(
        &scene, puppet, walk, pack,
    ));
    let mut errors = 0usize;
    let mut ev: Vec<String> = Vec::new();
    for f in &findings {
        if f.severity == "error" {
            errors += 1;
            ev.push(format!("{}: {}", f.code, f.message));
        }
    }
    let mut out = vec![format!(
        "scene '{scene_name}': {} findings, {} errors",
        findings.len(),
        errors
    )];
    out.extend(ev);
    (out, errors == 0)
}

fn scene_features_evidence(
    check: &Value,
    puppet: &Puppet,
    walk: &Walk,
    pack: &Value,
    capstone: bool,
) -> (Vec<String>, bool) {
    let (mut ev, mut ok) = scene_validates_evidence(check, puppet, walk, pack);
    if !ok {
        return (ev, ok);
    }
    let scene_name = pack_str(check, "scene");
    let scene = match crate::scenes::load_scene_doc(&scene_name) {
        Ok(s) => s,
        Err(e) => {
            ev.push(format!("scene vanished mid-check: {e}"));
            return (ev, false);
        }
    };
    let features = jget(check, "features");
    let Some(features) = features.as_array() else {
        ev.push("the lesson names no required features".to_string());
        return (ev, false);
    };
    for f in features {
        let Some(name) = f.as_str() else { continue };
        let present = scene_has_feature(&scene, name);
        ev.push(format!(
            "{}: {name}",
            if present {
                "feature present"
            } else {
                "feature MISSING"
            }
        ));
        if !present {
            ok = false;
        }
    }
    if capstone {
        ev.push(
            if ok {
                "scene capstone complete"
            } else {
                "scene capstone incomplete"
            }
            .to_string(),
        );
    }
    (ev, ok)
}

/// Runs one lesson's machine check against a loaded project.
pub fn check_lesson(
    puppet: &mut Puppet,
    walk: &mut Walk,
    payload: &Value,
    slot_count: usize,
    pack: &Value,
    check: &Value,
    ext: &ExtView,
) -> LessonCheckResult {
    let kind = pack_str(check, "kind");
    let ext_doc = &ext.doc;
    let (evidence, ok): (Vec<String>, bool) = match kind.as_str() {
        "project-validates" => validates_evidence(puppet, walk, payload, slot_count),
        "consent-recorded" => consent_evidence(ext_doc),
        "expression-covered" => expression_evidence(ext, pack),
        "export-builds" => export_evidence(puppet, walk, slot_count),
        "state-applied" => {
            let wanted = pack_str(check, "state");
            let mut state_name = wanted.clone();
            if wanted == "*" {
                let states = jget(ext_doc, "states");
                state_name = match states.as_array().filter(|a| !a.is_empty()) {
                    Some(arr) => pack_str(&arr[0], "name"),
                    None => String::new(),
                };
            }
            if state_name.is_empty() {
                return fail(
                    &kind,
                    vec!["the project stores no state to apply".to_string()],
                );
            }
            let mut track = Map::new();
            track.insert("format".to_string(), json!("torogao-input-track/1"));
            track.insert("steps".to_string(), json!(30));
            track.insert("inputs".to_string(), Value::Object(Map::new()));
            let track = Value::Object(track);
            let options = PreviewOptions {
                ext: ext_doc.clone(),
                state_name: state_name.clone(),
                ..Default::default()
            };
            match run_physics_preview(puppet, &track, 0, "tutorial-check", &options) {
                Ok(_) => (
                    vec![format!("state '{state_name}' composes in the preview")],
                    true,
                ),
                Err(e) => (
                    vec![format!("state '{state_name}' failed to compose: {e}")],
                    false,
                ),
            }
        }
        "tracking-covered" => {
            let set_id = pack_str(check, "set");
            let set = pack_tracking_set(pack, &set_id);
            if !set.is_object() {
                return fail(
                    &kind,
                    vec![format!("pack carries no tracking set '{set_id}'")],
                );
            }
            let (missing, range_bad) = tracking_coverage(puppet, &set);
            let ids = jget(&set, "ids");
            let total = ids.as_array().map_or(0, Vec::len);
            let covered = total - missing.len();
            let mut min_ratio = 1.0f64;
            let mr = jget(check, "minRatio");
            if let Value::Number(n) = &mr {
                min_ratio = n.as_f64().unwrap_or(min_ratio);
            }
            let ok =
                range_bad.is_empty() && total > 0 && (covered as f64 / total as f64) >= min_ratio;
            let mut ev = vec![format!(
                "set '{}': {}/{} ids covered, {} range violations, required ratio {}",
                set_id,
                covered,
                total,
                range_bad.len(),
                fmt_g(min_ratio)
            )];
            for m in &missing {
                ev.push(format!("missing: {m}"));
            }
            for b in &range_bad {
                ev.push(format!("range: {b}"));
            }
            (ev, ok)
        }
        "scene-validates" => scene_validates_evidence(check, puppet, walk, pack),
        "scene-features" => scene_features_evidence(check, puppet, walk, pack, false),
        "scene-capstone" => scene_features_evidence(check, puppet, walk, pack, true),
        "capstone" => {
            let (mut ev, v_ok) = validates_evidence(puppet, walk, payload, slot_count);
            let (ev2, c_ok) = consent_evidence(ext_doc);
            ev.extend(ev2);
            let (ev3, e_ok) = expression_evidence(ext, pack);
            ev.extend(ev3);
            let (ev4, x_ok) = export_evidence(puppet, walk, slot_count);
            ev.extend(ev4);
            (ev, v_ok && c_ok && e_ok && x_ok)
        }
        _ => return fail(&kind, vec![format!("unknown check kind '{kind}'")]),
    };
    if ok {
        LessonCheckResult {
            passed: true,
            code: String::new(),
            evidence,
        }
    } else {
        LessonCheckResult {
            passed: false,
            code: TG_TUTORIAL_CHECK_FAILED.to_string(),
            evidence,
        }
    }
}

// -- Progress store -------------------------------------------------

pub fn fresh_progress() -> Value {
    let mut doc = Map::new();
    doc.insert("format".to_string(), json!(PROGRESS_FORMAT));
    doc.insert("seq".to_string(), json!(0));
    doc.insert("xpTotal".to_string(), json!(0));
    doc.insert("completed".to_string(), Value::Object(Map::new()));
    Value::Object(doc)
}

pub fn load_progress(path: &str) -> Result<Value, String> {
    if path.is_empty() || !std::path::Path::new(path).exists() {
        return Ok(fresh_progress());
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("progress file unreadable: {e}"))?;
    let doc: Value =
        serde_json::from_str(&text).map_err(|e| format!("progress file unreadable: {e}"))?;
    if pack_str(&doc, "format") != PROGRESS_FORMAT {
        return Err(format!(
            "progress file format must be \"{PROGRESS_FORMAT}\""
        ));
    }
    Ok(doc)
}

pub fn save_progress(path: &str, progress: &Value) -> Result<(), String> {
    let parent = std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !parent.is_empty() {
        std::fs::create_dir_all(&parent)
            .map_err(|e| format!("cannot create progress directory: {e}"))?;
    }
    let tmp = format!("{path}.tmp");
    let text = torogao_format::jsonfmt::to_d_json(progress);
    std::fs::write(&tmp, format!("{text}\n")).map_err(|e| format!("cannot write progress: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot write progress: {e}"))?;
    Ok(())
}

fn progress_int(v: &Value, key: &str) -> i64 {
    match &jget(v, key) {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i
            } else if let Some(u) = n.as_u64() {
                u as i64
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// Records a completion; returns true when it is new.
pub fn record_completion(
    progress: &mut Value,
    pack_name: &str,
    track: &str,
    lesson: &Value,
    project_path: &str,
) -> bool {
    let key = format!("{pack_name}/{track}/{}", pack_str(lesson, "id"));
    let completed = jget(progress, "completed");
    if !completed.is_object() {
        if let Some(obj) = progress.as_object_mut() {
            obj.insert("completed".to_string(), Value::Object(Map::new()));
        }
    }
    if jget(jget(progress, "completed"), &key).is_object() {
        return false;
    }
    let seq = progress_int(progress, "seq") + 1;
    let xp = progress_int(progress, "xpTotal") + progress_int(lesson, "xp");
    let bytes = std::fs::read(project_path).unwrap_or_default();
    let mut entry = Map::new();
    entry.insert("seq".to_string(), json!(seq));
    entry.insert("xp".to_string(), jget(lesson, "xp").clone());
    entry.insert("projectSha256".to_string(), json!(sha256_hex(&bytes)));
    if let Some(obj) = progress.as_object_mut() {
        if let Some(completed) = obj.get_mut("completed").and_then(|c| c.as_object_mut()) {
            completed.insert(key, Value::Object(entry));
        }
        obj.insert("seq".to_string(), json!(seq));
        obj.insert("xpTotal".to_string(), json!(xp));
    }
    true
}

pub fn progress_summary(progress: &Value, lessons_doc: &Value) -> Value {
    let mut rows: Vec<(String, Value)> = Vec::new();
    let completed = jget(progress, "completed");
    if let Some(obj) = completed.as_object() {
        for (key, entry) in obj {
            let mut row = Map::new();
            row.insert("lesson".to_string(), json!(key));
            row.insert("xp".to_string(), jget(entry, "xp").clone());
            rows.push((key.clone(), Value::Object(row)));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut doc = Map::new();
    doc.insert("format".to_string(), jget(progress, "format").clone());
    doc.insert("xpTotal".to_string(), jget(progress, "xpTotal").clone());
    doc.insert("completions".to_string(), jget(progress, "seq").clone());
    doc.insert(
        "completed".to_string(),
        Value::Array(rows.into_iter().map(|(_, r)| r).collect()),
    );
    if lessons_doc.is_object() {
        let pack_name = pack_str(lessons_doc, "pack");
        let track = pack_str(lessons_doc, "track");
        let completed_obj = jget(progress, "completed");
        let mut remaining = 0usize;
        for lesson in lesson_list(lessons_doc) {
            let key = format!("{pack_name}/{track}/{}", pack_str(&lesson, "id"));
            if !jget(completed_obj, &key).is_object() {
                remaining += 1;
            }
        }
        doc.insert("pack".to_string(), json!(pack_name));
        doc.insert("track".to_string(), json!(track));
        doc.insert("lessonsRemaining".to_string(), json!(remaining));
    }
    Value::Object(doc)
}
