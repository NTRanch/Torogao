//! The special subcommands (everything outside the
//! generic project tail): each family lands here as its
//! port completes. Returns None when the invocation is
//! not a special command. `args[0]` is the subcommand
//! name (the reference's args[1]).

use crate::creator::create_character;
use crate::extdoc::{
    validate_hit_areas_only, write_project_with_ext, ExtView, EXT_FORMAT,
};
use crate::guards::{guard_output, load_project, TG_CONTAINER_MALFORMED};
use crate::packs::{load_pack, pack_names};
use crate::templates::{instantiate_template, list_templates};
use crate::util::jget;
use crate::walk::Walk;
use crate::writer::{
    build_scaffold, load_json_file, load_sfx_index, validate_voice, ScaffoldOutcome,
};
use serde_json::{json, Value};
use std::io::Write;
use torogao_format::jsonfmt::to_d_json;

/// Prints a document as one JSON line.
fn print_doc(out: &mut dyn Write, doc: &Value) {
    let _ = writeln!(out, "{}", to_d_json(doc));
}

/// The refusal document shape the authoring specials
/// share.
fn refusal_doc(extra: &[(&str, Value)], code: &str, message: &str) -> Value {
    let mut doc = json!({
        "code": code,
        "error": message,
        "result": "refused",
    });
    for (key, value) in extra {
        doc[key] = value.clone();
    }
    doc
}

/// Runs a special subcommand, if `args` names one.
pub fn run_special(
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Option<i32> {
    match args.first().map(String::as_str) {
        Some("audio-bake") => Some(cmd_audio_bake(args, out, err)),
        Some("audio-build-session") => Some(cmd_audio_build_session(args, out, err)),
        Some("audio-render") => Some(cmd_audio_render(args, out, err)),
        Some("audio-validate") => Some(cmd_audio_validate(args, out, err)),
        Some("audio-validate-session") => Some(cmd_audio_validate_session(args, out, err)),
        Some("events-author") => Some(cmd_events_author(args, out, err)),
        Some("lipsync-analyze") => Some(cmd_lipsync_analyze(args, out, err)),
        Some("pack-info") => Some(cmd_pack_info(args, out, err)),
        Some("sfx-list") => Some(cmd_sfx_list(out, err)),
        Some("tutorial-check") => Some(cmd_tutorial_check(args, out, err)),
        Some("tutorial-list") => Some(cmd_tutorial_list(args, out, err)),
        Some("tutorial-status") => Some(cmd_tutorial_status(args, out, err)),
        Some("viseme-author") => Some(cmd_viseme_author(args, out, err)),
        Some("viseme-bake") => Some(cmd_viseme_bake(args, out, err)),
        Some("viseme-check") => Some(cmd_viseme_check(args, out, err)),
        Some("viseme-validate") => Some(cmd_viseme_validate(args, out, err)),
        Some("voice-check") => Some(cmd_voice_check(args, out, err)),
        Some("create-character") => Some(cmd_create_character(args, out, err)),
        Some("writer-draft") => Some(cmd_writer_draft(args, out, err)),
        Some("writer-scaffold") => Some(cmd_writer_scaffold(args, out, err)),
        Some("hitareas-author") => Some(cmd_hitareas_author(args, out, err)),
        Some("list-templates") => Some(cmd_list_templates(out)),
        Some("instantiate-template") => Some(cmd_instantiate_template(args, out, err)),
        _ => None,
    }
}

/// The P5 subcommand family's interception (runs before
/// the other specials, as in the reference's main).
/// Returns None when the invocation is not a P5 command.
pub fn run_p5_cli(
    _args: &[String],
    _out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Option<i32> {
    None
}

fn cmd_pack_info(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(err, "usage: torogao-cli pack-info <pack>");
        return 3;
    }
    match load_pack(&args[1]) {
        Ok(pack) => {
            print_doc(
                out,
                &json!({
                    "pack": pack,
                    "packs": pack_names(),
                }),
            );
            0
        }
        Err(message) => {
            let _ = writeln!(err, "{message}");
            3
        }
    }
}

fn cmd_sfx_list(out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match load_sfx_index() {
        Ok(index) => {
            print_doc(out, &index);
            0
        }
        Err(message) => {
            let _ = writeln!(err, "{message}");
            3
        }
    }
}

fn cmd_voice_check(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(err, "usage: torogao-cli voice-check <record.json>");
        return 3;
    }
    let record = match load_json_file(&args[1]) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let findings = validate_voice(&record);
    print_doc(
        out,
        &json!({
            "file": args[1],
            "findings": findings.iter().map(|f| f.to_json()).collect::<Vec<_>>(),
            "result": if findings.is_empty() { "clean" } else { "findings" },
        }),
    );
    if findings.is_empty() {
        0
    } else {
        1
    }
}

fn cmd_create_character(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut identity_path = String::new();
    let mut pack_name = String::new();
    let mut out_path = String::new();
    let mut presets: Vec<String> = Vec::new();
    let mut force = false;
    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        let value = args.get(i + 1);
        match (arg, value) {
            ("--identity", Some(v)) => {
                identity_path = v.clone();
                i += 1;
            }
            ("--pack", Some(v)) => {
                pack_name = v.clone();
                i += 1;
            }
            ("--preset", Some(v)) => {
                presets.push(v.clone());
                i += 1;
            }
            ("--out", Some(v)) => {
                out_path = v.clone();
                i += 1;
            }
            ("--force", _) => force = true,
            _ => {
                let _ = writeln!(err, "create-character: unknown argument: {arg}");
                return 3;
            }
        }
        i += 1;
    }
    if identity_path.is_empty() || pack_name.is_empty() || out_path.is_empty() {
        let _ = writeln!(
            err,
            "usage: torogao-cli create-character --identity <record.json> --pack <name> [--preset <p>]... --out <file.inx>"
        );
        return 3;
    }
    let pack = match load_pack(&pack_name) {
        Ok(pack) => pack,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let identity = match load_json_file(&identity_path) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    if let Err(refusal) = guard_output(&out_path, &[identity_path.as_str()], force) {
        print_doc(out, &refusal_doc(&[], refusal.code, &refusal.message));
        return 1;
    }
    match create_character(&identity, &pack, &presets, &out_path) {
        Ok(summary) => {
            print_doc(out, &summary);
            0
        }
        Err((problems, message)) => {
            if !problems.is_empty() {
                print_doc(
                    out,
                    &json!({
                        "problems": problems.iter().map(|p| p.to_json()).collect::<Vec<_>>(),
                        "result": "refused",
                    }),
                );
                1
            } else {
                print_doc(
                    out,
                    &json!({
                        "error": message,
                        "result": "error",
                    }),
                );
                3
            }
        }
    }
}

fn cmd_writer_scaffold(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut char_path = String::new();
    let mut pack_name = String::new();
    let mut kink = String::new();
    let mut out_path = String::new();
    let mut sfx_ids: Vec<String> = Vec::new();
    let mut stage: i64 = -1;
    let mut force = false;
    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        let value = args.get(i + 1);
        match (arg, value) {
            ("--character", Some(v)) => {
                char_path = v.clone();
                i += 1;
            }
            ("--pack", Some(v)) => {
                pack_name = v.clone();
                i += 1;
            }
            ("--kink", Some(v)) => {
                kink = v.clone();
                i += 1;
            }
            ("--sfx", Some(v)) => {
                sfx_ids.push(v.clone());
                i += 1;
            }
            ("--out", Some(v)) => {
                out_path = v.clone();
                i += 1;
            }
            ("--force", _) => force = true,
            ("--stage", Some(v)) => {
                match v.parse::<i64>() {
                    Ok(parsed) => stage = parsed,
                    Err(_) => {
                        let _ = writeln!(err, "--stage needs a number");
                        return 3;
                    }
                }
                i += 1;
            }
            _ => {
                let _ = writeln!(err, "writer-scaffold: unknown argument: {arg}");
                return 3;
            }
        }
        i += 1;
    }
    if char_path.is_empty() || pack_name.is_empty() || kink.is_empty() || stage < 0 {
        let _ = writeln!(
            err,
            "usage: torogao-cli writer-scaffold --character <record.json> --pack <name> --kink <k> --stage <n> [--sfx <id>]... [--out <file.json>]"
        );
        return 3;
    }
    let pack = match load_pack(&pack_name) {
        Ok(pack) => pack,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let character = match load_json_file(&char_path) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let (doc, is_gap) = match build_scaffold(&character, &pack, &kink, stage, &sfx_ids) {
        ScaffoldOutcome::Scaffold(doc) => (doc, false),
        ScaffoldOutcome::Gap(doc) => (doc, true),
    };
    if !out_path.is_empty() {
        if let Err(refusal) = guard_output(&out_path, &[char_path.as_str()], force) {
            print_doc(out, &refusal_doc(&[], refusal.code, &refusal.message));
            return 1;
        }
        let _ = std::fs::write(&out_path, format!("{}\n", to_d_json(&doc)));
    }
    print_doc(out, &doc);
    if is_gap {
        1
    } else {
        0
    }
}

fn cmd_hitareas_author(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli hitareas-author <project.inx> --spec <areas.json> --out <new.inx>"
        );
        return 3;
    }
    let project_path = args[1].clone();
    let mut spec_path = String::new();
    let mut out_path = String::new();
    let mut force = false;
    let mut i = 2;
    while i < args.len() {
        let arg = args[i].as_str();
        let value = args.get(i + 1);
        match (arg, value) {
            ("--spec", Some(v)) => {
                spec_path = v.clone();
                i += 1;
            }
            ("--out", Some(v)) => {
                out_path = v.clone();
                i += 1;
            }
            ("--force", _) => force = true,
            _ => {
                let _ = writeln!(err, "hitareas-author: unknown argument: {arg}");
                return 3;
            }
        }
        i += 1;
    }
    if spec_path.is_empty() || out_path.is_empty() {
        let _ = writeln!(err, "hitareas-author requires --spec and --out");
        return 3;
    }
    if !std::path::Path::new(&project_path).exists() {
        let _ = writeln!(err, "project file not found: {project_path}");
        return 2;
    }
    let spec = match load_json_file(&spec_path) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    if let Err(refusal) = guard_output(
        &out_path,
        &[project_path.as_str(), spec_path.as_str()],
        force,
    ) {
        print_doc(out, &refusal_doc(&[], refusal.code, &refusal.message));
        return 1;
    }
    let mut loaded = match load_project(&project_path) {
        Ok(loaded) => loaded,
        Err(e) => {
            print_doc(
                out,
                &json!({
                    "code": TG_CONTAINER_MALFORMED,
                    "error": e.message(),
                    "file": project_path,
                    "result": "unloadable",
                }),
            );
            return 2;
        }
    };
    let walk = Walk::build(&mut loaded.puppet);
    let view = ExtView::of_project(&loaded);
    let mut ext = if view.present { view.doc.clone() } else { Value::Null };
    if !ext.is_object() {
        ext = json!({ "format": EXT_FORMAT });
    }
    let mut consumer = jget(&ext, "consumer").clone();
    if !consumer.is_object() {
        consumer = json!({});
    }
    let mut areas = jget(&spec, "hitAreas").clone();
    if !areas.is_array() {
        areas = spec.clone();
    }
    if !areas.is_array() {
        let _ = writeln!(
            err,
            "hit-area spec must be an array or an object with a hitAreas array"
        );
        return 3;
    }
    consumer["hitAreas"] = areas.clone();
    ext["consumer"] = consumer;
    let problems = validate_hit_areas_only(&ext, &loaded.puppet, &walk);
    if !problems.is_empty() {
        print_doc(
            out,
            &json!({
                "findings": problems.iter().map(|f| f.to_json()).collect::<Vec<_>>(),
                "result": "refused",
            }),
        );
        return 1;
    }
    if let Err(message) = write_project_with_ext(&project_path, &out_path, &ext) {
        let _ = writeln!(err, "{message}");
        return 3;
    }
    print_doc(
        out,
        &json!({
            "hitAreas": areas.as_array().map_or(0, Vec::len),
            "out": out_path,
            "result": "authored",
        }),
    );
    0
}

fn cmd_list_templates(out: &mut dyn Write) -> i32 {
    let list: Vec<Value> = list_templates()
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
    print_doc(out, &json!({ "templates": list }));
    0
}

fn cmd_instantiate_template(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli instantiate-template <name> [--preset <p>]... --out <file.inx>"
        );
        return 3;
    }
    let name = args[1].clone();
    let mut presets: Vec<String> = Vec::new();
    let mut out_path = String::new();
    let mut force = false;
    let mut i = 2;
    while i < args.len() {
        let arg = args[i].as_str();
        let value = args.get(i + 1);
        match (arg, value) {
            ("--preset", Some(v)) => {
                presets.push(v.clone());
                i += 1;
            }
            ("--out", Some(v)) => {
                out_path = v.clone();
                i += 1;
            }
            ("--force", _) => force = true,
            _ => {
                let _ = writeln!(err, "instantiate-template: unknown argument: {arg}");
                return 3;
            }
        }
        i += 1;
    }
    if out_path.is_empty() {
        let _ = writeln!(err, "instantiate-template requires --out <file>");
        return 3;
    }
    if let Err(refusal) = guard_output(&out_path, &[], force) {
        print_doc(
            out,
            &refusal_doc(
                &[("template", json!(name))],
                refusal.code,
                &refusal.message,
            ),
        );
        return 1;
    }
    match instantiate_template(&name, &presets, &out_path) {
        Ok(summary) => {
            print_doc(out, &summary);
            0
        }
        Err(message) => {
            print_doc(
                out,
                &json!({
                    "error": message,
                    "result": "error",
                    "template": name,
                }),
            );
            3
        }
    }
}

// ------------------------------------------------------------
// P4 specials (app.d)

fn read_json_file(path: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn cmd_audio_validate(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(err, "usage: torogao-cli audio-validate <recipe.json>");
        return 3;
    }
    let recipe = match read_json_file(&args[1]) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "recipe unreadable: {message}");
            return 3;
        }
    };
    let findings = crate::audioproc::validate_recipe(&recipe);
    let has_error = findings.iter().any(|f| f.severity == "error");
    print_doc(
        out,
        &json!({
            "file": args[1],
            "findings": crate::criteria::findings_json(&findings),
            "result": if has_error { "findings" } else { "clean" },
        }),
    );
    if has_error {
        1
    } else {
        0
    }
}

fn cmd_audio_validate_session(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(err, "usage: torogao-cli audio-validate-session <session.json>");
        return 3;
    }
    let session = match read_json_file(&args[1]) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "session unreadable: {message}");
            return 3;
        }
    };
    let findings = crate::sessionbuild::validate_session(&session);
    let has_error = findings.iter().any(|f| f.severity == "error");
    print_doc(
        out,
        &json!({
            "file": args[1],
            "findings": crate::criteria::findings_json(&findings),
            "result": if findings.is_empty() { "clean" } else { "findings" },
        }),
    );
    if has_error {
        1
    } else {
        0
    }
}

fn cmd_audio_build_session(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli audio-build-session <session.json> --out <dir>"
        );
        return 3;
    }
    let mut out_dir = String::new();
    let mut force = false;
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--out" && i + 1 < args.len() {
            out_dir = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--force" {
            force = true;
        }
        i += 1;
    }
    if out_dir.is_empty() {
        let _ = writeln!(err, "audio-build-session requires --out <dir>");
        return 3;
    }
    let session = match read_json_file(&args[1]) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "session unreadable: {message}");
            return 3;
        }
    };
    for f in crate::sessionbuild::validate_session(&session) {
        if f.severity == "error" {
            print_doc(
                out,
                &json!({
                    "code": f.code,
                    "error": f.message,
                    "file": args[1],
                    "result": "refused",
                }),
            );
            return 1;
        }
    }
    let recipe = match crate::sessionbuild::build_session_recipe(&session) {
        Ok(r) => r,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let name = crate::packs::pack_str(&session, "name");
    let files = [
        format!("{name}.recipe.json"),
        format!("{name}.wav"),
        format!("{name}.manifest.json"),
    ];
    if let Err(refusal) = crate::guards::guard_output_files(
        &out_dir,
        &[files[0].as_str(), files[1].as_str(), files[2].as_str()],
        &[args[1].as_str()],
        force,
    ) {
        print_doc(
            out,
            &json!({
                "code": refusal.code,
                "error": refusal.message,
                "file": args[1],
                "result": "refused",
            }),
        );
        return 1;
    }
    let _ = std::fs::create_dir_all(&out_dir);
    let recipe_text = to_d_json(&recipe);
    let recipe_path = format!("{out_dir}/{name}.recipe.json");
    let _ = std::fs::write(&recipe_path, format!("{recipe_text}\n"));
    let result = crate::audioproc::render_recipe(&recipe, &recipe_text, &out_dir);
    if !result.error.is_empty() {
        print_doc(
            out,
            &json!({
                "error": result.error,
                "file": args[1],
                "result": "error",
            }),
        );
        return 3;
    }
    print_doc(
        out,
        &json!({
            "manifest": result.manifest,
            "recipe": recipe_path,
            "session": args[1],
        }),
    );
    0
}

fn cmd_audio_render(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli audio-render <recipe.json> --out <dir>"
        );
        return 3;
    }
    let mut out_dir = String::new();
    let mut force = false;
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--out" && i + 1 < args.len() {
            out_dir = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--force" {
            force = true;
        }
        i += 1;
    }
    if out_dir.is_empty() {
        let _ = writeln!(err, "audio-render requires --out <dir>");
        return 3;
    }
    let recipe_text = match std::fs::read_to_string(&args[1]) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(err, "recipe unreadable: {e}");
            return 3;
        }
    };
    let recipe: Value = match serde_json::from_str(&recipe_text) {
        Ok(doc) => doc,
        Err(e) => {
            let _ = writeln!(err, "recipe unreadable: {e}");
            return 3;
        }
    };
    let mut rname = crate::packs::pack_str(&recipe, "name");
    if rname.is_empty() {
        rname = "untitled".to_string();
    }
    let files = [format!("{rname}.wav"), format!("{rname}.manifest.json")];
    if let Err(refusal) = crate::guards::guard_output_files(
        &out_dir,
        &[files[0].as_str(), files[1].as_str()],
        &[args[1].as_str()],
        force,
    ) {
        print_doc(
            out,
            &json!({
                "code": refusal.code,
                "error": refusal.message,
                "file": args[1],
                "result": "refused",
            }),
        );
        return 1;
    }
    let _ = std::fs::create_dir_all(&out_dir);
    let result = crate::audioproc::render_recipe(&recipe, &recipe_text, &out_dir);
    if !result.error.is_empty() {
        print_doc(
            out,
            &json!({
                "error": result.error,
                "file": args[1],
                "result": "error",
            }),
        );
        return 3;
    }
    print_doc(out, &result.manifest);
    0
}

fn cmd_audio_bake(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli audio-bake <file.wav> --recipe <bake.json> --out <curves.json>"
        );
        return 3;
    }
    let mut recipe_path = String::new();
    let mut bake_out = String::new();
    let mut force = false;
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--recipe" && i + 1 < args.len() {
            recipe_path = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--out" && i + 1 < args.len() {
            bake_out = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--force" {
            force = true;
        }
        i += 1;
    }
    if recipe_path.is_empty() || bake_out.is_empty() {
        let _ = writeln!(err, "audio-bake requires --recipe and --out");
        return 3;
    }
    let recipe = match read_json_file(&recipe_path) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "bake recipe unreadable: {message}");
            return 3;
        }
    };
    let wav = match crate::wav::read_wav_file(&args[1]) {
        Ok(w) => w,
        Err(message) => {
            let _ = writeln!(err, "WAV unreadable: {message}");
            return 3;
        }
    };
    if let Err(refusal) = guard_output(
        &bake_out,
        &[args[1].as_str(), recipe_path.as_str()],
        force,
    ) {
        print_doc(out, &refusal_doc(&[("file", json!(args[1]))], refusal.code, &refusal.message));
        return 1;
    }
    let curves = match crate::audiobake::bake_envelope(&wav, &recipe, &args[1]) {
        Ok(c) => c,
        Err(message) => {
            print_doc(
                out,
                &json!({
                    "error": message,
                    "file": args[1],
                    "result": "error",
                }),
            );
            return 3;
        }
    };
    let text = curves.to_json();
    let _ = std::fs::write(&bake_out, format!("{text}\n"));
    print_doc(
        out,
        &json!({
            "curves": bake_out,
            "file": args[1],
            "series": curves.series.len(),
            "steps": curves.steps(),
        }),
    );
    0
}

fn cmd_viseme_check(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli viseme-check <project.inx> [--contract <file>]"
        );
        return 3;
    }
    let mut contract_path = String::new();
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--contract" && i + 1 < args.len() {
            contract_path = args[i + 1].clone();
            i += 1;
        }
        i += 1;
    }
    let contract = match crate::viseme::load_viseme_contract(&contract_path) {
        Ok(c) => c,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    if !std::path::Path::new(&args[1]).exists() {
        let _ = writeln!(err, "project file not found: {}", args[1]);
        return 2;
    }
    let puppet = match crate::guards::load_puppet_guarded(&args[1]) {
        Ok(p) => p,
        Err(e) => {
            print_doc(
                out,
                &json!({
                    "code": TG_CONTAINER_MALFORMED,
                    "error": e.message(),
                    "file": args[1],
                    "result": "unloadable",
                }),
            );
            return 2;
        }
    };
    let findings = crate::viseme::check_visemes(&puppet, &contract);
    print_doc(
        out,
        &json!({
            "contract": contract.set_name,
            "covered": crate::viseme::covered_parameters(&puppet, &contract),
            "file": args[1],
            "findings": crate::criteria::findings_json(&findings),
            "result": if findings.is_empty() { "clean" } else { "findings" },
        }),
    );
    if findings.is_empty() {
        0
    } else {
        1
    }
}

fn cmd_lipsync_analyze(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli lipsync-analyze <wav> --out <proposal.json> [--contract <file>]"
        );
        return 3;
    }
    let mut contract_path = String::new();
    let mut proposal_out = String::new();
    let mut force = false;
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--contract" && i + 1 < args.len() {
            contract_path = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--out" && i + 1 < args.len() {
            proposal_out = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--force" {
            force = true;
        }
        i += 1;
    }
    if proposal_out.is_empty() {
        let _ = writeln!(err, "lipsync-analyze requires --out <proposal.json>");
        return 3;
    }
    let contract = match crate::viseme::load_viseme_contract(&contract_path) {
        Ok(c) => c,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    if let Err(refusal) = guard_output(&proposal_out, &[args[1].as_str()], force) {
        print_doc(out, &refusal_doc(&[("file", json!(args[1]))], refusal.code, &refusal.message));
        return 1;
    }
    let proposal = match crate::lipsync::analyze_lipsync(&args[1], &contract) {
        Ok(p) => p,
        Err(message) => {
            print_doc(
                out,
                &json!({
                    "error": message,
                    "file": args[1],
                    "result": "error",
                }),
            );
            return 1;
        }
    };
    let text = to_d_json(&proposal);
    let _ = std::fs::write(&proposal_out, format!("{text}\n"));
    print_doc(
        out,
        &json!({
            "durationSeconds": jget(&proposal, "durationSeconds"),
            "events": jget(&proposal, "features").as_array().map_or(0, Vec::len),
            "proposal": proposal_out,
            "wav": args[1],
        }),
    );
    0
}

fn cmd_viseme_author(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli viseme-author <spec.json> [--contract <file>] --out <track.json>"
        );
        return 3;
    }
    let mut contract_path = String::new();
    let mut author_out = String::new();
    let mut force = false;
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--contract" && i + 1 < args.len() {
            contract_path = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--out" && i + 1 < args.len() {
            author_out = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--force" {
            force = true;
        }
        i += 1;
    }
    if author_out.is_empty() {
        let _ = writeln!(err, "viseme-author requires --out <track.json>");
        return 3;
    }
    let contract = match crate::viseme::load_viseme_contract(&contract_path) {
        Ok(c) => c,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let spec = match read_json_file(&args[1]) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "spec unreadable: {message}");
            return 3;
        }
    };
    if let Err(refusal) = guard_output(&author_out, &[args[1].as_str()], force) {
        print_doc(out, &refusal_doc(&[("file", json!(args[1]))], refusal.code, &refusal.message));
        return 1;
    }
    let track = match crate::viseme::author_viseme_track(&spec, &contract) {
        Ok(t) => t,
        Err(message) => {
            print_doc(
                out,
                &json!({
                    "code": "TG-VISEME-AUTHOR",
                    "error": message,
                    "file": args[1],
                    "result": "error",
                }),
            );
            return 1;
        }
    };
    let text = to_d_json(&track);
    let _ = std::fs::write(&author_out, format!("{text}\n"));
    print_doc(
        out,
        &json!({
            "durationSeconds": jget(&track, "durationSeconds"),
            "events": jget(&track, "events").as_array().map_or(0, Vec::len),
            "spec": args[1],
            "track": author_out,
        }),
    );
    0
}

fn cmd_viseme_validate(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 3 {
        let _ = writeln!(
            err,
            "usage: torogao-cli viseme-validate <track.json> <project.inx> [--contract <file>]"
        );
        return 3;
    }
    let mut contract_path = String::new();
    let mut i = 3;
    while i < args.len() {
        if args[i] == "--contract" && i + 1 < args.len() {
            contract_path = args[i + 1].clone();
            i += 1;
        }
        i += 1;
    }
    let contract = match crate::viseme::load_viseme_contract(&contract_path) {
        Ok(c) => c,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let track = match read_json_file(&args[1]) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "track unreadable: {message}");
            return 3;
        }
    };
    if !std::path::Path::new(&args[2]).exists() {
        let _ = writeln!(err, "project file not found: {}", args[2]);
        return 2;
    }
    let puppet = match crate::guards::load_puppet_guarded(&args[2]) {
        Ok(p) => p,
        Err(e) => {
            print_doc(
                out,
                &json!({
                    "code": TG_CONTAINER_MALFORMED,
                    "error": e.message(),
                    "file": args[2],
                    "result": "unloadable",
                }),
            );
            return 2;
        }
    };
    let mut findings = crate::viseme::validate_viseme_track(&track, &contract);
    findings.extend(crate::viseme::check_visemes(&puppet, &contract));
    let has_error = findings.iter().any(|f| f.severity == "error");
    print_doc(
        out,
        &json!({
            "findings": crate::criteria::findings_json(&findings),
            "project": args[2],
            "result": if findings.is_empty() { "clean" } else { "findings" },
            "track": args[1],
        }),
    );
    if has_error {
        1
    } else {
        0
    }
}

fn cmd_viseme_bake(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli viseme-bake <track.json> [--contract <file>] --out <curves.json>"
        );
        return 3;
    }
    let mut contract_path = String::new();
    let mut bake_out = String::new();
    let mut force = false;
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--contract" && i + 1 < args.len() {
            contract_path = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--out" && i + 1 < args.len() {
            bake_out = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--force" {
            force = true;
        }
        i += 1;
    }
    if bake_out.is_empty() {
        let _ = writeln!(err, "viseme-bake requires --out <curves.json>");
        return 3;
    }
    let contract = match crate::viseme::load_viseme_contract(&contract_path) {
        Ok(c) => c,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let track = match read_json_file(&args[1]) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "track unreadable: {message}");
            return 3;
        }
    };
    if let Err(refusal) = guard_output(&bake_out, &[args[1].as_str()], force) {
        print_doc(out, &refusal_doc(&[("file", json!(args[1]))], refusal.code, &refusal.message));
        return 1;
    }
    let curves = match crate::viseme::bake_visemes(&track, &contract) {
        Ok(c) => c,
        Err(message) => {
            print_doc(
                out,
                &json!({
                    "error": message,
                    "file": args[1],
                    "result": "error",
                }),
            );
            return 3;
        }
    };
    let text = curves.to_json();
    let _ = std::fs::write(&bake_out, format!("{text}\n"));
    print_doc(
        out,
        &json!({
            "curves": bake_out,
            "series": curves.series.len(),
            "steps": curves.steps(),
            "track": args[1],
        }),
    );
    0
}

fn cmd_events_author(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli events-author <project.inx> --spec <events.json> --out <new.inx>"
        );
        return 3;
    }
    let mut spec_path = String::new();
    let mut ev_out = String::new();
    let mut force = false;
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--spec" && i + 1 < args.len() {
            spec_path = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--out" && i + 1 < args.len() {
            ev_out = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--force" {
            force = true;
        } else {
            let _ = writeln!(err, "events-author: unknown argument: {}", args[i]);
            return 3;
        }
        i += 1;
    }
    if spec_path.is_empty() || ev_out.is_empty() {
        let _ = writeln!(err, "events-author requires --spec and --out");
        return 3;
    }
    if !std::path::Path::new(&args[1]).exists() {
        let _ = writeln!(err, "project file not found: {}", args[1]);
        return 2;
    }
    let spec = match load_json_file(&spec_path) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    if let Err(refusal) = guard_output(
        &ev_out,
        &[args[1].as_str(), spec_path.as_str()],
        force,
    ) {
        print_doc(out, &refusal_doc(&[], refusal.code, &refusal.message));
        return 1;
    }
    let loaded = match load_project(&args[1]) {
        Ok(l) => l,
        Err(e) => {
            print_doc(
                out,
                &json!({
                    "code": TG_CONTAINER_MALFORMED,
                    "error": e.message(),
                    "file": args[1],
                    "result": "unloadable",
                }),
            );
            return 2;
        }
    };
    let view = ExtView::of_project(&loaded);
    let mut ext = if view.present { view.doc.clone() } else { Value::Null };
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
        let _ = writeln!(
            err,
            "event spec must be an array or an object with a motionEvents array"
        );
        return 3;
    }
    consumer["motionEvents"] = events.clone();
    ext["consumer"] = consumer;
    let probe = crate::extdoc::validate_motion_events_only(&ext, &loaded.puppet);
    if !probe.is_empty() {
        print_doc(
            out,
            &json!({
                "findings": crate::criteria::findings_json(&probe),
                "result": "refused",
            }),
        );
        return 1;
    }
    if let Err(message) = write_project_with_ext(&args[1], &ev_out, &ext) {
        let _ = writeln!(err, "{message}");
        return 3;
    }
    print_doc(
        out,
        &json!({
            "motionEvents": events.as_array().map_or(0, Vec::len),
            "out": ev_out,
            "result": "authored",
        }),
    );
    0
}

fn cmd_writer_draft(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut char_path = String::new();
    let mut w_pack = String::new();
    let mut w_kink = String::new();
    let mut w_out = String::new();
    let mut lane_path = String::new();
    let mut w_stage: i64 = -1;
    let mut w_sfx: Vec<String> = Vec::new();
    let mut force = false;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--character" && i + 1 < args.len() {
            char_path = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--pack" && i + 1 < args.len() {
            w_pack = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--kink" && i + 1 < args.len() {
            w_kink = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--lane" && i + 1 < args.len() {
            lane_path = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--sfx" && i + 1 < args.len() {
            w_sfx.push(args[i + 1].clone());
            i += 1;
        } else if args[i] == "--out" && i + 1 < args.len() {
            w_out = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--force" {
            force = true;
        } else if args[i] == "--stage" && i + 1 < args.len() {
            i += 1;
            match args[i].parse::<i64>() {
                Ok(v) => w_stage = v,
                Err(_) => {
                    let _ = writeln!(err, "--stage needs a number");
                    return 3;
                }
            }
        } else {
            let _ = writeln!(err, "writer-draft: unknown argument: {}", args[i]);
            return 3;
        }
        i += 1;
    }
    if char_path.is_empty()
        || w_pack.is_empty()
        || w_kink.is_empty()
        || w_stage < 0
        || lane_path.is_empty()
    {
        let _ = writeln!(
            err,
            "usage: torogao-cli writer-draft --character <record.json> --pack <name> --kink <k> --stage <n> --lane <lane.json> [--sfx <id>]... [--out <file.json>]"
        );
        return 3;
    }
    let lane = match crate::writerlane::load_lane_config(&lane_path) {
        Ok(l) => l,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let pack = match load_pack(&w_pack) {
        Ok(p) => p,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let character = match load_json_file(&char_path) {
        Ok(doc) => doc,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let (doc, is_draft) = match crate::writerlane::run_writer_lane(
        &character, &pack, &w_kink, w_stage, &w_sfx, &lane,
    ) {
        crate::writerlane::LaneOutcome::Draft(d) => (d, true),
        crate::writerlane::LaneOutcome::Gap(d) => (d, false),
        crate::writerlane::LaneOutcome::Refusal(d) => (d, false),
        crate::writerlane::LaneOutcome::Error(message) => {
            print_doc(
                out,
                &json!({
                    "code": crate::writerlane::TG_WRITER_LANE,
                    "error": message,
                    "result": "lane-error",
                }),
            );
            return 3;
        }
    };
    if !w_out.is_empty() {
        if let Err(refusal) = guard_output(
            &w_out,
            &[char_path.as_str(), lane_path.as_str()],
            force,
        ) {
            print_doc(out, &refusal_doc(&[], refusal.code, &refusal.message));
            return 1;
        }
        let text = to_d_json(&doc);
        let _ = std::fs::write(&w_out, format!("{text}\n"));
    }
    print_doc(out, &doc);
    if is_draft {
        0
    } else {
        1
    }
}

fn cmd_tutorial_list(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        let _ = writeln!(
            err,
            "usage: torogao-cli tutorial-list <pack> [--track <track>]"
        );
        return 3;
    }
    let mut track_name = "characters".to_string();
    let mut i = 2;
    while i + 1 < args.len() {
        if args[i] == "--track" {
            track_name = args[i + 1].clone();
        }
        i += 1;
    }
    match crate::tutorials::load_lessons(&args[1], &track_name) {
        Ok(lessons) => {
            print_doc(out, &crate::tutorials::lessons_summary(&lessons));
            0
        }
        Err(message) => {
            let _ = writeln!(err, "{message}");
            3
        }
    }
}

fn cmd_tutorial_status(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut progress_path = crate::paths::default_tutorial_progress_path().unwrap_or_default();
    let mut status_pack = String::new();
    let mut status_track = "characters".to_string();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--progress" && i + 1 < args.len() {
            progress_path = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--pack" && i + 1 < args.len() {
            status_pack = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--track" && i + 1 < args.len() {
            status_track = args[i + 1].clone();
            i += 1;
        }
        i += 1;
    }
    let progress = match crate::tutorials::load_progress(&progress_path) {
        Ok(p) => p,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let mut lessons_doc = Value::Null;
    if !status_pack.is_empty() {
        match crate::tutorials::load_lessons(&status_pack, &status_track) {
            Ok(d) => lessons_doc = d,
            Err(message) => {
                let _ = writeln!(err, "{message}");
                return 3;
            }
        }
    }
    let mut doc = crate::tutorials::progress_summary(&progress, &lessons_doc);
    doc["progressFile"] = json!(progress_path);
    print_doc(out, &doc);
    0
}

fn cmd_tutorial_check(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.len() < 4 {
        let _ = writeln!(
            err,
            "usage: torogao-cli tutorial-check <pack> <lesson-id> <project.inx> [--progress <path>]"
        );
        return 3;
    }
    let check_pack = args[1].clone();
    let lesson_id = args[2].clone();
    let project = args[3].clone();
    let mut progress_path = crate::paths::default_tutorial_progress_path().unwrap_or_default();
    let mut check_track = "characters".to_string();
    let mut i = 4;
    while i < args.len() {
        if args[i] == "--progress" && i + 1 < args.len() {
            progress_path = args[i + 1].clone();
            i += 1;
        } else if args[i] == "--track" && i + 1 < args.len() {
            check_track = args[i + 1].clone();
            i += 1;
        }
        i += 1;
    }
    let lessons = match crate::tutorials::load_lessons(&check_pack, &check_track) {
        Ok(l) => l,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let lesson = crate::tutorials::find_lesson(&lessons, &lesson_id);
    if !lesson.is_object() {
        print_doc(
            out,
            &json!({
                "code": crate::tutorials::TG_TUTORIAL_LESSON_UNKNOWN,
                "lesson": lesson_id,
                "pack": check_pack,
                "result": "unknown-lesson",
            }),
        );
        return 1;
    }
    let pack = match load_pack(&check_pack) {
        Ok(p) => p,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            return 3;
        }
    };
    let mut loaded = match load_project(&project) {
        Ok(l) => l,
        Err(e) => {
            print_doc(
                out,
                &json!({
                    "code": TG_CONTAINER_MALFORMED,
                    "error": e.message(),
                    "file": project,
                    "result": "unloadable",
                }),
            );
            return 2;
        }
    };
    let mut walk = Walk::build(&mut loaded.puppet);
    let view = ExtView::of_project(&loaded);
    let slot_count = loaded.puppet.texture_slots.len();
    let outcome = crate::tutorials::check_lesson(
        &mut loaded.puppet,
        &mut walk,
        &loaded.payload.clone(),
        slot_count,
        &pack,
        &jget(&lesson, "check"),
        &view,
    );
    let mut doc = json!({
        "evidence": outcome.evidence,
        "lesson": lesson_id,
        "pack": check_pack,
        "passed": outcome.passed,
        "project": project,
        "track": crate::packs::pack_str(&lessons, "track"),
    });
    if !outcome.passed {
        doc["code"] = json!(outcome.code);
    }
    if outcome.passed {
        let mut progress = match crate::tutorials::load_progress(&progress_path) {
            Ok(p) => p,
            Err(message) => {
                let _ = writeln!(err, "{message}");
                return 3;
            }
        };
        let is_new = crate::tutorials::record_completion(
            &mut progress,
            &check_pack,
            &crate::packs::pack_str(&lessons, "track"),
            &lesson,
            &project,
        );
        if let Err(message) = crate::tutorials::save_progress(&progress_path, &progress) {
            let _ = writeln!(err, "{message}");
            return 3;
        }
        doc["recorded"] = json!(is_new);
        doc["progressFile"] = json!(progress_path);
        doc["xpTotal"] = jget(&progress, "xpTotal").clone();
    }
    print_doc(out, &doc);
    if outcome.passed {
        0
    } else {
        1
    }
}
