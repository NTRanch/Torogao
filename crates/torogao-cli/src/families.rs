//! Tool-family dispatch for the non-core registry tools,
//! plus the shared layer helpers the CLI tail uses
//! (viseme baking, ambient driver configs). Each family
//! lands here as its port completes; a tool with no
//! ported handler yet fails as `internal_error`, never
//! as a fabricated success.

use crate::creator::create_character;
use crate::curves::CurveSet;
use crate::drivers::parse_driver_config as parse_driver_set;
use crate::extdoc::{
    expression_missing, ext_summary, rating_stamp, rating_verdict, tracking_coverage,
    validate_hit_areas_only, write_project_with_ext, ExtView, EXT_FORMAT, TG_EXT_MALFORMED,
};
use crate::guards::{guard_output, load_puppet_guarded, tool_force};
use crate::handlers::load_for_tool;
use crate::packs::{load_pack, pack_names, pack_str, pack_tracking_set};
use crate::preview::{run_physics_preview, PreviewOptions};
use crate::registry::{opt_string_arg, string_arg, ToolResult};
use crate::util::jget;
use crate::writer::{
    build_scaffold, load_json_file, load_sfx_index, validate_voice, ScaffoldOutcome,
};
use serde_json::{json, Value};
use torogao_format::container::TOROGAO_EXT_SECTION;
use torogao_format::jsonfmt::to_d_json;

/// How a CLI layer file can fail: a usage-class problem
/// (stderr, exit 3) or a bake failure (error document,
/// exit 3).
pub enum LayerError {
    /// Usage-class failure; message goes to stderr.
    Usage(String),
    /// Bake failure; message goes into an error document.
    Bake(String),
}

/// Loads a viseme contract and bakes a viseme track into
/// curves (the CLI tail's --viseme layer).
pub fn bake_viseme_layer(
    viseme_path: &str,
    contract_path: &str,
) -> Result<CurveSet, LayerError> {
    let contract = crate::viseme::load_viseme_contract(contract_path)
        .map_err(LayerError::Usage)?;
    let track: Value = std::fs::read_to_string(viseme_path)
        .map_err(|e| LayerError::Usage(format!("viseme track unreadable: {e}")))
        .and_then(|text| {
            serde_json::from_str(&text)
                .map_err(|e| LayerError::Usage(format!("viseme track unreadable: {e}")))
        })?;
    crate::viseme::bake_visemes(&track, &contract).map_err(LayerError::Bake)
}

/// Parses an ambient driver configuration document.
pub fn parse_driver_config(doc: &Value) -> Result<Value, String> {
    parse_driver_set(doc)?;
    Ok(doc.clone())
}

/// The pack argument, loaded (pack_not_found on failure).
fn load_pack_arg(args: &Value) -> Result<Value, ToolResult> {
    let name = string_arg(args, "pack")?;
    load_pack(&name).map_err(|e| ToolResult::fail("pack_not_found", &e))
}

/// A JSON file argument, loaded (file_unreadable on
/// failure).
fn load_json_arg(args: &Value, key: &str) -> Result<Value, ToolResult> {
    let path = string_arg(args, key)?;
    load_json_file(&path).map_err(|e| ToolResult::fail("file_unreadable", &e))
}

// ------------------------------------------------------------
// P3 tools (p3tools.d)

fn tool_pack_info(args: &Value) -> ToolResult {
    let pack = match load_pack_arg(args) {
        Ok(pack) => pack,
        Err(error) => return error,
    };
    ToolResult::ok(json!({
        "pack": pack,
        "packs": pack_names(),
    }))
}

fn tool_rating_check(args: &Value) -> ToolResult {
    let load = match load_for_tool(args) {
        Ok(load) => load,
        Err(error) => return error,
    };
    let view = load.ext_view();
    let mut doc = json!({ "file": load.path });
    if view.malformed {
        doc["stamp"] = rating_stamp(&view.doc);
        doc["result"] = json!("malformed");
        doc["code"] = json!(TG_EXT_MALFORMED);
        doc["error"] = json!("the torogao EXT section is not parseable JSON");
        return ToolResult::ok(doc);
    }
    let verdict = rating_verdict(&view);
    doc["stamp"] = verdict.stamp;
    if !verdict.complete {
        doc["result"] = json!("missing");
        doc["code"] = json!(verdict.code);
        doc["error"] = json!(verdict.reason);
    } else if verdict.blocked {
        doc["result"] = json!("mismatch");
        doc["code"] = json!(verdict.code);
        doc["error"] = json!(verdict.reason);
    } else {
        doc["result"] = json!("ok");
    }
    ToolResult::ok(doc)
}

fn tool_list_states(args: &Value) -> ToolResult {
    let load = match load_for_tool(args) {
        Ok(load) => load,
        Err(error) => return error,
    };
    ToolResult::ok(json!({
        "extension": ext_summary(&load.ext_view().doc),
        "file": load.path,
    }))
}

fn tool_apply_state(args: &Value) -> ToolResult {
    let mut load = match load_for_tool(args) {
        Ok(load) => load,
        Err(error) => return error,
    };
    let track_path = match string_arg(args, "track") {
        Ok(path) => path,
        Err(error) => return error,
    };
    let state_name = opt_string_arg(args, "state");
    let variant_id = opt_string_arg(args, "variant");
    if state_name.is_empty() && variant_id.is_empty() {
        return ToolResult::fail(
            "invalid_arguments",
            "one of 'state' or 'variant' is required",
        );
    }
    if !std::path::Path::new(&track_path).exists() {
        return ToolResult::fail(
            "track_not_found",
            &format!("input track file not found: {track_path}"),
        );
    }
    let track: Value = match std::fs::read_to_string(&track_path)
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
    {
        Ok(track) => track,
        Err(message) => {
            return ToolResult::fail(
                "track_unreadable",
                &format!("input track is not valid JSON: {message}"),
            )
        }
    };
    let options = PreviewOptions {
        ext: load.ext_view().doc,
        state_name,
        variant_id,
        ..Default::default()
    };
    let label = load.path.clone();
    match run_physics_preview(&mut load.loaded.puppet, &track, 0, &label, &options) {
        Ok(curves) => ToolResult::ok(curves.to_json_value()),
        Err(message) => ToolResult::fail("preview_failed", &message),
    }
}

fn tool_expression_check(args: &Value) -> ToolResult {
    let load = match load_for_tool(args) {
        Ok(load) => load,
        Err(error) => return error,
    };
    let pack = match load_pack_arg(args) {
        Ok(pack) => pack,
        Err(error) => return error,
    };
    let missing = expression_missing(&load.ext_view().doc, &pack);
    let mut doc = json!({
        "file": load.path,
        "missing": missing,
        "pack": pack_str(&pack, "name"),
        "result": if missing.is_empty() { "covered" } else { "findings" },
    });
    if !missing.is_empty() {
        doc["code"] = json!(crate::extdoc::TG_EXPR_MISSING);
    }
    ToolResult::ok(doc)
}

fn tool_tracking_check(args: &Value) -> ToolResult {
    let load = match load_for_tool(args) {
        Ok(load) => load,
        Err(error) => return error,
    };
    let pack = match load_pack_arg(args) {
        Ok(pack) => pack,
        Err(error) => return error,
    };
    let set_id = match string_arg(args, "set") {
        Ok(id) => id,
        Err(error) => return error,
    };
    let set = pack_tracking_set(&pack, &set_id);
    if !set.is_object() {
        return ToolResult::fail(
            "set_not_found",
            &format!(
                "pack '{}' has no tracking set '{}'",
                pack_str(&pack, "name"),
                set_id
            ),
        );
    }
    let (missing, range_bad) = tracking_coverage(&load.loaded.puppet, &set);
    let ok = missing.is_empty() && range_bad.is_empty();
    let mut doc = json!({
        "completeness": pack_str(&set, "completeness"),
        "file": load.path,
        "missing": missing,
        "pack": pack_str(&pack, "name"),
        "rangeViolations": range_bad,
        "result": if ok { "covered" } else { "findings" },
        "set": set_id,
    });
    if !missing.is_empty() {
        doc["code"] = json!(crate::extdoc::TG_TRACK_MISSING);
    } else if !range_bad.is_empty() {
        doc["code"] = json!(crate::extdoc::TG_TRACK_RANGE);
    }
    ToolResult::ok(doc)
}

fn tool_hitareas_list(args: &Value) -> ToolResult {
    let load = match load_for_tool(args) {
        Ok(load) => load,
        Err(error) => return error,
    };
    let view = load.ext_view();
    let areas = if view.present {
        jget(jget(&view.doc, "consumer"), "hitAreas").clone()
    } else {
        Value::Null
    };
    ToolResult::ok(json!({
        "file": load.path,
        "hitAreas": areas,
    }))
}

fn tool_hitareas_author(args: &Value) -> ToolResult {
    let load = match load_for_tool(args) {
        Ok(load) => load,
        Err(error) => return error,
    };
    let spec = match load_json_arg(args, "spec") {
        Ok(spec) => spec,
        Err(error) => return error,
    };
    let out_path = match string_arg(args, "out") {
        Ok(path) => path,
        Err(error) => return error,
    };
    let spec_path = string_arg(args, "spec").unwrap_or_default();
    if let Err(refusal) = guard_output(
        &out_path,
        &[load.path.as_str(), spec_path.as_str()],
        tool_force(args),
    ) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    let mut areas = jget(&spec, "hitAreas").clone();
    if !areas.is_array() {
        areas = spec.clone();
    }
    if !areas.is_array() {
        return ToolResult::fail(
            "invalid_arguments",
            "spec must be an array or an object with a hitAreas array",
        );
    }
    let view = load.ext_view();
    let mut ext = if view.present { view.doc.clone() } else { Value::Null };
    if !ext.is_object() {
        ext = json!({ "format": EXT_FORMAT });
    }
    let mut consumer = jget(&ext, "consumer").clone();
    if !consumer.is_object() {
        consumer = json!({});
    }
    consumer["hitAreas"] = areas.clone();
    ext["consumer"] = consumer;
    let problems = validate_hit_areas_only(&ext, &load.loaded.puppet, &load.walk);
    if !problems.is_empty() {
        return ToolResult::ok(json!({
            "findings": problems.iter().map(|f| f.to_json()).collect::<Vec<_>>(),
            "result": "refused",
        }));
    }
    if let Some(parent) = std::path::Path::new(&out_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    if let Err(message) = write_project_with_ext(&load.path, &out_path, &ext) {
        return ToolResult::fail("io_error", &message);
    }
    ToolResult::ok(json!({
        "hitAreas": areas.as_array().map_or(0, Vec::len),
        "out": out_path,
        "result": "authored",
    }))
}

fn tool_create_character(args: &Value) -> ToolResult {
    let identity = match load_json_arg(args, "identity") {
        Ok(doc) => doc,
        Err(error) => return error,
    };
    let pack = match load_pack_arg(args) {
        Ok(pack) => pack,
        Err(error) => return error,
    };
    let out_path = match string_arg(args, "out") {
        Ok(path) => path,
        Err(error) => return error,
    };
    let identity_path = string_arg(args, "identity").unwrap_or_default();
    if let Err(refusal) = guard_output(&out_path, &[identity_path.as_str()], tool_force(args)) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    let mut presets: Vec<String> = Vec::new();
    if let Some(list) = args.get("presets") {
        let Some(entries) = list.as_array() else {
            return ToolResult::fail(
                "invalid_arguments",
                "argument 'presets' must be an array of names",
            );
        };
        for entry in entries {
            let Some(name) = entry.as_str() else {
                return ToolResult::fail(
                    "invalid_arguments",
                    "argument 'presets' must be an array of names",
                );
            };
            presets.push(name.to_string());
        }
    }
    match create_character(&identity, &pack, &presets, &out_path) {
        Ok(summary) => ToolResult::ok(summary),
        Err((problems, message)) => {
            if !problems.is_empty() {
                return ToolResult::ok(json!({
                    "problems": problems.iter().map(|p| p.to_json()).collect::<Vec<_>>(),
                    "result": "refused",
                }));
            }
            ToolResult::fail("create_failed", &message)
        }
    }
}

fn tool_writer_scaffold(args: &Value) -> ToolResult {
    let character = match load_json_arg(args, "character") {
        Ok(doc) => doc,
        Err(error) => return error,
    };
    let pack = match load_pack_arg(args) {
        Ok(pack) => pack,
        Err(error) => return error,
    };
    let kink = match string_arg(args, "kink") {
        Ok(kink) => kink,
        Err(error) => return error,
    };
    let mut stage: i64 = -1;
    if let Some(value) = args.get("stage") {
        if let Some(n) = value.as_i64() {
            stage = n;
        } else if let Some(u) = value.as_u64() {
            stage = u as i64;
        }
    }
    if stage < 0 {
        return ToolResult::fail(
            "invalid_arguments",
            "argument 'stage' (non-negative integer) is required",
        );
    }
    let mut sfx_ids: Vec<String> = Vec::new();
    if let Some(list) = args.get("sfx") {
        let Some(entries) = list.as_array() else {
            return ToolResult::fail(
                "invalid_arguments",
                "argument 'sfx' must be an array of SFX entry ids",
            );
        };
        for entry in entries {
            let Some(id) = entry.as_str() else {
                return ToolResult::fail(
                    "invalid_arguments",
                    "argument 'sfx' must be an array of SFX entry ids",
                );
            };
            sfx_ids.push(id.to_string());
        }
    }
    let doc = match build_scaffold(&character, &pack, &kink, stage, &sfx_ids) {
        ScaffoldOutcome::Scaffold(doc) | ScaffoldOutcome::Gap(doc) => doc,
    };
    let out_path = opt_string_arg(args, "out");
    if !out_path.is_empty() {
        let char_path = string_arg(args, "character").unwrap_or_default();
        if let Err(refusal) = guard_output(&out_path, &[char_path.as_str()], tool_force(args)) {
            return ToolResult::fail(refusal.code, &refusal.message);
        }
        if let Some(parent) = std::path::Path::new(&out_path).parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let _ = std::fs::write(&out_path, format!("{}\n", to_d_json(&doc)));
        return ToolResult::ok(json!({
            "document": doc,
            "out": out_path,
        }));
    }
    ToolResult::ok(doc)
}

fn tool_voice_check(args: &Value) -> ToolResult {
    let record = match load_json_arg(args, "record") {
        Ok(doc) => doc,
        Err(error) => return error,
    };
    let findings = validate_voice(&record);
    ToolResult::ok(json!({
        "findings": findings.iter().map(|f| f.to_json()).collect::<Vec<_>>(),
        "result": if findings.is_empty() { "clean" } else { "findings" },
    }))
}

fn tool_sfx_list(_args: &Value) -> ToolResult {
    match load_sfx_index() {
        Ok(index) => ToolResult::ok(index),
        Err(message) => ToolResult::fail("sfx_unavailable", &message),
    }
}

// ------------------------------------------------------------
// physics-preview tool (physicstools.d)

fn tool_physics_preview(args: &Value) -> ToolResult {
    if !args.is_object() {
        return ToolResult::fail("invalid_arguments", "arguments must be a JSON object");
    }
    let project_path = match string_arg(args, "project") {
        Ok(path) => path,
        Err(error) => return error,
    };
    let track_path = match string_arg(args, "track") {
        Ok(path) => path,
        Err(error) => return error,
    };
    let mut fps: i32 = 0;
    if let Some(field) = args.get("fps") {
        if let Some(n) = field.as_i64() {
            fps = n as i32;
        } else if let Some(u) = field.as_u64() {
            fps = u as i32;
        } else {
            return ToolResult::fail(
                "invalid_arguments",
                "argument 'fps' must be an integer",
            );
        }
    }
    if !std::path::Path::new(&project_path).exists() {
        return ToolResult::fail(
            "project_not_found",
            &format!("project file not found: {project_path}"),
        );
    }
    if !std::path::Path::new(&track_path).exists() {
        return ToolResult::fail(
            "track_not_found",
            &format!("input track file not found: {track_path}"),
        );
    }
    let track: Value = match std::fs::read_to_string(&track_path)
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
    {
        Ok(track) => track,
        Err(message) => {
            return ToolResult::fail(
                "track_unreadable",
                &format!("input track is not valid JSON: {message}"),
            )
        }
    };
    let mut puppet = match load_puppet_guarded(&project_path) {
        Ok(puppet) => puppet,
        Err(e) => return ToolResult::fail(e.code(), e.message()),
    };
    let ext_doc = ExtView::of_section(puppet.ext_section(TOROGAO_EXT_SECTION)).doc;
    let mut options = PreviewOptions {
        ext: ext_doc,
        state_name: opt_string_arg(args, "state"),
        variant_id: opt_string_arg(args, "variant"),
        ..Default::default()
    };
    if let Some(driver_doc) = args.get("drivers") {
        if let Err(message) = parse_driver_set(driver_doc) {
            return ToolResult::fail("invalid_arguments", &message);
        }
        options.has_drivers = true;
        options.drivers = driver_doc.clone();
    }
    if let Some(controller_arg) = args.get("controllers") {
        let wrapped = json!({
            "format": "torogao-controller-values/1",
            "values": controller_arg.clone(),
        });
        let (resolved, problems) =
            crate::controllers::resolve_controllers(&options.ext, &wrapped);
        if let Some(first) = problems.first() {
            return ToolResult::fail("invalid_arguments", first);
        }
        options.has_controllers = true;
        options.controller_values = resolved;
    }
    if let Some(override_arg) = args.get("override") {
        let Some(override_path) = override_arg.as_str() else {
            return ToolResult::fail(
                "invalid_arguments",
                "argument 'override' must be a curves file path",
            );
        };
        let doc: Value = match std::fs::read_to_string(override_path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
        {
            Ok(doc) => doc,
            Err(message) => {
                return ToolResult::fail(
                    "override_unreadable",
                    &format!("override take is not valid JSON: {message}"),
                )
            }
        };
        options.has_override = true;
        options.override_curves = doc;
    }
    match run_physics_preview(&mut puppet, &track, fps, &project_path, &options) {
        Ok(curves) => ToolResult::ok(curves.to_json_value()),
        Err(message) => ToolResult::fail("preview_failed", &message),
    }
}

// ------------------------------------------------------------
// Audio tools (audiotools.d)

fn load_recipe_arg(path: &str) -> Result<(Value, String), ToolResult> {
    if !std::path::Path::new(path).exists() {
        return Err(ToolResult::fail(
            "recipe_not_found",
            &format!("recipe file not found: {path}"),
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|e| {
        ToolResult::fail("recipe_unreadable", &format!("recipe is not valid JSON: {e}"))
    })?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| {
        ToolResult::fail("recipe_unreadable", &format!("recipe is not valid JSON: {e}"))
    })?;
    Ok((doc, text))
}

fn mkdir_parent(path: &str) {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
}

fn tool_audio_validate(args: &Value) -> ToolResult {
    let path = match string_arg(args, "recipe") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (recipe, _) = match load_recipe_arg(&path) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let findings = crate::audioproc::validate_recipe(&recipe);
    let has_error = findings.iter().any(|f| f.severity == "error");
    ToolResult::ok(json!({
        "file": path,
        "findings": crate::criteria::findings_json(&findings),
        "result": if has_error { "findings" } else { "clean" },
    }))
}

fn tool_audio_render(args: &Value) -> ToolResult {
    let path = match string_arg(args, "recipe") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let out_dir = match string_arg(args, "out") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (recipe, text) = match load_recipe_arg(&path) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let name_v = jget(&recipe, "name");
    let rname = match name_v.as_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "untitled".to_string(),
    };
    let files = [format!("{rname}.wav"), format!("{rname}.manifest.json")];
    if let Err(refusal) = crate::guards::guard_output_files(
        &out_dir,
        &[files[0].as_str(), files[1].as_str()],
        &[path.as_str()],
        tool_force(args),
    ) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    let _ = std::fs::create_dir_all(&out_dir);
    let result = crate::audioproc::render_recipe(&recipe, &text, &out_dir);
    if !result.error.is_empty() {
        return ToolResult::fail("render_failed", &result.error);
    }
    ToolResult::ok(result.manifest)
}

fn tool_audio_bake(args: &Value) -> ToolResult {
    let wav_path = match string_arg(args, "wav") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let recipe_path = match string_arg(args, "recipe") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let out_path = match string_arg(args, "out") {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !std::path::Path::new(&wav_path).exists() {
        return ToolResult::fail(
            "wav_not_found",
            &format!("WAV file not found: {wav_path}"),
        );
    }
    let (recipe, _) = match load_recipe_arg(&recipe_path) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let wav = match crate::wav::read_wav_file(&wav_path) {
        Ok(w) => w,
        Err(e) => return ToolResult::fail("wav_unreadable", &e),
    };
    if let Err(refusal) = guard_output(
        &out_path,
        &[wav_path.as_str(), recipe_path.as_str()],
        tool_force(args),
    ) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    let curves = match crate::audiobake::bake_envelope(&wav, &recipe, &wav_path) {
        Ok(c) => c,
        Err(e) => return ToolResult::fail("bake_failed", &e),
    };
    mkdir_parent(&out_path);
    let text = curves.to_json();
    if let Err(e) = std::fs::write(&out_path, format!("{text}\n")) {
        return ToolResult::fail("io_error", &e.to_string());
    }
    ToolResult::ok(json!({
        "curves": out_path,
        "file": wav_path,
        "series": curves.series.len(),
        "steps": curves.steps(),
    }))
}

// ------------------------------------------------------------
// P4 tools (p4tools.d)

fn tool_tutorial_list(args: &Value) -> ToolResult {
    let pack_name = match string_arg(args, "pack") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut track = opt_string_arg(args, "track");
    if track.is_empty() {
        track = "characters".to_string();
    }
    match crate::tutorials::load_lessons(&pack_name, &track) {
        Ok(lessons) => ToolResult::ok(crate::tutorials::lessons_summary(&lessons)),
        Err(e) => ToolResult::fail(crate::tutorials::TG_TUTORIAL_NO_LESSONS, &e),
    }
}

fn tool_tutorial_status(args: &Value) -> ToolResult {
    let mut progress_path = opt_string_arg(args, "progress");
    if progress_path.is_empty() {
        progress_path = crate::paths::default_tutorial_progress_path().unwrap_or_default();
    }
    let progress = match crate::tutorials::load_progress(&progress_path) {
        Ok(p) => p,
        Err(e) => return ToolResult::fail("progress_unreadable", &e),
    };
    let pack_name = opt_string_arg(args, "pack");
    let mut lessons_doc = Value::Null;
    if !pack_name.is_empty() {
        match crate::tutorials::load_lessons(&pack_name, "characters") {
            Ok(d) => lessons_doc = d,
            Err(e) => {
                return ToolResult::fail(crate::tutorials::TG_TUTORIAL_NO_LESSONS, &e)
            }
        }
    }
    let mut doc = crate::tutorials::progress_summary(&progress, &lessons_doc);
    doc["progressFile"] = json!(progress_path);
    ToolResult::ok(doc)
}

fn tool_tutorial_check(args: &Value) -> ToolResult {
    let pack_name = match string_arg(args, "pack") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let lesson_id = match string_arg(args, "lesson") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let project = match string_arg(args, "project") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut track = opt_string_arg(args, "track");
    if track.is_empty() {
        track = "characters".to_string();
    }
    let lessons = match crate::tutorials::load_lessons(&pack_name, &track) {
        Ok(l) => l,
        Err(e) => return ToolResult::fail(crate::tutorials::TG_TUTORIAL_NO_LESSONS, &e),
    };
    let lesson = crate::tutorials::find_lesson(&lessons, &lesson_id);
    if !lesson.is_object() {
        return ToolResult::fail(
            crate::tutorials::TG_TUTORIAL_LESSON_UNKNOWN,
            &format!("pack '{pack_name}' has no lesson '{lesson_id}'"),
        );
    }
    let pack = match load_pack(&pack_name) {
        Ok(p) => p,
        Err(e) => return ToolResult::fail("pack_unavailable", &e),
    };
    let mut load = match load_for_tool(&json!({ "project": project })) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let view = load.ext_view();
    let slot_count = load.loaded.puppet.texture_slots.len();
    let outcome = crate::tutorials::check_lesson(
        &mut load.loaded.puppet,
        &mut load.walk,
        &load.payload.clone(),
        slot_count,
        &pack,
        &jget(&lesson, "check"),
        &view,
    );
    let mut doc = json!({
        "evidence": outcome.evidence,
        "lesson": lesson_id,
        "pack": pack_name,
        "passed": outcome.passed,
        "project": project,
        "track": pack_str(&lessons, "track"),
    });
    if !outcome.passed {
        doc["code"] = json!(outcome.code);
    }
    if outcome.passed {
        let mut progress_path = opt_string_arg(args, "progress");
        if progress_path.is_empty() {
            progress_path = crate::paths::default_tutorial_progress_path().unwrap_or_default();
        }
        let mut progress = match crate::tutorials::load_progress(&progress_path) {
            Ok(p) => p,
            Err(e) => return ToolResult::fail("progress_unreadable", &e),
        };
        let is_new = crate::tutorials::record_completion(
            &mut progress,
            &pack_name,
            &pack_str(&lessons, "track"),
            &lesson,
            &project,
        );
        if let Err(e) = crate::tutorials::save_progress(&progress_path, &progress) {
            return ToolResult::fail("progress_unwritable", &e);
        }
        doc["recorded"] = json!(is_new);
        doc["progressFile"] = json!(progress_path);
        doc["xpTotal"] = jget(&progress, "xpTotal").clone();
    }
    ToolResult::ok(doc)
}

fn tool_writer_draft(args: &Value) -> ToolResult {
    let char_path = match string_arg(args, "character") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let pack_name = match string_arg(args, "pack") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let kink = match string_arg(args, "kink") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let lane_path = match string_arg(args, "lane") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let out_path = match string_arg(args, "out") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let stage = match args.get("stage").and_then(|v| v.as_i64()) {
        Some(s) => s,
        None => {
            return ToolResult::fail(
                "invalid_arguments",
                "argument 'stage' (integer) is required",
            )
        }
    };
    let mut sfx_ids: Vec<String> = Vec::new();
    if let Some(sv) = args.get("sfx") {
        let Some(arr) = sv.as_array() else {
            return ToolResult::fail(
                "invalid_arguments",
                "argument 'sfx' must be an array of strings",
            );
        };
        for s in arr {
            let Some(text) = s.as_str() else {
                return ToolResult::fail(
                    "invalid_arguments",
                    "argument 'sfx' must be an array of strings",
                );
            };
            sfx_ids.push(text.to_string());
        }
    }
    let lane = match crate::writerlane::load_lane_config(&lane_path) {
        Ok(l) => l,
        Err(e) => return ToolResult::fail(crate::writerlane::TG_WRITER_LANE, &e),
    };
    let pack = match load_pack(&pack_name) {
        Ok(p) => p,
        Err(e) => return ToolResult::fail("pack_unavailable", &e),
    };
    let character = match load_json_file(&char_path) {
        Ok(c) => c,
        Err(e) => return ToolResult::fail("character_unreadable", &e),
    };
    if let Err(refusal) = guard_output(
        &out_path,
        &[char_path.as_str(), lane_path.as_str()],
        tool_force(args),
    ) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    let doc = match crate::writerlane::run_writer_lane(
        &character, &pack, &kink, stage, &sfx_ids, &lane,
    ) {
        crate::writerlane::LaneOutcome::Draft(d) => d,
        crate::writerlane::LaneOutcome::Gap(d) => d,
        crate::writerlane::LaneOutcome::Refusal(d) => d,
        crate::writerlane::LaneOutcome::Error(e) => {
            return ToolResult::fail(crate::writerlane::TG_WRITER_LANE, &e)
        }
    };
    mkdir_parent(&out_path);
    let text = to_d_json(&doc);
    if let Err(e) = std::fs::write(&out_path, format!("{text}\n")) {
        return ToolResult::fail("io_error", &e.to_string());
    }
    ToolResult::ok(doc)
}

fn tool_viseme_author(args: &Value) -> ToolResult {
    let spec_path = match string_arg(args, "spec") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let out_path = match string_arg(args, "out") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let contract = match crate::viseme::load_viseme_contract(&opt_string_arg(args, "contract")) {
        Ok(c) => c,
        Err(e) => return ToolResult::fail("contract_unavailable", &e),
    };
    let spec: Value = match std::fs::read_to_string(&spec_path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => return ToolResult::fail("spec_unreadable", &e),
    };
    if let Err(refusal) = guard_output(&out_path, &[spec_path.as_str()], tool_force(args)) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    let track = match crate::viseme::author_viseme_track(&spec, &contract) {
        Ok(t) => t,
        Err(e) => return ToolResult::fail("TG-VISEME-AUTHOR", &e),
    };
    mkdir_parent(&out_path);
    let text = to_d_json(&track);
    if let Err(e) = std::fs::write(&out_path, format!("{text}\n")) {
        return ToolResult::fail("io_error", &e.to_string());
    }
    ToolResult::ok(json!({
        "durationSeconds": jget(&track, "durationSeconds"),
        "events": jget(&track, "events").as_array().map_or(0, Vec::len),
        "spec": spec_path,
        "track": out_path,
    }))
}

fn tool_viseme_validate(args: &Value) -> ToolResult {
    let track_path = match string_arg(args, "track") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let project = match string_arg(args, "project") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let contract = match crate::viseme::load_viseme_contract(&opt_string_arg(args, "contract")) {
        Ok(c) => c,
        Err(e) => return ToolResult::fail("contract_unavailable", &e),
    };
    let track: Value = match std::fs::read_to_string(&track_path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => return ToolResult::fail("track_unreadable", &e),
    };
    let load = match load_for_tool(&json!({ "project": project })) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let mut findings = crate::viseme::validate_viseme_track(&track, &contract);
    findings.extend(crate::viseme::check_visemes(&load.loaded.puppet, &contract));
    ToolResult::ok(json!({
        "findings": crate::criteria::findings_json(&findings),
        "project": project,
        "result": if findings.is_empty() { "clean" } else { "findings" },
        "track": track_path,
    }))
}

fn tool_session_validate(args: &Value) -> ToolResult {
    let session_path = match string_arg(args, "session") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let session: Value = match std::fs::read_to_string(&session_path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => return ToolResult::fail("session_unreadable", &e),
    };
    let findings = crate::sessionbuild::validate_session(&session);
    ToolResult::ok(json!({
        "file": session_path,
        "findings": crate::criteria::findings_json(&findings),
        "result": if findings.is_empty() { "clean" } else { "findings" },
    }))
}

fn tool_session_build(args: &Value) -> ToolResult {
    let session_path = match string_arg(args, "session") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let out_dir = match string_arg(args, "out") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let session: Value = match std::fs::read_to_string(&session_path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => return ToolResult::fail("session_unreadable", &e),
    };
    for f in crate::sessionbuild::validate_session(&session) {
        if f.severity == "error" {
            return ToolResult::fail(f.code, &f.message);
        }
    }
    let recipe = match crate::sessionbuild::build_session_recipe(&session) {
        Ok(r) => r,
        Err(e) => return ToolResult::fail("session_build_failed", &e),
    };
    let name = pack_str(&session, "name");
    let files = [
        format!("{name}.recipe.json"),
        format!("{name}.wav"),
        format!("{name}.manifest.json"),
    ];
    if let Err(refusal) = crate::guards::guard_output_files(
        &out_dir,
        &[files[0].as_str(), files[1].as_str(), files[2].as_str()],
        &[session_path.as_str()],
        tool_force(args),
    ) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    let _ = std::fs::create_dir_all(&out_dir);
    let recipe_text = to_d_json(&recipe);
    let recipe_path = format!("{out_dir}/{name}.recipe.json");
    if let Err(e) = std::fs::write(&recipe_path, format!("{recipe_text}\n")) {
        return ToolResult::fail("io_error", &e.to_string());
    }
    let result = crate::audioproc::render_recipe(&recipe, &recipe_text, &out_dir);
    if !result.error.is_empty() {
        return ToolResult::fail("session_render_failed", &result.error);
    }
    ToolResult::ok(json!({
        "manifest": result.manifest,
        "recipe": recipe_path,
        "session": session_path,
    }))
}

fn tool_events_list(args: &Value) -> ToolResult {
    let project = match string_arg(args, "project") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let load = match load_for_tool(&json!({ "project": project })) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let view = load.ext_view();
    ToolResult::ok(json!({
        "file": project,
        "motionEvents": if view.present {
            jget(&jget(&view.doc, "consumer"), "motionEvents").clone()
        } else {
            Value::Null
        },
    }))
}

fn tool_events_author(args: &Value) -> ToolResult {
    let project = match string_arg(args, "project") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let spec_path = match string_arg(args, "spec") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let out_path = match string_arg(args, "out") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let spec = match load_json_file(&spec_path) {
        Ok(s) => s,
        Err(e) => return ToolResult::fail("spec_unreadable", &e),
    };
    let load = match load_for_tool(&json!({ "project": project })) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let view = load.ext_view();
    let mut ext = if view.present {
        view.doc.clone()
    } else {
        Value::Null
    };
    if !ext.is_object() {
        ext = json!({ "format": EXT_FORMAT });
    }
    let mut consumer = jget(&ext, "consumer").clone();
    if !consumer.is_object() {
        consumer = json!({});
    }
    let mut events = jget(&spec, "motionEvents").clone();
    if !events.is_array() {
        events = spec.clone();
    }
    if !events.is_array() {
        return ToolResult::fail(
            "invalid_arguments",
            "spec must be an array or carry a motionEvents array",
        );
    }
    consumer["motionEvents"] = events.clone();
    ext["consumer"] = consumer;
    let probe = crate::extdoc::validate_motion_events_only(&ext, &load.loaded.puppet);
    if !probe.is_empty() {
        return ToolResult::ok(json!({
            "findings": crate::criteria::findings_json(&probe),
            "result": "refused",
        }));
    }
    if let Err(refusal) = guard_output(
        &out_path,
        &[project.as_str(), spec_path.as_str()],
        tool_force(args),
    ) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    if let Err(e) = write_project_with_ext(&project, &out_path, &ext) {
        return ToolResult::fail("project_write_failed", &e);
    }
    ToolResult::ok(json!({
        "motionEvents": events.as_array().map_or(0, Vec::len),
        "out": out_path,
        "result": "authored",
    }))
}

fn tool_lipsync_analyze(args: &Value) -> ToolResult {
    let wav_path = match string_arg(args, "wav") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let out_path = match string_arg(args, "out") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let contract = match crate::viseme::load_viseme_contract(&opt_string_arg(args, "contract")) {
        Ok(c) => c,
        Err(e) => return ToolResult::fail("contract_unavailable", &e),
    };
    if let Err(refusal) = guard_output(&out_path, &[wav_path.as_str()], tool_force(args)) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    let proposal = match crate::lipsync::analyze_lipsync(&wav_path, &contract) {
        Ok(p) => p,
        Err(e) => return ToolResult::fail(crate::lipsync::TG_LIPSYNC_INPUT, &e),
    };
    mkdir_parent(&out_path);
    let text = to_d_json(&proposal);
    if let Err(e) = std::fs::write(&out_path, format!("{text}\n")) {
        return ToolResult::fail("io_error", &e.to_string());
    }
    ToolResult::ok(json!({
        "durationSeconds": jget(&proposal, "durationSeconds"),
        "events": jget(&proposal, "features").as_array().map_or(0, Vec::len),
        "proposal": out_path,
        "wav": wav_path,
    }))
}

// ------------------------------------------------------------
// Viseme tools (visemetools.d, core namespace)

fn tool_viseme_check(args: &Value) -> ToolResult {
    let project = match string_arg(args, "project") {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !std::path::Path::new(&project).exists() {
        return ToolResult::fail(
            "project_not_found",
            &format!("project file not found: {project}"),
        );
    }
    let contract = match crate::viseme::load_viseme_contract(&opt_string_arg(args, "contract")) {
        Ok(c) => c,
        Err(e) => return ToolResult::fail("contract_unavailable", &e),
    };
    let puppet = match load_puppet_guarded(&project) {
        Ok(p) => p,
        Err(e) => return ToolResult::fail(e.code(), e.message()),
    };
    let findings = crate::viseme::check_visemes(&puppet, &contract);
    let covered = crate::viseme::covered_parameters(&puppet, &contract);
    let missing: Vec<String> = contract
        .parameters
        .iter()
        .filter(|p| !covered.contains(p))
        .cloned()
        .collect();
    ToolResult::ok(json!({
        "contract": contract.set_name,
        "covered": covered,
        "file": project,
        "findings": crate::criteria::findings_json(&findings),
        "missing": missing,
        "result": if findings.is_empty() { "clean" } else { "findings" },
    }))
}

fn tool_viseme_bake(args: &Value) -> ToolResult {
    let track_path = match string_arg(args, "track") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let out_path = match string_arg(args, "out") {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !std::path::Path::new(&track_path).exists() {
        return ToolResult::fail(
            "track_not_found",
            &format!("track file not found: {track_path}"),
        );
    }
    let contract = match crate::viseme::load_viseme_contract(&opt_string_arg(args, "contract")) {
        Ok(c) => c,
        Err(e) => return ToolResult::fail("contract_unavailable", &e),
    };
    let track: Value = match std::fs::read_to_string(&track_path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => {
            return ToolResult::fail(
                "track_unreadable",
                &format!("track is not valid JSON: {e}"),
            )
        }
    };
    if let Err(refusal) = guard_output(&out_path, &[track_path.as_str()], tool_force(args)) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    let curves = match crate::viseme::bake_visemes(&track, &contract) {
        Ok(c) => c,
        Err(e) => return ToolResult::fail("bake_failed", &e),
    };
    mkdir_parent(&out_path);
    let text = curves.to_json();
    if let Err(e) = std::fs::write(&out_path, format!("{text}\n")) {
        return ToolResult::fail("io_error", &e.to_string());
    }
    ToolResult::ok(json!({
        "curves": out_path,
        "series": curves.series.len(),
        "steps": curves.steps(),
        "track": track_path,
    }))
}

// ------------------------------------------------------------
// Template tools (templatools.d)

fn tool_list_templates(_args: &Value) -> ToolResult {
    let templates: Vec<Value> = crate::templates::list_templates()
        .iter()
        .map(|t| {
            json!({
                "description": t.description,
                "kind": t.kind,
                "name": t.name,
                "title": t.title,
            })
        })
        .collect();
    ToolResult::ok(json!({ "templates": templates }))
}

fn tool_instantiate_template(args: &Value) -> ToolResult {
    let template = match string_arg(args, "template") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let out_path = match string_arg(args, "out") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut presets: Vec<String> = Vec::new();
    if let Some(pv) = args.get("presets") {
        let Some(arr) = pv.as_array() else {
            return ToolResult::fail(
                "invalid_arguments",
                "argument 'presets' must be an array of names",
            );
        };
        for p in arr {
            let Some(text) = p.as_str() else {
                return ToolResult::fail(
                    "invalid_arguments",
                    "argument 'presets' must be an array of names",
                );
            };
            presets.push(text.to_string());
        }
    }
    if let Err(refusal) = guard_output(&out_path, &[], tool_force(args)) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    match crate::templates::instantiate_template(&template, &presets, &out_path) {
        Ok(summary) => ToolResult::ok(summary),
        Err(e) => ToolResult::fail("instantiate_failed", &e),
    }
}

/// Dispatches a non-core tool by name.
pub fn dispatch_family(name: &str, args: &Value) -> ToolResult {
    match name {
        "apply-state" => tool_apply_state(args),
        "audio-bake" => tool_audio_bake(args),
        "audio-render" => tool_audio_render(args),
        "audio-validate" => tool_audio_validate(args),
        "create-character" => tool_create_character(args),
        "events-author" => tool_events_author(args),
        "events-list" => tool_events_list(args),
        "expression-check" => tool_expression_check(args),
        "hitareas-author" => tool_hitareas_author(args),
        "hitareas-list" => tool_hitareas_list(args),
        "instantiate-template" => tool_instantiate_template(args),
        "lipsync-analyze" => tool_lipsync_analyze(args),
        "list-states" => tool_list_states(args),
        "list-templates" => tool_list_templates(args),
        "pack-info" => tool_pack_info(args),
        "physics-preview" => tool_physics_preview(args),
        "rating-check" => tool_rating_check(args),
        "session-build" => tool_session_build(args),
        "session-validate" => tool_session_validate(args),
        "sfx-list" => tool_sfx_list(args),
        "tracking-check" => tool_tracking_check(args),
        "tutorial-check" => tool_tutorial_check(args),
        "tutorial-list" => tool_tutorial_list(args),
        "tutorial-status" => tool_tutorial_status(args),
        "viseme-author" => tool_viseme_author(args),
        "viseme-bake" => tool_viseme_bake(args),
        "viseme-check" => tool_viseme_check(args),
        "viseme-validate" => tool_viseme_validate(args),
        "voice-check" => tool_voice_check(args),
        "writer-draft" => tool_writer_draft(args),
        "writer-scaffold" => tool_writer_scaffold(args),
        _ => ToolResult::fail(
            "internal_error",
            &format!("tool '{name}' is not ported yet"),
        ),
    }
}
