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
    _viseme_path: &str,
    _contract_path: &str,
) -> Result<CurveSet, LayerError> {
    Err(LayerError::Bake(
        "viseme baking is not ported yet".to_string(),
    ))
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

/// Dispatches a non-core tool by name.
pub fn dispatch_family(name: &str, args: &Value) -> ToolResult {
    match name {
        "apply-state" => tool_apply_state(args),
        "create-character" => tool_create_character(args),
        "expression-check" => tool_expression_check(args),
        "hitareas-author" => tool_hitareas_author(args),
        "hitareas-list" => tool_hitareas_list(args),
        "list-states" => tool_list_states(args),
        "pack-info" => tool_pack_info(args),
        "physics-preview" => tool_physics_preview(args),
        "rating-check" => tool_rating_check(args),
        "sfx-list" => tool_sfx_list(args),
        "tracking-check" => tool_tracking_check(args),
        "voice-check" => tool_voice_check(args),
        "writer-scaffold" => tool_writer_scaffold(args),
        _ => ToolResult::fail(
            "internal_error",
            &format!("tool '{name}' is not ported yet"),
        ),
    }
}
