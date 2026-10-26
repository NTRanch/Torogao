//! Tool handlers: one function per registry tool,
//! dispatched by name. The CLI subcommands and the
//! MCP/A2A surfaces run exactly these implementations.

use crate::criteria::{findings_json, validate_puppet};
use crate::extdoc::{
    ExtView, count_glue_keys, ext_summary, rating_blocks_export, rating_stamp, validate_container_file,
    validate_ext,
};
use crate::guards::{self, LoadedProject, load_project, payload_guarded};
use crate::inspect::{census, project_info_doc};
use crate::registry::ToolResult;
use crate::util::jget;
use crate::walk::Walk;
use serde_json::Value;

/// The loaded-project bundle the loading tools share.
pub struct ToolLoad {
    /// The project path as given.
    pub path: String,
    /// The loaded project.
    pub loaded: LoadedProject,
    /// The built walk.
    pub walk: Walk,
    /// The payload document (null when unparseable).
    pub payload: Value,
}

impl ToolLoad {
    /// The extension view of the loaded project.
    pub fn ext_view(&self) -> ExtView {
        ExtView::of_project(&self.loaded)
    }
}

/// Shared front half of the loading tools:
/// args -> loaded project + walk.
pub fn load_for_tool(args: &Value) -> Result<ToolLoad, ToolResult> {
    let path = crate::registry::string_arg(args, "project")?;
    if !std::path::Path::new(&path).exists() {
        return Err(ToolResult::fail(
            "project_not_found",
            &format!("project file not found: {path}"),
        ));
    }
    match load_project(&path) {
        Ok(mut loaded) => {
            let walk = Walk::build(&mut loaded.puppet);
            let payload = payload_guarded(&path);
            Ok(ToolLoad {
                path,
                loaded,
                walk,
                payload,
            })
        }
        Err(err) => Err(ToolResult::fail(err.code(), err.message())),
    }
}

/// Loads only the container + payload (no puppet) for
/// tools that work at the document level.
pub fn load_container_for_tool(args: &Value) -> Result<(String, LoadedProject), ToolResult> {
    let path = crate::registry::string_arg(args, "project")?;
    if !std::path::Path::new(&path).exists() {
        return Err(ToolResult::fail(
            "project_not_found",
            &format!("project file not found: {path}"),
        ));
    }
    match guards::load_project(&path) {
        Ok(loaded) => Ok((path, loaded)),
        Err(err) => Err(ToolResult::fail(err.code(), err.message())),
    }
}

fn tool_inspect(args: &Value) -> ToolResult {
    let load = match load_for_tool(args) {
        Ok(load) => load,
        Err(err) => return err,
    };
    let ext_view = load.ext_view();
    let mut summary = ext_summary(&ext_view.doc);
    let glue = count_glue_keys(&load.payload)
        + if ext_view.present {
            count_glue_keys(jget(&ext_view.doc, "foreign"))
        } else {
            0
        };
    summary["glueRecords"] = Value::from(glue);
    ToolResult::ok(serde_json::json!({
        "census": census(&load.loaded.puppet, &load.walk),
        "extension": summary,
        "file": load.path,
    }))
}

/// The validate findings for a loaded project (the CLI
/// and the tool share this composition).
pub fn validate_findings(load: &ToolLoad) -> Vec<crate::criteria::Finding> {
    let puppet = &load.loaded.puppet;
    let mut findings = validate_puppet(
        puppet,
        &load.walk,
        &load.payload,
        puppet.texture_slots.len(),
    );
    findings.extend(validate_ext(puppet, &load.walk, &load.payload));
    findings.extend(validate_container_file(&load.path));
    findings
}

fn tool_validate(args: &Value) -> ToolResult {
    let load = match load_for_tool(args) {
        Ok(load) => load,
        Err(err) => return err,
    };
    let findings = validate_findings(&load);
    ToolResult::ok(serde_json::json!({
        "file": load.path,
        "findings": findings_json(&findings),
        "result": if findings.is_empty() { "clean" } else { "findings" },
    }))
}

fn tool_project_info(args: &Value) -> ToolResult {
    let path = match crate::registry::string_arg(args, "project") {
        Ok(path) => path,
        Err(err) => return err,
    };
    if !std::path::Path::new(&path).exists() {
        return ToolResult::fail(
            "project_not_found",
            &format!("project file not found: {path}"),
        );
    }
    match project_info_doc(&path) {
        Ok(doc) => ToolResult::ok(doc),
        Err(message) => ToolResult::fail(
            "project_unloadable",
            &format!("INP payload unreadable: {message}"),
        ),
    }
}

/// The file base name with its last extension stripped
/// (the reference's baseName/stripExtension pair).
pub fn base_stem(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    match base.rfind('.') {
        Some(pos) if pos > 0 => base[..pos].to_string(),
        _ => base.to_string(),
    }
}

/// Joins a directory and a file name with '/'.
pub fn join_path(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn tool_export_rig(args: &Value) -> ToolResult {
    let load = match load_for_tool(args) {
        Ok(load) => load,
        Err(err) => return err,
    };
    let out_dir = match crate::registry::string_arg(args, "out") {
        Ok(out) => out,
        Err(err) => return err,
    };
    let ext_view = load.ext_view();
    if ext_view.present {
        if let Some((_, reason)) = rating_blocks_export(&ext_view) {
            return ToolResult::fail("TG-RATING-LANE-MISMATCH", &reason);
        }
    }
    let base = base_stem(&load.path);
    let names = [
        format!("{base}.model.json"),
        format!("{base}.export-report.json"),
        format!("{base}.stamp.json"),
    ];
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    if let Err(refusal) = guards::guard_output_files(
        &out_dir,
        &name_refs,
        &[load.path.as_str()],
        guards::tool_force(args),
    ) {
        return ToolResult::fail(refusal.code, &refusal.message);
    }
    crate::rigexport::export_rig(&load, &out_dir, &base, &ext_view)
}

/// The rating stamp document for the export sidecar.
pub fn stamp_doc(file: &str, model_path: &str, ext_doc: &Value) -> Value {
    serde_json::json!({
        "file": file,
        "format": "torogao-rating-stamp/1",
        "model": model_path,
        "stamp": rating_stamp(ext_doc),
    })
}

/// Dispatches a registered, enabled tool by name.
pub fn dispatch(name: &str, args: &Value) -> ToolResult {
    match name {
        "inspect" => tool_inspect(args),
        "validate" => tool_validate(args),
        "project-info" => tool_project_info(args),
        "export-rig" => tool_export_rig(args),
        _ => crate::families::dispatch_family(name, args),
    }
}
