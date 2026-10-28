//! Dispatch: a parsed command becomes an operation on an
//! existing surface, or a typed refusal. Voice adds no
//! authority: registry tools run through the same
//! `call_tool` every surface uses, disabled tools are refused
//! by the registry itself, and targets whose execution
//! surface does not exist yet refuse `TG-VOICE-NOT-BOUND`
//! instead of pretending.

use crate::grammar::{ConfirmClass, Target};
use crate::parse::ParsedCommand;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// Refusal code: the command parsed, but its execution
/// surface is not built/bound yet.
pub const TG_VOICE_NOT_BOUND: &str = "TG-VOICE-NOT-BOUND";
/// Refusal code: a spoken reference could not be resolved.
pub const TG_VOICE_UNRESOLVED_REF: &str = "TG-VOICE-UNRESOLVED-REF";

/// Bounds for the project-index scan.
const INDEX_DEPTH_MAX: usize = 4;
/// Bounds for the project-index scan.
const INDEX_FILES_MAX: usize = 4096;

/// Session context the dispatcher resolves references
/// against: the project index (file stem → path) and the
/// session's current project.
#[derive(Debug, Clone, Default)]
pub struct Session {
    /// Project file stem (lowercase) → full path.
    pub project_index: BTreeMap<String, String>,
    /// The session's current project path, for
    /// "current project" references.
    pub default_project: Option<String>,
}

impl Session {
    /// Builds a session by scanning `root` for project files
    /// (`.inx` / `.inpx`), bounded in depth and count.
    pub fn scan(root: &Path) -> Session {
        let mut index = BTreeMap::new();
        scan_dir(root, 0, &mut index);
        Session {
            project_index: index,
            default_project: None,
        }
    }

    /// Sets the session's current project.
    pub fn with_default_project(mut self, path: &str) -> Session {
        self.default_project = Some(path.to_string());
        self
    }
}

/// Recursive bounded scan helper for [`Session::scan`].
fn scan_dir(dir: &Path, depth: usize, index: &mut BTreeMap<String, String>) {
    if depth > INDEX_DEPTH_MAX || index.len() >= INDEX_FILES_MAX {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = read.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if index.len() >= INDEX_FILES_MAX {
            return;
        }
        if path.is_dir() {
            scan_dir(&path, depth + 1, index);
        } else if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            let lower = name.to_lowercase();
            if lower.ends_with(".inx") || lower.ends_with(".inpx") {
                if let Some(stem) = Path::new(&lower).file_stem().and_then(|stem| stem.to_str()) {
                    index
                        .entry(stem.to_string())
                        .or_insert_with(|| path.to_string_lossy().to_string());
                }
            }
        }
    }
}

/// What dispatch did with a parsed command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecStatus {
    /// The command ran; `payload` carries the result.
    Executed,
    /// The command is held for an explicit confirm act.
    ConfirmationRequired,
    /// A typed refusal; nothing ran.
    Refused,
    /// The stop outcome fired.
    Stopped,
}

/// The dispatch report: the record a surface renders and,
/// for executed commands, the shape that joins the op log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExecReport {
    /// What happened.
    pub status: ExecStatus,
    /// The grammar entry that was dispatched.
    pub entry_id: String,
    /// The result payload when executed.
    pub payload: Value,
    /// Stable machine code when refused (`TG-VOICE-*` or the
    /// registry's own code, passed through untouched).
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl ExecReport {
    /// A refusal report.
    fn refused(entry_id: &str, code: &str, message: &str) -> ExecReport {
        ExecReport {
            status: ExecStatus::Refused,
            entry_id: entry_id.to_string(),
            payload: Value::Null,
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

/// The report for the stop outcome. In the app this binds to
/// the synchronous total-stop path; the one-shot CLI has no
/// running operation to halt, and says so.
pub fn stop_report() -> ExecReport {
    ExecReport {
        status: ExecStatus::Stopped,
        entry_id: "safety.stop".to_string(),
        payload: Value::Null,
        code: String::new(),
        message: "stop outcome: in the app this fires the synchronous total-stop path; \
                  the one-shot CLI has no running operation to halt"
            .to_string(),
    }
}

/// Resolves the `project` argument of a command against the
/// session. A literal path (separator or project extension)
/// passes verbatim; "current" takes the session default; a
/// bare name resolves through the index.
fn resolve_project_arg(command: &ParsedCommand, session: &Session) -> Result<Value, ExecReport> {
    let mut args = command.args.clone();
    let Some(raw) = args.get("project").and_then(Value::as_str) else {
        return Ok(args);
    };
    let resolved = if raw == "current" {
        Some(session.default_project.clone().ok_or_else(|| {
            ExecReport::refused(
                &command.entry_id,
                TG_VOICE_UNRESOLVED_REF,
                "no current project in this session",
            )
        })?)
    } else if raw.contains('/') || raw.ends_with(".inx") || raw.ends_with(".inpx") {
        Some(raw.to_string())
    } else {
        match session.project_index.get(&raw.to_lowercase()) {
            Some(path) => Some(path.clone()),
            None => {
                return Err(ExecReport::refused(
                    &command.entry_id,
                    TG_VOICE_UNRESOLVED_REF,
                    &format!(
                        "no project named '{raw}' in the session index ({} known)",
                        session.project_index.len()
                    ),
                ));
            }
        }
    };
    if let Some(path) = resolved {
        args["project"] = Value::String(path);
    }
    Ok(args)
}

/// Executes one parsed command. `confirmed` is the explicit
/// confirm act for confirm-class commands (the spoken word
/// "confirm" inside its window, or a tap/keypress — the
/// surface owns the window; this layer owns the rule that an
/// unconfirmed confirm-class command never runs).
pub fn execute(command: &ParsedCommand, confirmed: bool, session: &Session) -> ExecReport {
    if command.confirm == ConfirmClass::Confirm && !confirmed {
        return ExecReport {
            status: ExecStatus::ConfirmationRequired,
            entry_id: command.entry_id.clone(),
            payload: json!({
                "command": command.entry_id.clone(),
                "args": command.args.clone(),
            }),
            code: String::new(),
            message: "parsed and held: an explicit confirm act is required before this runs"
                .to_string(),
        };
    }
    match &command.target {
        Target::RegistryTool { name } => {
            let args = match resolve_project_arg(command, session) {
                Ok(args) => args,
                Err(report) => return report,
            };
            let result = torogao_cli::registry::call_tool(name, &args);
            if result.is_error {
                ExecReport::refused(&command.entry_id, &result.error_code, &result.error_message)
            } else {
                ExecReport {
                    status: ExecStatus::Executed,
                    entry_id: command.entry_id.clone(),
                    payload: result.payload,
                    code: String::new(),
                    message: String::new(),
                }
            }
        }
        Target::CliMode { name } if name == "paths" || name == "registry-dump" => {
            let mut out: Vec<u8> = Vec::new();
            let mut err: Vec<u8> = Vec::new();
            let code = torogao_cli::run(std::slice::from_ref(name), &mut out, &mut err);
            if code == 0 {
                let text = String::from_utf8_lossy(&out);
                let payload = serde_json::from_str(text.trim()).unwrap_or(Value::Null);
                ExecReport {
                    status: ExecStatus::Executed,
                    entry_id: command.entry_id.clone(),
                    payload,
                    code: String::new(),
                    message: String::new(),
                }
            } else {
                ExecReport::refused(
                    &command.entry_id,
                    "cli_mode_failed",
                    String::from_utf8_lossy(&err).trim(),
                )
            }
        }
        Target::CliMode { name } => ExecReport::refused(
            &command.entry_id,
            TG_VOICE_NOT_BOUND,
            &format!("'{name}' is a long-running server mode; start it from the terminal"),
        ),
        Target::AuthorVerb { name } => ExecReport::refused(
            &command.entry_id,
            TG_VOICE_NOT_BOUND,
            &format!(
                "author verb '{name}' parsed; the torogao-author execution surface \
                 (load → decide() → apply) is not built yet — the parse is the record"
            ),
        ),
        Target::AppCommand { name } => ExecReport::refused(
            &command.entry_id,
            TG_VOICE_NOT_BOUND,
            &format!("app command '{name}' parsed; the app shell binding lands with slice V3"),
        ),
        Target::SafetyStop => stop_report(),
    }
}
