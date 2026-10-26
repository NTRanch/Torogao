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
        Some("pack-info") => Some(cmd_pack_info(args, out, err)),
        Some("sfx-list") => Some(cmd_sfx_list(out, err)),
        Some("voice-check") => Some(cmd_voice_check(args, out, err)),
        Some("create-character") => Some(cmd_create_character(args, out, err)),
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
