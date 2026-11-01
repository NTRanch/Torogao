//! The face-neutral dispatcher: one verb (or one script) in, one
//! typed answer out. CLI and MCP faces call this and contain no
//! validation of their own; they only resolve project names to
//! directories (`resolve`) and print the answer.

use crate::census::{census, expression_test};
use crate::diff::diff;
use crate::limits::SCRIPT_OPS_MAX;
use crate::op::{IdInput, refuse_unimplemented, verb_spec};
use crate::refusal::{Refusal, RefusalClass, refuse};
use crate::store::{Loaded, Request, load, replay, run_script};
use crate::validate::validate;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The outcome class of an answer (faces map it to exit codes).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    /// The verb ran and found nothing wrong.
    Ok,
    /// Validation ran and reported findings.
    Findings,
    /// The verb (or a script op) was refused.
    Refused,
    /// The project could not be loaded.
    Unloadable,
}

/// One answer: a status and the JSON document to print.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// Outcome class.
    pub status: Status,
    /// The result document.
    pub doc: Value,
}

fn refused(verb: &str, refusal: &Refusal) -> Answer {
    Answer {
        status: Status::Refused,
        doc: json!({"code": refusal.code(), "message": refusal.message, "result": "refused", "verb": verb}),
    }
}

fn unloadable(verb: &str, refusal: &Refusal) -> Answer {
    Answer {
        status: Status::Unloadable,
        doc: json!({"code": refusal.code(), "message": refusal.message, "result": "unloadable", "verb": verb}),
    }
}

/// Inputs of verbs that take none: an empty object (any field,
/// including any override-style flag, is refused).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoInputs {}

/// `project_diff` inputs.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiffInput {
    other: String,
}

fn inputs_of<T: for<'de> Deserialize<'de>>(verb: &str, inputs: &Value) -> Result<T, Refusal> {
    serde_json::from_value(inputs.clone()).or_else(|err| {
        refuse(
            RefusalClass::Malformed,
            format!("inputs for '{verb}' do not match its schema: {err}"),
        )
    })
}

fn loaded(root: &Path) -> Result<Loaded, Refusal> {
    match load(root)? {
        Some(loaded) => Ok(loaded),
        None => refuse(
            RefusalClass::UnknownTarget,
            format!("no project exists at '{}'", root.display()),
        ),
    }
}

/// Runs one verb against the project at `root`. Relative source
/// paths resolve against `base`; `resolve` maps a project name
/// (for `project_diff`) to its directory.
pub fn run_verb(
    root: &Path,
    base: &Path,
    verb: &str,
    inputs: &Value,
    resolve: &dyn Fn(&str) -> Result<PathBuf, Refusal>,
) -> Answer {
    match verb_spec(verb) {
        Some(spec) if spec.slice == crate::op::Slice::A1 && spec.mutating => {
            let request = Request {
                verb: verb.to_string(),
                inputs: inputs.clone(),
            };
            return run_requests(root, base, &[request]);
        }
        Some(spec) if spec.slice == crate::op::Slice::A1 => {}
        _ => {
            let Err(refusal) = refuse_unimplemented::<()>(verb) else {
                unreachable!("refuse_unimplemented always refuses")
            };
            return refused(verb, &refusal);
        }
    }
    match query(root, verb, inputs, resolve) {
        Ok(answer) => answer,
        Err(Failure::Refused(refusal)) => refused(verb, &refusal),
        Err(Failure::Unloadable(refusal)) => unloadable(verb, &refusal),
    }
}

/// Which stage of a read-only verb failed.
enum Failure {
    /// Inputs or the computation were refused.
    Refused(Refusal),
    /// A project could not be loaded.
    Unloadable(Refusal),
}

fn parsed<T: for<'de> Deserialize<'de>>(verb: &str, inputs: &Value) -> Result<T, Failure> {
    inputs_of(verb, inputs).map_err(Failure::Refused)
}

fn open(root: &Path) -> Result<Loaded, Failure> {
    loaded(root).map_err(Failure::Unloadable)
}

fn query(
    root: &Path,
    verb: &str,
    inputs: &Value,
    resolve: &dyn Fn(&str) -> Result<PathBuf, Refusal>,
) -> Result<Answer, Failure> {
    let ok = |doc: Value| Answer {
        status: Status::Ok,
        doc,
    };
    match verb {
        "project_inspect" => {
            parsed::<NoInputs>(verb, inputs)?;
            let loaded = open(root)?;
            Ok(ok(
                json!({"census": census(&loaded.project), "result": "ok", "sha256": loaded.sha256}),
            ))
        }
        "project_validate" => {
            parsed::<NoInputs>(verb, inputs)?;
            let loaded = open(root)?;
            let findings = validate(&loaded.project, &loaded.text, root);
            let status = if findings.is_empty() {
                Status::Ok
            } else {
                Status::Findings
            };
            Ok(Answer {
                status,
                doc: json!({
                    "findings": findings.iter().map(Refusal::to_json).collect::<Vec<_>>(),
                    "result": if findings.is_empty() { "clean" } else { "findings" },
                    "sha256": loaded.sha256,
                }),
            })
        }
        "project_replay" => {
            parsed::<NoInputs>(verb, inputs)?;
            let report = replay(root).map_err(Failure::Refused)?;
            Ok(ok(json!({
                "denials": report.denials,
                "records": report.records,
                "result": "identical",
                "sha256": report.sha256,
            })))
        }
        "project_diff" => {
            let input: DiffInput = parsed(verb, inputs)?;
            let left = open(root)?;
            let other = resolve(&input.other).map_err(Failure::Refused)?;
            let right = open(&other)?;
            Ok(ok(
                json!({"diff": diff(&left.project, &right.project), "result": "ok"}),
            ))
        }
        "expression_test" => {
            let input: IdInput = parsed(verb, inputs)?;
            let loaded = open(root)?;
            let state = expression_test(&loaded.project, &input.id).map_err(Failure::Refused)?;
            Ok(ok(json!({"result": "ok", "state": state})))
        }
        other => unreachable!("verb '{other}' is A1, read-only, and unrouted"),
    }
}

/// Runs a parsed script transactionally.
pub fn run_requests(root: &Path, base: &Path, requests: &[Request]) -> Answer {
    match run_script(root, base, requests) {
        Ok(report) => Answer {
            status: Status::Ok,
            doc: json!({
                "applied": report.applied,
                "ops": report.ops,
                "result": if report.applied == 0 { "no-op" } else { "applied" },
                "sha256_after": report.sha256_after,
                "sha256_before": report.sha256_before,
            }),
        },
        Err(refusal) => Answer {
            status: Status::Refused,
            doc: json!({
                "code": refusal.refusal.code(),
                "index": refusal.index,
                "message": refusal.refusal.message,
                "result": "refused",
                "verb": refusal.verb,
            }),
        },
    }
}

/// Parses a script: JSONL, one `{"verb": ..., "inputs": {...}}`
/// object per non-empty line; lines starting with `#` are comments.
pub fn parse_script(text: &str) -> Result<Vec<Request>, Refusal> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Line {
        verb: String,
        inputs: Value,
    }
    let mut requests = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if requests.len() >= SCRIPT_OPS_MAX {
            return refuse(
                RefusalClass::Range,
                format!("a script carries at most {SCRIPT_OPS_MAX} ops"),
            );
        }
        let parsed: Line = serde_json::from_str(trimmed).or_else(|err| {
            refuse(
                RefusalClass::Malformed,
                format!(
                    "script line {} is not a {{verb, inputs}} object: {err}",
                    index + 1
                ),
            )
        })?;
        requests.push(Request {
            verb: parsed.verb,
            inputs: parsed.inputs,
        });
    }
    Ok(requests)
}

/// The implemented verb listing: name, group, mutating flag.
pub fn verbs_doc() -> Value {
    let implemented: Vec<Value> = crate::op::MANIFEST
        .iter()
        .filter(|spec| spec.slice == crate::op::Slice::A1)
        .map(|spec| json!({"group": spec.group, "mutating": spec.mutating, "verb": spec.name}))
        .collect();
    let deferred: Vec<Value> = crate::op::MANIFEST
        .iter()
        .filter(|spec| spec.slice != crate::op::Slice::A1)
        .map(|spec| json!({"group": spec.group, "slice": spec.slice.label(), "verb": spec.name}))
        .collect();
    json!({
        "deferred": deferred,
        "implemented": implemented,
        "implemented_count": implemented.len(),
        "manifest_count": crate::op::MANIFEST.len(),
    })
}
