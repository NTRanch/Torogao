//! The project directory: load, transactional script runs, the
//! op log, and replay.
//!
//! Layout (brief §Item 3): `project.ron`, `ops.jsonl`, `assets/`
//! (content-hashed). A run is transactional: every op is applied
//! to a working copy through `step` (decide, then apply); only
//! when the whole run is admitted are asset files written, then
//! `project.ron` via temp file + rename, then the log records
//! appended. A refused run never opens `project.ron` for writing;
//! it appends one denial record to `ops.jsonl` (when the project
//! exists) naming the refused op.
//!
//! Concurrency: one run per project at a time, enforced by an
//! exclusive `.lock` file; a second run is refused `TG-AUTH-IO`,
//! never queued.

use crate::apply::step;
use crate::codec::{emit_project, parse_project};
use crate::decide::Observed;
use crate::import::{observe_model, read_texture_live, texture_sources};
use crate::intake::{observe_png, observe_psd, read_bounded, resolve_source, sha256_hex};
use crate::limits::{
    IMAGE_SOURCE_BYTES_MAX, MODEL_SOURCE_BYTES_MAX, OPS_LOG_BYTES_MAX, OPS_LOG_RECORDS_MAX,
    PROJECT_BYTES_MAX, PSD_SOURCE_BYTES_MAX, SCRIPT_OPS_MAX,
};
use crate::model::Project;
use crate::op::Op;
use crate::refusal::{Refusal, RefusalClass, refuse};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use torogao_format::jsonfmt::to_d_json;

/// The canonical project file name.
pub const PROJECT_FILE: &str = "project.ron";
/// The op log file name.
pub const OPS_FILE: &str = "ops.jsonl";
/// The lock file name.
pub const LOCK_FILE: &str = ".lock";

fn io_refusal(what: &str, path: &Path, err: std::io::Error) -> Refusal {
    Refusal::new(
        RefusalClass::Io,
        format!("{what} '{}': {err}", path.display()),
    )
}

/// A loaded project file: the parsed project, the exact bytes on
/// disk, and their sha256.
#[derive(Clone, Debug)]
pub struct Loaded {
    /// The parsed project.
    pub project: Project,
    /// The file text as read.
    pub text: String,
    /// sha256 of the file bytes.
    pub sha256: String,
}

/// Loads `project.ron` (None when the file does not exist).
pub fn load(root: &Path) -> Result<Option<Loaded>, Refusal> {
    let path = root.join(PROJECT_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = read_bounded(&path, PROJECT_BYTES_MAX)?;
    let Ok(text) = String::from_utf8(bytes) else {
        return refuse(RefusalClass::Malformed, "project.ron is not UTF-8");
    };
    let project = parse_project(&text)?;
    let sha256 = sha256_hex(text.as_bytes());
    Ok(Some(Loaded {
        project,
        text,
        sha256,
    }))
}

/// The sha256 a project's canonical bytes would have.
pub fn project_sha256(project: &Project) -> String {
    sha256_hex(emit_project(project).as_bytes())
}

/// An exclusive per-project lock, released on drop.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
}

impl Lock {
    /// Takes the lock, refusing when another run holds it.
    pub fn take(root: &Path) -> Result<Lock, Refusal> {
        let path = root.join(LOCK_FILE);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => Ok(Lock { path }),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => refuse(
                RefusalClass::Io,
                format!(
                    "project is locked by another run ('{}' exists)",
                    path.display()
                ),
            ),
            Err(err) => Err(io_refusal("cannot create lock", &path, err)),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Best effort: a lock left behind refuses later runs
        // loudly (TG-AUTH-IO naming the file), never silently.
        let _removed = std::fs::remove_file(&self.path);
    }
}

/// One op as a face hands it over: a verb name and JSON inputs.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// The verb name.
    pub verb: String,
    /// The inputs object.
    pub inputs: Value,
}

/// Where intake ops read their sources.
pub enum SourceMode<'a> {
    /// From the filesystem; relative paths resolve against `base`.
    Live {
        /// The directory relative source paths resolve against.
        base: &'a Path,
    },
    /// From the project's own content-hashed asset store.
    Replay {
        /// The project directory.
        root: &'a Path,
    },
}

fn read_stored(root: &Path, sha256: &str, extension: &str) -> Result<Vec<u8>, Refusal> {
    let path = root.join("assets").join(format!("{sha256}.{extension}"));
    let bytes = read_bounded(&path, PSD_SOURCE_BYTES_MAX).map_err(|refusal| {
        Refusal::new(
            RefusalClass::ReplayDivergence,
            format!(
                "asset store lacks a source the log names: {}",
                refusal.message
            ),
        )
    })?;
    if sha256_hex(&bytes) != sha256 {
        return refuse(
            RefusalClass::ReplayDivergence,
            format!(
                "stored source '{}' does not hash to its name",
                path.display()
            ),
        );
    }
    Ok(bytes)
}

fn logged_sha(pinned: &Option<String>) -> Result<&str, Refusal> {
    pinned.as_deref().map_or_else(
        || {
            refuse(
                RefusalClass::ReplayDivergence,
                "a logged intake op lacks its source_sha256",
            )
        },
        Ok,
    )
}

/// Reads an op's sources and normalizes its inputs: source paths
/// become canonical and unpinned hashes are filled in, so the op
/// log records exactly what was read.
pub fn prepare(op: Op, mode: &SourceMode<'_>) -> Result<(Op, Observed), Refusal> {
    match op {
        Op::IntakeImage(mut inputs) => {
            let (path, bytes) = match mode {
                SourceMode::Live { base } => {
                    let path = resolve_source(&inputs.path, base)?;
                    let bytes = read_bounded(&path, IMAGE_SOURCE_BYTES_MAX)?;
                    (path, bytes)
                }
                SourceMode::Replay { root } => (
                    PathBuf::from(&inputs.path),
                    read_stored(root, logged_sha(&inputs.source_sha256)?, "png")?,
                ),
            };
            let image = observe_png(&path, bytes)?;
            inputs.path = image.canonical_path.clone();
            inputs
                .source_sha256
                .get_or_insert_with(|| image.sha256.clone());
            Ok((Op::IntakeImage(inputs), Observed::Image(image)))
        }
        Op::IntakePsd(mut inputs) => {
            let (path, bytes) = match mode {
                SourceMode::Live { base } => {
                    let path = resolve_source(&inputs.path, base)?;
                    let bytes = read_bounded(&path, PSD_SOURCE_BYTES_MAX)?;
                    (path, bytes)
                }
                SourceMode::Replay { root } => (
                    PathBuf::from(&inputs.path),
                    read_stored(root, logged_sha(&inputs.source_sha256)?, "psd")?,
                ),
            };
            let psd = observe_psd(&path, bytes)?;
            inputs.path = psd.canonical_path.clone();
            inputs
                .source_sha256
                .get_or_insert_with(|| psd.sha256.clone());
            Ok((Op::IntakePsd(inputs), Observed::Psd(psd)))
        }
        Op::ProjectImport(inputs) => prepare_import(inputs, mode),
        other => Ok((other, Observed::Nothing)),
    }
}

fn prepare_import(
    mut inputs: crate::op::ImportInput,
    mode: &SourceMode<'_>,
) -> Result<(Op, Observed), Refusal> {
    let model = match mode {
        SourceMode::Live { base } => {
            let path = resolve_source(&inputs.path, base)?;
            let bytes = read_bounded(&path, MODEL_SOURCE_BYTES_MAX)?;
            let directory = path.parent().unwrap_or(Path::new("/")).to_path_buf();
            observe_model(&path, bytes, |_, source| {
                read_texture_live(&directory, source)
            })?
        }
        SourceMode::Replay { root } => {
            let bytes = read_stored(root, logged_sha(&inputs.source_sha256)?, "model.json")?;
            let textures = inputs.textures.clone().unwrap_or_default();
            observe_model(Path::new(&inputs.path), bytes, |index, _| {
                let Some(texture) = textures.get(index) else {
                    return refuse(
                        RefusalClass::ReplayDivergence,
                        "a logged import lacks one of its texture hashes",
                    );
                };
                let bytes = read_stored(root, &texture.sha256, "png")?;
                Ok((PathBuf::from(&texture.path), bytes))
            })?
        }
    };
    inputs.path = model.canonical_path.clone();
    inputs
        .source_sha256
        .get_or_insert_with(|| model.sha256.clone());
    inputs
        .textures
        .get_or_insert_with(|| texture_sources(&model));
    Ok((Op::ProjectImport(inputs), Observed::Model(model)))
}

/// The outcome of an admitted run.
#[derive(Clone, Debug, PartialEq)]
pub struct RunReport {
    /// Ops admitted (applied or no-op).
    pub ops: usize,
    /// Ops that changed the project.
    pub applied: usize,
    /// sha256 of `project.ron` before the run ("absent" if new).
    pub sha256_before: String,
    /// sha256 of `project.ron` after the run.
    pub sha256_after: String,
}

/// A refused run: which op (by index in the run) and why.
#[derive(Clone, Debug, PartialEq)]
pub struct RunRefusal {
    /// Index of the refused op within the run.
    pub index: usize,
    /// The verb, as requested.
    pub verb: String,
    /// The refusal.
    pub refusal: Refusal,
}

struct Admitted {
    project: Project,
    records: Vec<(String, Value, String)>,
    files: Vec<(String, Vec<u8>)>,
    applied: usize,
}

fn admit_all(
    state: Option<Project>,
    requests: &[Request],
    base: &Path,
) -> Result<Admitted, RunRefusal> {
    let mut state = state;
    let mut admitted = Admitted {
        project: Project::default(),
        records: Vec::with_capacity(requests.len()),
        files: Vec::new(),
        applied: 0,
    };
    for (index, request) in requests.iter().enumerate() {
        let fail = |refusal: Refusal| RunRefusal {
            index,
            verb: request.verb.clone(),
            refusal,
        };
        let op = Op::parse(&request.verb, &request.inputs).map_err(fail)?;
        let (op, observed) = prepare(op, &SourceMode::Live { base }).map_err(fail)?;
        let stepped = step(state.take(), &op, &observed).map_err(fail)?;
        if stepped.verdict == crate::decide::Verdict::Apply {
            admitted.applied += 1;
        }
        for file in stepped.files {
            admitted.files.push((file.path, file.bytes.to_vec()));
        }
        let inputs = op.inputs_json().map_err(fail)?;
        let sha = project_sha256(&stepped.project);
        admitted.records.push((op.verb().to_string(), inputs, sha));
        state = Some(stepped.project);
    }
    admitted.project = state.unwrap_or_default();
    Ok(admitted)
}

/// Runs a script (one or more ops) against the project at `root`,
/// transactionally. Relative source paths resolve against `base`.
pub fn run_script(root: &Path, base: &Path, requests: &[Request]) -> Result<RunReport, RunRefusal> {
    let whole = |refusal: Refusal| RunRefusal {
        index: 0,
        verb: requests.first().map(|r| r.verb.clone()).unwrap_or_default(),
        refusal,
    };
    if requests.is_empty() || requests.len() > SCRIPT_OPS_MAX {
        return Err(whole(Refusal::new(
            RefusalClass::Range,
            format!("a run carries 1..={SCRIPT_OPS_MAX} ops"),
        )));
    }
    let created = if root.join(PROJECT_FILE).exists() {
        false
    } else if requests[0].verb == "project_init" {
        create_project_dir(root).map_err(whole)?
    } else {
        return Err(whole(Refusal::new(
            RefusalClass::UnknownTarget,
            format!(
                "no project exists at '{}'; run project_init first",
                root.display()
            ),
        )));
    };
    let lock = Lock::take(root).map_err(whole)?;
    let loaded = load(root).map_err(whole)?;
    let sha256_before = loaded
        .as_ref()
        .map_or_else(|| "absent".to_string(), |l| l.sha256.clone());
    let state = loaded.map(|l| l.project);
    let admitted = match admit_all(state, requests, base) {
        Ok(admitted) => admitted,
        Err(refused) => {
            if created {
                drop(lock);
                // Only the directory this run created, and only if
                // still empty; never anything pre-existing.
                let _removed = std::fs::remove_dir(root);
            } else {
                append_denial(
                    root,
                    &requests[refused.index],
                    &refused.refusal,
                    &sha256_before,
                )
                .map_err(whole)?;
            }
            return Err(refused);
        }
    };
    commit(root, &admitted).map_err(whole)?;
    drop(lock);
    Ok(RunReport {
        ops: requests.len(),
        applied: admitted.applied,
        sha256_before,
        sha256_after: project_sha256(&admitted.project),
    })
}

/// Creates the project directory (and its parents). Returns true
/// when this call created it.
fn create_project_dir(root: &Path) -> Result<bool, Refusal> {
    if root.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(root).map_err(|err| io_refusal("cannot create", root, err))?;
    Ok(true)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    let temp = path.with_extension("tmp-write");
    let result = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    result.map_err(|err| {
        let _removed = std::fs::remove_file(&temp);
        io_refusal("cannot write", path, err)
    })
}

fn commit(root: &Path, admitted: &Admitted) -> Result<(), Refusal> {
    if !admitted.files.is_empty() {
        let assets = root.join("assets");
        std::fs::create_dir_all(&assets)
            .map_err(|err| io_refusal("cannot create", &assets, err))?;
    }
    for (relative, bytes) in &admitted.files {
        let path = root.join(relative);
        if !path.exists() {
            write_atomic(&path, bytes)?;
        }
    }
    write_atomic(
        &root.join(PROJECT_FILE),
        emit_project(&admitted.project).as_bytes(),
    )?;
    let mut seq = read_log(root)?.len();
    let mut lines = String::new();
    for (verb, inputs, sha) in &admitted.records {
        seq += 1;
        let record = json!({
            "inputs": inputs,
            "project_sha256_after": sha,
            "seq": seq,
            "verb": verb,
        });
        lines.push_str(&to_d_json(&record));
        lines.push('\n');
    }
    append_log(root, &lines)
}

fn append_log(root: &Path, lines: &str) -> Result<(), Refusal> {
    let path = root.join(OPS_FILE);
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        file.write_all(lines.as_bytes())?;
        file.sync_all()
    })();
    result.map_err(|err| io_refusal("cannot append to", &path, err))
}

fn append_denial(
    root: &Path,
    request: &Request,
    refusal: &Refusal,
    sha: &str,
) -> Result<(), Refusal> {
    let seq = read_log(root)?.len() + 1;
    let record = json!({
        "inputs": request.inputs,
        "project_sha256": sha,
        "refusal": refusal.to_json(),
        "seq": seq,
        "verb": request.verb,
    });
    append_log(root, &format!("{}\n", to_d_json(&record)))
}

/// Reads the op log's records (bounded), each a JSON object.
pub fn read_log(root: &Path) -> Result<Vec<Value>, Refusal> {
    let path = root.join(OPS_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = read_bounded(&path, OPS_LOG_BYTES_MAX)?;
    let Ok(text) = String::from_utf8(bytes) else {
        return refuse(RefusalClass::ReplayDivergence, "ops.jsonl is not UTF-8");
    };
    let mut records = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if records.len() >= OPS_LOG_RECORDS_MAX {
            return refuse(RefusalClass::Range, "ops.jsonl exceeds the record bound");
        }
        let record: Value = serde_json::from_str(line).or_else(|_| {
            refuse(
                RefusalClass::ReplayDivergence,
                format!("ops.jsonl line {} is not a JSON object", line_index + 1),
            )
        })?;
        records.push(record);
    }
    Ok(records)
}

/// The result of a verified replay.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayReport {
    /// Log records read.
    pub records: usize,
    /// Of those, denial records (verified, not applied).
    pub denials: usize,
    /// sha256 of the replayed project (equals `project.ron`'s).
    pub sha256: String,
}

fn divergence<T>(seq: usize, message: impl std::fmt::Display) -> Result<T, Refusal> {
    refuse(
        RefusalClass::ReplayDivergence,
        format!("op log seq {seq}: {message}"),
    )
}

/// Rebuilds the project from empty by applying `ops.jsonl` in
/// order (sources from the asset store), verifying every recorded
/// hash and finally the bytes of `project.ron`. Read-only: it
/// writes nothing.
pub fn replay(root: &Path) -> Result<ReplayReport, Refusal> {
    let records = read_log(root)?;
    let mut state: Option<Project> = None;
    let mut denials = 0;
    for (index, record) in records.iter().enumerate() {
        let seq = index + 1;
        if record.get("seq").and_then(Value::as_u64) != Some(seq as u64) {
            return divergence(seq, "sequence number is missing or out of order");
        }
        let current = state
            .as_ref()
            .map_or_else(|| "absent".to_string(), project_sha256);
        if let Some(recorded) = record.get("refusal") {
            if record.get("project_sha256").and_then(Value::as_str) != Some(current.as_str()) {
                return divergence(
                    seq,
                    format!("denial ({recorded}) recorded a different project"),
                );
            }
            denials += 1;
            continue;
        }
        let (Some(verb), Some(inputs), Some(after)) = (
            record.get("verb").and_then(Value::as_str),
            record.get("inputs"),
            record.get("project_sha256_after").and_then(Value::as_str),
        ) else {
            return divergence(seq, "record lacks verb, inputs, or project_sha256_after");
        };
        let op = Op::parse(verb, inputs).or_else(|r| divergence(seq, r))?;
        let (op, observed) =
            prepare(op, &SourceMode::Replay { root }).or_else(|r| divergence(seq, r))?;
        let stepped = step(state.take(), &op, &observed).or_else(|r| divergence(seq, r))?;
        let sha = project_sha256(&stepped.project);
        if sha != after {
            return divergence(
                seq,
                format!("replay produced {sha}, the log recorded {after}"),
            );
        }
        state = Some(stepped.project);
    }
    let Some(project) = state else {
        return refuse(RefusalClass::ReplayDivergence, "the op log applies no ops");
    };
    let replayed = emit_project(&project);
    let on_disk = load(root)?.map(|l| l.sha256).unwrap_or_default();
    let sha256 = sha256_hex(replayed.as_bytes());
    if sha256 != on_disk {
        return refuse(
            RefusalClass::ReplayDivergence,
            format!("replay produced {sha256}; project.ron on disk is {on_disk}"),
        );
    }
    Ok(ReplayReport {
        records: records.len(),
        denials,
        sha256,
    })
}
