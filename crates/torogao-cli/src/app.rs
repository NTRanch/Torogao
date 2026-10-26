//! The headless CLI (app.d): subcommand dispatch.
//!
//! Exit codes: 0 success (validate: no findings); 1
//! validate findings; 2 project could not be loaded; 3
//! usage error; 4 export failed after load. Export-class
//! subcommands pass the output guard first; project loads
//! are guarded at this boundary — a malformed container
//! is a typed TG-CONTAINER-MALFORMED result, never a raw
//! exception dump.

use crate::criteria::findings_json;
use crate::extdoc::{
    ExtView, TG_EXT_MALFORMED, count_glue_keys, expression_missing, ext_summary,
    rating_blocks_export, rating_stamp, rating_verdict, tracking_coverage,
};
use crate::guards::{self, TG_CONTAINER_MALFORMED, load_project, payload_guarded};
use crate::handlers::{ToolLoad, base_stem, join_path, validate_findings};
use crate::inspect::{census, project_info_doc};
use crate::packs::{load_pack, pack_str, pack_tracking_set};
use crate::paths::{
    PathKind, base_dir, default_playroom_session_path, default_tutorial_progress_path,
    playroom_sets_dir,
};
use crate::registry;
use crate::util::jget;
use crate::walk::Walk;
use serde_json::Value;
use sha2::Digest;
use std::io::Write;
use torogao_format::jsonfmt::to_d_json;

/// Prints a document as one JSON line.
fn writeln_doc(out: &mut dyn Write, doc: &Value) {
    let _ = writeln!(out, "{}", to_d_json(doc));
}

/// The reference's `emit`: the document is written to the
/// --out file (when named) AND printed.
fn emit(out: &mut dyn Write, doc: &Value, out_path: &str) {
    let text = to_d_json(doc);
    if !out_path.is_empty() {
        let _ = std::fs::write(out_path, format!("{text}\n"));
    }
    let _ = writeln!(out, "{text}");
}

/// Runs one CLI invocation.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    // Agent-surface modes take no project argument.
    if args.first().map(String::as_str) == Some("mcp") {
        return crate::mcp::run_mcp_server();
    }
    if args.first().map(String::as_str) == Some("registry-dump") {
        writeln_doc(out, &registry::dump_json());
        return 0;
    }
    if args.first().map(String::as_str) == Some("a2a") {
        return run_a2a_command(args, err);
    }
    // The P5 subcommand family intercepts next.
    if let Some(code) = crate::specials::run_p5_cli(args, out, err) {
        return code;
    }
    if let Some(code) = crate::specials::run_special(args, out, err) {
        return code;
    }
    match args.first().map(String::as_str) {
        Some("paths") => return run_paths(out),
        Some("project-info") => return run_project_info(args, out, err),
        _ => {}
    }
    run_tail(args, out, err)
}

fn run_a2a_command(args: &[String], err: &mut dyn Write) -> i32 {
    let mut bind = "127.0.0.1".to_string();
    let mut port: u16 = crate::a2a::A2A_DEFAULT_PORT;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--port" && i + 1 < args.len() {
            i += 1;
            match args[i].parse::<u16>() {
                Ok(parsed) => port = parsed,
                Err(_) => {
                    let _ = writeln!(err, "a2a: --port needs a number");
                    return 3;
                }
            }
        } else if args[i] == "--bind" && i + 1 < args.len() {
            i += 1;
            bind = args[i].clone();
        } else {
            let _ = writeln!(err, "a2a: unknown argument: {}", args[i]);
            return 3;
        }
        i += 1;
    }
    crate::a2a::run_a2a_server(&bind, port)
}

fn run_paths(out: &mut dyn Write) -> i32 {
    let or_null = |value: Option<String>| value.map_or(Value::Null, Value::String);
    let config = base_dir(PathKind::Config);
    let doc = serde_json::json!({
        "cache": or_null(base_dir(PathKind::Cache)),
        "config": or_null(config.clone()),
        "data": or_null(base_dir(PathKind::Data)),
        "playroomSession": or_null(default_playroom_session_path()),
        "playroomSets": or_null(playroom_sets_dir()),
        "state": or_null(base_dir(PathKind::State)),
        "tutorialProgress": or_null(default_tutorial_progress_path()),
    });
    writeln_doc(out, &doc);
    if config.is_none() { 2 } else { 0 }
}

fn run_project_info(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(err, "usage: torogao-cli project-info <project.inx>");
        return 3;
    }
    let path = &args[1];
    if !std::path::Path::new(path).exists() {
        let _ = writeln!(err, "project file not found: {path}");
        return 2;
    }
    match project_info_doc(path) {
        Ok(doc) => {
            writeln_doc(out, &doc);
            0
        }
        Err(message) => {
            let doc = serde_json::json!({
                "code": TG_CONTAINER_MALFORMED,
                "error": format!("INP payload unreadable: {message}"),
                "file": path,
                "result": "unloadable",
            });
            writeln_doc(out, &doc);
            2
        }
    }
}

/// Consumes the value after a flag, when one is present
/// (the reference's paired-argument scan).
fn take_value(args: &[String], i: &mut usize, slot: &mut String) {
    if *i + 1 < args.len() {
        *i += 1;
        *slot = args[*i].clone();
    }
}

/// The generic project tail: inspect, validate,
/// export-rig, physics-preview, apply-state, list-states,
/// rating-check, expression-check, tracking-check,
/// hitareas-list, events-list.
fn run_tail(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli <inspect|validate|export-rig|physics-preview|list-templates|instantiate-template|mcp|a2a|registry-dump> ..."
        );
        return 3;
    }
    let command = args[0].as_str();
    let project_path = args[1].clone();

    // Flag scan.
    let mut out_path = String::new();
    let mut track_path = String::new();
    let mut viseme_path = String::new();
    let mut controllers_path = String::new();
    let mut override_path = String::new();
    let mut contract_path = String::new();
    let mut state_name = String::new();
    let mut variant_id = String::new();
    let mut drivers_path = String::new();
    let mut pack_name = String::new();
    let mut set_id = String::new();
    let mut fps_override: i32 = 0;
    let mut force_out = false;
    let mut i = 2;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--out" => take_value(args, &mut i, &mut out_path),
            "--force" => force_out = true,
            "--track" => take_value(args, &mut i, &mut track_path),
            "--viseme" => take_value(args, &mut i, &mut viseme_path),
            "--contract" => take_value(args, &mut i, &mut contract_path),
            "--state" => take_value(args, &mut i, &mut state_name),
            "--variant" => take_value(args, &mut i, &mut variant_id),
            "--drivers" => take_value(args, &mut i, &mut drivers_path),
            "--controllers" => take_value(args, &mut i, &mut controllers_path),
            "--override" => take_value(args, &mut i, &mut override_path),
            "--pack" => take_value(args, &mut i, &mut pack_name),
            "--set" => take_value(args, &mut i, &mut set_id),
            "--fps" if i + 1 < args.len() => {
                i += 1;
                match args[i].parse::<i32>() {
                    Ok(value) => fps_override = value,
                    Err(_) => {
                        let _ = writeln!(err, "--fps needs a number");
                        return 3;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }

    const TAIL_COMMANDS: [&str; 11] = [
        "inspect",
        "validate",
        "export-rig",
        "physics-preview",
        "apply-state",
        "list-states",
        "rating-check",
        "expression-check",
        "tracking-check",
        "hitareas-list",
        "events-list",
    ];
    if !TAIL_COMMANDS.contains(&command) {
        let _ = writeln!(err, "unknown subcommand: {command}");
        return 3;
    }
    if !std::path::Path::new(&project_path).exists() {
        let _ = writeln!(err, "project file not found: {project_path}");
        return 2;
    }
    if command == "export-rig" && out_path.is_empty() {
        let _ = writeln!(err, "export-rig requires --out <dir>");
        return 3;
    }
    if command == "physics-preview" && track_path.is_empty() {
        let _ = writeln!(err, "physics-preview requires --track <track.json>");
        return 3;
    }
    if command == "apply-state" && track_path.is_empty() {
        let _ = writeln!(err, "apply-state requires --track <track.json>");
        return 3;
    }
    if command == "apply-state" && state_name.is_empty() && variant_id.is_empty() {
        let _ = writeln!(
            err,
            "apply-state requires --state <name> and/or --variant <id>"
        );
        return 3;
    }

    // The output guard covers every --out of this command
    // group (the export's file set is guarded again at its
    // own site with its full target list).
    if !out_path.is_empty() && command != "export-rig" {
        let mut guard_inputs: Vec<&str> = vec![&project_path];
        if !track_path.is_empty() {
            guard_inputs.push(&track_path);
        }
        if !viseme_path.is_empty() {
            guard_inputs.push(&viseme_path);
        }
        if let Err(refusal) = guards::guard_output(&out_path, &guard_inputs, force_out) {
            let doc = serde_json::json!({
                "code": refusal.code,
                "error": refusal.message,
                "file": project_path,
                "result": "refused",
            });
            writeln_doc(out, &doc);
            return 1;
        }
    }

    let mut loaded = match load_project(&project_path) {
        Ok(loaded) => loaded,
        Err(load_error) => {
            let doc = serde_json::json!({
                "code": TG_CONTAINER_MALFORMED,
                "error": load_error.message(),
                "file": project_path,
                "result": "unloadable",
            });
            writeln_doc(out, &doc);
            return 2;
        }
    };
    let walk = Walk::build(&mut loaded.puppet);
    let payload = payload_guarded(&project_path);
    let load = ToolLoad {
        path: project_path.clone(),
        loaded,
        walk,
        payload,
    };
    let ext_view = load.ext_view();

    match command {
        "inspect" => {
            let mut summary = ext_summary(&ext_view.doc);
            let glue = count_glue_keys(&load.payload)
                + if ext_view.present {
                    count_glue_keys(jget(&ext_view.doc, "foreign"))
                } else {
                    0
                };
            summary["glueRecords"] = Value::from(glue);
            let doc = serde_json::json!({
                "census": census(&load.loaded.puppet, &load.walk),
                "extension": summary,
                "file": project_path,
            });
            emit(out, &doc, &out_path);
            0
        }
        "validate" => {
            let findings = validate_findings(&load);
            let doc = serde_json::json!({
                "file": project_path,
                "findings": findings_json(&findings),
                "result": if findings.is_empty() { "clean" } else { "findings" },
            });
            let code = if findings.is_empty() { 0 } else { 1 };
            emit(out, &doc, &out_path);
            code
        }
        "export-rig" => cli_export_rig(&load, &ext_view, &out_path, force_out, out, err),
        "list-states" => {
            let doc = serde_json::json!({
                "extension": ext_summary(&ext_view.doc),
                "file": project_path,
            });
            emit(out, &doc, &out_path);
            0
        }
        "rating-check" => {
            let mut doc = serde_json::json!({
                "file": project_path,
                "stamp": rating_stamp(&ext_view.doc),
            });
            if ext_view.malformed {
                doc["result"] = Value::String("malformed".to_string());
                doc["code"] = Value::String(TG_EXT_MALFORMED.to_string());
                doc["error"] =
                    Value::String("the torogao EXT section is not parseable JSON".to_string());
                emit(out, &doc, &out_path);
                return 1;
            }
            let verdict = rating_verdict(&ext_view);
            doc["stamp"] = verdict.stamp.clone();
            if !verdict.complete {
                doc["result"] = Value::String("missing".to_string());
                doc["code"] = Value::String(verdict.code.to_string());
                doc["error"] = Value::String(verdict.reason.clone());
                emit(out, &doc, &out_path);
                return 1;
            }
            if verdict.blocked {
                doc["result"] = Value::String("mismatch".to_string());
                doc["code"] = Value::String(verdict.code.to_string());
                doc["error"] = Value::String(verdict.reason.clone());
                emit(out, &doc, &out_path);
                return 1;
            }
            doc["result"] = Value::String("ok".to_string());
            emit(out, &doc, &out_path);
            0
        }
        "expression-check" => {
            if pack_name.is_empty() {
                let _ = writeln!(err, "expression-check requires --pack <name>");
                return 3;
            }
            let pack = match load_pack(&pack_name) {
                Ok(pack) => pack,
                Err(message) => {
                    let _ = writeln!(err, "{message}");
                    return 3;
                }
            };
            let missing = expression_missing(&ext_view.doc, &pack);
            let mut doc = serde_json::json!({
                "file": project_path,
                "missing": missing,
                "pack": pack_name,
                "result": if missing.is_empty() { "covered" } else { "findings" },
            });
            if !missing.is_empty() {
                doc["code"] = Value::String("TG-EXPR-MISSING".to_string());
            }
            let code = if missing.is_empty() { 0 } else { 1 };
            emit(out, &doc, &out_path);
            code
        }
        "tracking-check" => {
            if pack_name.is_empty() || set_id.is_empty() {
                let _ = writeln!(err, "tracking-check requires --pack <name> --set <id>");
                return 3;
            }
            let pack = match load_pack(&pack_name) {
                Ok(pack) => pack,
                Err(message) => {
                    let _ = writeln!(err, "{message}");
                    return 3;
                }
            };
            let set = pack_tracking_set(&pack, &set_id);
            if !set.is_object() {
                let _ = writeln!(err, "pack '{pack_name}' has no tracking set '{set_id}'");
                return 3;
            }
            let (missing, range_bad) = tracking_coverage(&load.loaded.puppet, &set);
            let ok = missing.is_empty() && range_bad.is_empty();
            let mut doc = serde_json::json!({
                "completeness": pack_str(&set, "completeness"),
                "file": project_path,
                "missing": missing,
                "pack": pack_name,
                "rangeViolations": range_bad,
                "result": if ok { "covered" } else { "findings" },
                "set": set_id,
            });
            if !missing.is_empty() {
                doc["code"] = Value::String("TG-TRACK-MISSING".to_string());
            } else if !range_bad.is_empty() {
                doc["code"] = Value::String("TG-TRACK-RANGE".to_string());
            }
            emit(out, &doc, &out_path);
            if ok { 0 } else { 1 }
        }
        "hitareas-list" => {
            let doc = serde_json::json!({
                "file": project_path,
                "hitAreas": if ext_view.present {
                    jget(jget(&ext_view.doc, "consumer"), "hitAreas").clone()
                } else {
                    Value::Null
                },
            });
            emit(out, &doc, &out_path);
            0
        }
        "events-list" => {
            let doc = serde_json::json!({
                "file": project_path,
                "motionEvents": if ext_view.present {
                    jget(jget(&ext_view.doc, "consumer"), "motionEvents").clone()
                } else {
                    Value::Null
                },
            });
            emit(out, &doc, &out_path);
            0
        }
        "apply-state" | "physics-preview" => run_preview_command(
            load,
            &track_path,
            &viseme_path,
            &contract_path,
            &drivers_path,
            &controllers_path,
            &override_path,
            &state_name,
            &variant_id,
            fps_override,
            &out_path,
            out,
            err,
        ),
        _ => 3,
    }
}

fn cli_export_rig(
    load: &ToolLoad,
    ext_view: &ExtView,
    out_dir: &str,
    force: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let base = base_stem(&load.path);
    let names = [
        format!("{base}.model.json"),
        format!("{base}.export-report.json"),
        format!("{base}.stamp.json"),
    ];
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    if let Err(refusal) =
        guards::guard_output_files(out_dir, &name_refs, &[load.path.as_str()], force)
    {
        let doc = serde_json::json!({
            "code": refusal.code,
            "error": refusal.message,
            "file": load.path,
            "result": "refused",
        });
        writeln_doc(out, &doc);
        return 4;
    }
    if ext_view.present {
        if let Some((_, reason)) = rating_blocks_export(ext_view) {
            let doc = serde_json::json!({
                "code": "TG-RATING-LANE-MISMATCH",
                "error": reason,
                "file": load.path,
                "result": "refused",
            });
            writeln_doc(out, &doc);
            return 4;
        }
    }
    match crate::rigexport::export_rig_files(load, out_dir, &base, ext_view) {
        Ok(report) => {
            writeln_doc(out, &report);
            0
        }
        Err(crate::rigexport::ExportError::Build(message))
        | Err(crate::rigexport::ExportError::Io(message)) => {
            let _ = writeln!(err, "export failed: {message}");
            4
        }
    }
}

/// The shared apply-state / physics-preview block: layer
/// files load (failures are usage errors on stderr), the
/// preview runs, and the curves print or write.
#[allow(clippy::too_many_arguments)]
fn run_preview_command(
    mut load: ToolLoad,
    track_path: &str,
    viseme_path: &str,
    contract_path: &str,
    drivers_path: &str,
    controllers_path: &str,
    override_path: &str,
    state_name: &str,
    variant_id: &str,
    fps_override: i32,
    out_path: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let track: Value = match std::fs::read_to_string(track_path)
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
    {
        Ok(track) => track,
        Err(message) => {
            let _ = writeln!(err, "input track unreadable: {message}");
            return 3;
        }
    };
    let mut options = crate::preview::PreviewOptions {
        ext: load.ext_view().doc,
        state_name: state_name.to_string(),
        variant_id: variant_id.to_string(),
        ..Default::default()
    };
    if !viseme_path.is_empty() {
        match crate::families::bake_viseme_layer(viseme_path, contract_path) {
            Ok(curves) => {
                options.has_viseme = true;
                options.viseme_curves = curves;
            }
            Err(crate::families::LayerError::Usage(message)) => {
                let _ = writeln!(err, "{message}");
                return 3;
            }
            Err(crate::families::LayerError::Bake(message)) => {
                let doc = serde_json::json!({
                    "error": message,
                    "file": load.path,
                    "result": "error",
                });
                writeln_doc(out, &doc);
                return 3;
            }
        }
    }
    if !drivers_path.is_empty() {
        let driver_doc: Value = match std::fs::read_to_string(drivers_path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
        {
            Ok(doc) => doc,
            Err(message) => {
                let _ = writeln!(err, "driver set unreadable: {message}");
                return 3;
            }
        };
        match crate::families::parse_driver_config(&driver_doc) {
            Ok(config) => {
                options.has_drivers = true;
                options.drivers = config;
            }
            Err(message) => {
                let _ = writeln!(err, "{message}");
                return 3;
            }
        }
    }
    if !controllers_path.is_empty() {
        let values_doc: Value = match std::fs::read_to_string(controllers_path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
        {
            Ok(doc) => doc,
            Err(message) => {
                let _ = writeln!(err, "controller values unreadable: {message}");
                return 3;
            }
        };
        let (resolved, problems) =
            crate::controllers::resolve_controllers(&options.ext, &values_doc);
        if let Some(first) = problems.first() {
            let _ = writeln!(err, "{first}");
            return 3;
        }
        options.has_controllers = true;
        options.controller_values = resolved;
    }
    if !override_path.is_empty() {
        match std::fs::read_to_string(override_path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
        {
            Ok(doc) => {
                options.has_override = true;
                options.override_curves = doc;
            }
            Err(message) => {
                let _ = writeln!(err, "override take unreadable: {message}");
                return 3;
            }
        }
    }
    let project_label = load.path.clone();
    match crate::preview::run_physics_preview(
        &mut load.loaded.puppet,
        &track,
        fps_override,
        &project_label,
        &options,
    ) {
        Ok(curves) => {
            let text = curves.to_json();
            if !out_path.is_empty() {
                let bytes = format!("{text}\n");
                let _ = std::fs::write(out_path, &bytes);
                let digest = sha2::Sha256::digest(bytes.as_bytes());
                let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
                let doc = serde_json::json!({
                    "curves": out_path,
                    "file": load.path,
                    "series": curves.series.len(),
                    "sha256": hex,
                    "steps": curves.steps(),
                });
                writeln_doc(out, &doc);
            } else {
                let _ = writeln!(out, "{text}");
            }
            0
        }
        Err(message) => {
            let doc = serde_json::json!({
                "error": message,
                "file": load.path,
                "result": "error",
            });
            writeln_doc(out, &doc);
            3
        }
    }
}

/// The paths the export writes (used by tests).
pub fn export_paths(out_dir: &str, base: &str) -> [String; 3] {
    [
        join_path(out_dir, &format!("{base}.model.json")),
        join_path(out_dir, &format!("{base}.export-report.json")),
        join_path(out_dir, &format!("{base}.stamp.json")),
    ]
}
