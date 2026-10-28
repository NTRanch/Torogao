//! The live session: genesis state, current state, the
//! append-only op log, and the gate mirror every op is
//! decided through.
//!
//! The op pipeline is three stages so a host can interleave
//! its live application between decision and record:
//! `admit` (ingress finiteness + the gate's `decide()` +
//! session-local validation) → the host applies the op to
//! its live scene → `commit` (state apply, gate mirror step,
//! log append). `apply_op` runs admit + commit back to back
//! for hosts without a live scene (replay, CLI rebuilds).
//! A refusal at any stage leaves the state, the mirror, and
//! the log untouched.

use crate::canon::{from_canon_ron, state_hash, to_canon_ron};
use crate::gate::{GateMirror, GateVerdict};
use crate::op::Op;
use crate::refusal::{R, Refusal};
use crate::state::{EditorState, Transform, Viewport};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogEntry {
    pub seq: u64,
    pub session: String,
    pub op: serde_json::Value,
    pub pre: String,
    pub post: String,
}

/// An op the gate has admitted, awaiting commit. Carries the
/// op and the gate's verdict; a host applies the op to its
/// live scene between `Session::admit` and `Session::commit`.
#[derive(Debug, Clone)]
pub struct Admission {
    op: Op,
    verdict: GateVerdict,
}

impl Admission {
    /// The admitted op.
    pub fn op(&self) -> &Op {
        &self.op
    }

    /// The gate's verdict on the op.
    pub fn verdict(&self) -> GateVerdict {
        self.verdict
    }
}

/// A live session: genesis state, current state, the
/// append-only log, and the gate mirror.
#[derive(Debug, Clone)]
pub struct Session {
    /// Session id: `ses-<first 12 hex of the genesis hash>`.
    pub id: String,
    /// The state the session started from.
    pub genesis: EditorState,
    /// The current state.
    pub state: EditorState,
    /// The append-only op log.
    pub log: Vec<LogEntry>,
    /// Who holds this session (provenance for the record's
    /// `held_by` line; a record line, never a protocol
    /// difference).
    pub held_by: String,
    gate: GateMirror,
    param_defaults: BTreeMap<String, f32>,
}

impl Session {
    /// Opens a session on a genesis state. `param_defaults`
    /// carries each projected parameter's rig-declared
    /// default; the gate mirror is built from both.
    pub fn new(
        genesis: EditorState,
        param_defaults: &BTreeMap<String, f32>,
        held_by: &str,
    ) -> Session {
        let gh = state_hash(&genesis);
        let gate = GateMirror::build(&genesis, param_defaults);
        Session {
            id: format!("ses-{}", &gh[..12]),
            state: genesis.clone(),
            genesis,
            log: Vec::new(),
            held_by: held_by.to_string(),
            gate,
            param_defaults: param_defaults.clone(),
        }
    }

    /// Rebuilds a session for replaying a record: the
    /// authoring defaults are the genesis values themselves
    /// (each parameter starts neutral at its genesis value).
    /// Refuse/allow decisions are identical to the live
    /// session's — the routed verbs check existence and range
    /// only — while the pose bookkeeping stays consistent.
    pub fn from_genesis_replay(genesis: EditorState) -> Session {
        let defaults: BTreeMap<String, f32> = genesis
            .params
            .iter()
            .map(|(name, p)| (name.clone(), p.value))
            .collect();
        Session::new(genesis, &defaults, "record replay")
    }

    /// The genesis state's hash.
    pub fn genesis_hash(&self) -> String {
        state_hash(&self.genesis)
    }

    /// The current state's hash.
    pub fn state_hash(&self) -> String {
        state_hash(&self.state)
    }

    /// Stage 1: ingress + gate. Non-finite op floats refuse
    /// `TG-SES-NONFINITE` at ingress; authoring ops are
    /// decided by `torogao-author`'s `decide()`; session-local
    /// ops are validated under the session-local rules.
    pub fn admit(&self, op: &Op) -> R<Admission> {
        for v in op.floats() {
            if !v.is_finite() {
                return Err(Refusal::new(
                    "TG-SES-NONFINITE",
                    format!("op value {v} is not finite"),
                ));
            }
        }
        let verdict = self.gate.decide(op)?;
        if verdict == GateVerdict::SessionLocal {
            validate_session_local(&self.state, op)?;
        }
        Ok(Admission {
            op: op.clone(),
            verdict,
        })
    }

    /// Stage 3: apply an admitted op to the state, step the
    /// gate mirror, and append the log entry. Callers that
    /// interleave live application (see `apply_op`) must not
    /// commit an admission twice.
    pub fn commit(&mut self, admission: Admission) -> R<LogEntry> {
        let pre = self.state_hash();
        self.gate.commit(&admission.op)?;
        apply(&mut self.state, &admission.op);
        let post = self.state_hash();
        let entry = LogEntry {
            seq: self.log.len() as u64 + 1,
            session: self.id.clone(),
            op: admission.op.to_json(),
            pre,
            post,
        };
        self.log.push(entry.clone());
        Ok(entry)
    }

    /// Admit + commit with nothing in between: the pipeline
    /// replay and CLI rebuilds use.
    pub fn apply_op(&mut self, op: &Op) -> R<LogEntry> {
        let admission = self.admit(op)?;
        self.commit(admission)
    }

    /// Replays the session's own log against genesis through
    /// a fresh gate mirror built with this session's
    /// defaults; returns the replayed final hash.
    pub fn replay_check(&self) -> R<String> {
        let mut replay = Session::new(self.genesis.clone(), &self.param_defaults, "replay check");
        replay.id = self.id.clone();
        replay_log(&mut replay, &self.log)
    }

    /// In-process replay check (the `session_verify` tool's
    /// logic): replayed hash, live hash, equality.
    pub fn verify(&self) -> R<(String, String, bool)> {
        let replayed = self.replay_check()?;
        let live = self.state_hash();
        Ok((replayed.clone(), live.clone(), replayed == live))
    }
}

/// Session-local validation for the op kinds no authoring
/// verb owns, per the design's AUTH mapping: duplicate
/// selection entries conflict; naming a missing node is an
/// unknown target.
fn validate_session_local(st: &EditorState, op: &Op) -> R<()> {
    if let Op::SelectionSet { nodes } = op {
        let mut seen = std::collections::BTreeSet::new();
        for id in nodes {
            if !seen.insert(id) {
                return Err(Refusal::new(
                    "TG-AUTH-CONFLICT",
                    format!("selection lists node {id} twice"),
                ));
            }
            if st.node(*id).is_none() {
                return Err(Refusal::new(
                    "TG-AUTH-UNKNOWN-TARGET",
                    format!("selection names missing node {id}"),
                ));
            }
        }
    }
    Ok(())
}

/// Replays `log` into `replay`, verifying every chain link,
/// and returns the final hash.
fn replay_log(replay: &mut Session, log: &[LogEntry]) -> R<String> {
    let mut cur = replay.state_hash();
    if !log.is_empty() && cur != log[0].pre {
        return Err(Refusal::new(
            "TG-SES-LOG-CHAIN",
            "first log entry's pre does not match the genesis hash",
        ));
    }
    for (i, e) in log.iter().enumerate() {
        if e.seq != i as u64 + 1 || e.session != replay.id {
            return Err(Refusal::new(
                "TG-SES-LOG-CHAIN",
                format!("log entry {} breaks sequence/session", e.seq),
            ));
        }
        if e.pre != cur {
            return Err(Refusal::new(
                "TG-SES-LOG-CHAIN",
                format!("log entry {} pre-hash does not chain", e.seq),
            ));
        }
        let op = Op::from_json(&e.op)?;
        replay.apply_op(&op)?;
        cur = replay.state_hash();
        if e.post != cur {
            return Err(Refusal::new(
                "TG-SES-LOG-CHAIN",
                format!("log entry {} post-hash does not match replay", e.seq),
            ));
        }
    }
    Ok(cur)
}

/// Replay a log against a genesis state, verifying every
/// chain link. Shared by compile's record path and the
/// fresh-process replay; the gate mirror is rebuilt from
/// the genesis (see `Session::from_genesis_replay`).
pub fn replay_ops(genesis: &EditorState, log: &[LogEntry], session: &str) -> R<String> {
    let mut replay = Session::from_genesis_replay(genesis.clone());
    replay.id = session.to_string();
    replay_log(&mut replay, log)
}

/// Apply an op decide() has allowed. Never call on a refused op.
pub fn apply(st: &mut EditorState, op: &Op) {
    match op {
        Op::ParamSet { param, value } => {
            if let Some(p) = st.params.get_mut(param) {
                p.value = *value;
            }
        }
        Op::NodeSetVisible { node, visible } => {
            if let Some(n) = st.node_mut(*node) {
                n.visible = *visible;
            }
        }
        Op::NodeSetTransform {
            node,
            tx,
            ty,
            rot,
            sx,
            sy,
        } => {
            if let Some(n) = st.node_mut(*node) {
                n.transform = Transform {
                    tx: *tx,
                    ty: *ty,
                    rot: *rot,
                    sx: *sx,
                    sy: *sy,
                };
            }
        }
        Op::SelectionSet { nodes } => {
            let mut s = nodes.clone();
            s.sort_unstable();
            st.selection = s;
        }
        Op::ViewportSet { cam_x, cam_y, zoom } => {
            st.viewport = Viewport {
                cam_x: *cam_x,
                cam_y: *cam_y,
                zoom: *zoom,
            };
        }
    }
}

#[derive(Debug)]
pub struct CompileOutcome {
    pub genesis_hash: String,
    pub final_hash: String,
    pub ops_applied: usize,
}

/// The compile step: live session -> canonical record on disk.
/// Emits `base.ron` (genesis), `ops.jsonl` (the replayable sequence),
/// `project.ron` (final state), `session.json` (identity + hashes).
/// Refuses — writing nothing — while uncapturable state is declared, and
/// refuses on any log-chain break.
pub fn compile_session(session: &Session, out_dir: &std::path::Path) -> R<CompileOutcome> {
    if !session.state.uncapturable.is_empty() {
        return Err(Refusal::new(
            "TG-SES-UNCAPTURABLE",
            format!(
                "state declares uncapturable elements: {}",
                session.state.uncapturable.join(", ")
            ),
        ));
    }
    let replayed = session.replay_check()?;
    let live = session.state_hash();
    if replayed != live {
        return Err(Refusal::new(
            "TG-SES-LOG-CHAIN",
            format!("replay hash {replayed} != live hash {live}"),
        ));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| Refusal::new("TG-SES-IO", format!("create {}: {e}", out_dir.display())))?;
    let write = |name: &str, text: &str| -> R<()> {
        std::fs::write(out_dir.join(name), text)
            .map_err(|e| Refusal::new("TG-SES-IO", format!("write {name}: {e}")))
    };
    write("base.ron", &to_canon_ron(&session.genesis))?;
    write("project.ron", &to_canon_ron(&session.state))?;
    let mut log_text = String::new();
    for e in &session.log {
        log_text.push_str(&serde_json::to_string(e).expect("log entry serialization is total"));
        log_text.push('\n');
    }
    write("ops.jsonl", &log_text)?;
    let session_json = serde_json::json!({
        "format": "torogao-editor-session/1",
        "session": session.id,
        "genesis_hash": session.genesis_hash(),
        "final_hash": live,
        "ops": session.log.len(),
        "held_by": session.held_by.as_str(),
    });
    write(
        "session.json",
        &format!("{}\n", serde_json::to_string_pretty(&session_json).unwrap()),
    )?;
    Ok(CompileOutcome {
        genesis_hash: session.genesis_hash(),
        final_hash: live,
        ops_applied: session.log.len(),
    })
}

/// Fresh-load replay: read ONLY an emitted record directory, rebuild the
/// state through the RON parser + op replay, and return the final hash.
/// This is the round-trip gate's core; it never sees session memory.
pub fn replay_record(record_dir: &std::path::Path) -> R<String> {
    let read = |name: &str| -> R<String> {
        std::fs::read_to_string(record_dir.join(name))
            .map_err(|e| Refusal::new("TG-SES-IO", format!("read {name}: {e}")))
    };
    let genesis = from_canon_ron(&read("base.ron")?)?;
    let session_meta: serde_json::Value = serde_json::from_str(&read("session.json")?)
        .map_err(|e| Refusal::new("TG-SES-SCHEMA", format!("session.json: {e}")))?;
    let session_id = session_meta
        .get("session")
        .and_then(|s| s.as_str())
        .ok_or_else(|| Refusal::new("TG-SES-SCHEMA", "session.json has no session id"))?
        .to_string();
    let mut log = Vec::new();
    for line in read("ops.jsonl")?.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let e: LogEntry = serde_json::from_str(line)
            .map_err(|e| Refusal::new("TG-SES-SCHEMA", format!("ops.jsonl: {e}")))?;
        log.push(e);
    }
    let final_hash = replay_ops(&genesis, &log, &session_id)?;
    // The emitted final record must hash to the same value independently.
    let project = from_canon_ron(&read("project.ron")?)?;
    let project_hash = state_hash(&project);
    if project_hash != final_hash {
        return Err(Refusal::new(
            "TG-SES-LOG-CHAIN",
            format!("project.ron hash {project_hash} != replay hash {final_hash}"),
        ));
    }
    Ok(final_hash)
}
