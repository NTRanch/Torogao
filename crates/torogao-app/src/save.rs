//! The save path (§5.4 of the parity inventory, ruled by
//! D-081): a save document is the session's own evidence
//! plus the play state. The session half is the genesis
//! state in canonical RON and the admitted op log — every
//! op in it passed the AUTH-D2 `decide()` gate when it was
//! committed, and restoring re-runs each op through the
//! same gate (`Session::apply_op`), so a save never
//! smuggles an unadmitted write into a session. The play
//! half is the transport record (animation, playhead,
//! step count), restored through the play host's own
//! operations. Both state hashes are recorded at capture
//! and proven equal at restore; a mismatch is a typed
//! refusal, never a silent divergence.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use torogao_session::canon::{from_canon_ron, to_canon_ron};
use torogao_session::op::Op;
use torogao_session::session::Session;

use crate::error::{AppError, AppResult};
use crate::player::{PlayHost, PlayState};
use crate::scene::Scene;

/// The save document format tag.
pub const SAVE_FORMAT: &str = "torogao-session-save/1";
/// Largest save document read from disk, in bytes.
pub const SAVE_BYTES_MAX: usize = 16 * 1024 * 1024;
/// Largest op log a save may carry.
pub const SAVE_OPS_MAX: usize = 100_000;
/// Largest play step count a restore will replay.
pub const RESTORE_STEPS_MAX: u64 = 3_600_000;

/// One recorded play-surface operation, in the order the
/// host performed it. A save whose play record carries a
/// trajectory restores by replaying it exactly — the
/// evaluated state is a function of the whole step
/// history, so the trajectory, not the snapshot alone,
/// is what makes the play half deterministic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PlayOp {
    /// `play_control apply_state`.
    ApplyState {
        /// The named state applied.
        state: String,
    },
    /// `play_control play`.
    Play {
        /// The animation named, when one was.
        animation: Option<String>,
        /// Whether playback loops.
        looping: bool,
        /// The speed multiplier, when one was given.
        speed: Option<f32>,
    },
    /// `play_control pause`.
    Pause,
    /// `play_control stop`.
    Stop {
        /// Whether the stop was immediate.
        immediate: bool,
    },
    /// `play_control seek`.
    Seek {
        /// The playhead frame sought.
        frame: i32,
    },
    /// `play_control step`.
    Step {
        /// Fixed steps advanced.
        steps: u64,
    },
}

/// The transport half of a save document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayRecord {
    /// The loaded animation's name, when one is loaded.
    pub animation: Option<String>,
    /// The playhead frame (rounded).
    pub frame: i32,
    /// The playhead floating frame.
    pub hframe: f32,
    /// Whether the transport was playing.
    pub playing: bool,
    /// Whether the transport was paused.
    pub paused: bool,
    /// Whether the playback loops.
    pub looping: bool,
    /// Completed loops.
    pub looped: i32,
    /// The speed multiplier.
    pub speed: f32,
    /// The last named state applied through the player.
    pub applied_state: Option<String>,
    /// Fixed steps the play host had advanced since the
    /// scene was installed.
    pub step_count: u64,
    /// The recorded play trajectory, when the capture
    /// surface recorded one (the session bridge does; a
    /// windowed transport capture does not, and its
    /// restore falls back to the step-count alignment in
    /// `restore_play`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trajectory: Option<Vec<PlayOp>>,
}

impl PlayRecord {
    /// Captures the record from a play-host snapshot.
    pub fn capture(state: &PlayState) -> PlayRecord {
        PlayRecord {
            animation: state.animation.clone(),
            frame: state.frame,
            hframe: state.hframe,
            playing: state.playing,
            paused: state.paused,
            looping: state.looping,
            looped: state.looped,
            speed: state.speed,
            applied_state: state.applied_state.clone(),
            step_count: state.step_count,
            trajectory: None,
        }
    }

    /// Whether a restored snapshot lands on this record's
    /// deterministic fields (animation, frame, loop, speed,
    /// applied state). Step count is compared separately:
    /// a corrective seek may legitimately move it by one.
    pub fn matches(&self, state: &PlayState) -> bool {
        self.animation == state.animation
            && self.frame == state.frame
            && self.looping == state.looping
            && self.speed == state.speed
            && self.applied_state == state.applied_state
    }
}

/// One save document (`torogao-session-save/1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveDocument {
    /// The format tag; must equal `SAVE_FORMAT`.
    pub format: String,
    /// The model the scene was loaded from (display and
    /// reload path).
    pub model: String,
    /// The session id (derived from the genesis hash).
    pub session_id: String,
    /// Who held the session at capture.
    pub held_by: String,
    /// The genesis editor state, canonical RON.
    pub genesis: String,
    /// The rig-declared parameter defaults the session's
    /// gate mirror was built from.
    pub defaults: BTreeMap<String, f32>,
    /// The admitted op log, as session op JSON, in order.
    pub ops: Vec<serde_json::Value>,
    /// The transport record.
    pub play: PlayRecord,
    /// The session state hash at capture.
    pub session_state_hash: String,
    /// The app's evaluated state hash at capture.
    pub evaluated_state_hash: String,
}

/// Captures a save document from a live session and play
/// snapshot. `evaluated_state_hash` is the embedding
/// host's current evaluated hash (the session bridge's
/// `evaluated_hash`).
pub fn capture(
    session: &Session,
    play: &PlayState,
    model: &str,
    evaluated_state_hash: &str,
) -> SaveDocument {
    SaveDocument {
        format: SAVE_FORMAT.to_string(),
        model: model.to_string(),
        session_id: session.id.clone(),
        held_by: session.held_by.clone(),
        genesis: to_canon_ron(&session.genesis),
        defaults: session_defaults(session),
        ops: session.log.iter().map(|entry| entry.op.clone()).collect(),
        play: PlayRecord::capture(play),
        session_state_hash: session.state_hash(),
        evaluated_state_hash: evaluated_state_hash.to_string(),
    }
}

/// The session's parameter defaults. The session does not
/// expose its mirror's copy, so the app keeps the defaults
/// it built the session with; `capture_with_defaults` is
/// the precise entry point and `capture` falls back to the
/// genesis state's own parameter values.
fn session_defaults(session: &Session) -> BTreeMap<String, f32> {
    let mut defaults = BTreeMap::new();
    for (name, param) in &session.genesis.params {
        defaults.insert(name.clone(), param.value);
    }
    defaults
}

/// Captures a save document with the exact parameter
/// defaults the session was opened with (the shell keeps
/// them from `project_genesis`).
pub fn capture_with_defaults(
    session: &Session,
    play: &PlayState,
    model: &str,
    evaluated_state_hash: &str,
    defaults: &BTreeMap<String, f32>,
) -> SaveDocument {
    let mut document = capture(session, play, model, evaluated_state_hash);
    document.defaults = defaults.clone();
    document
}

/// Serializes a save document. Field order is the
/// struct's; map keys sort (BTreeMap); the op log keeps
/// its order — re-serializing a parsed document yields
/// the identical bytes, which the round-trip gate asserts.
pub fn serialize(document: &SaveDocument) -> AppResult<String> {
    serde_json::to_string_pretty(document)
        .map_err(|err| AppError::Format(format!("save document does not serialize: {err}")))
}

/// Parses and validates a save document: format tag, op
/// count bound, and every op through the session op
/// parser, so a malformed document refuses here rather
/// than mid-restore.
pub fn parse(text: &str) -> AppResult<SaveDocument> {
    let document: SaveDocument = serde_json::from_str(text)
        .map_err(|err| AppError::Format(format!("save document does not parse: {err}")))?;
    if document.format != SAVE_FORMAT {
        return Err(AppError::Format(format!(
            "save document format is {:?}; this build reads {SAVE_FORMAT:?}",
            document.format
        )));
    }
    if document.ops.len() > SAVE_OPS_MAX {
        return Err(AppError::Format(format!(
            "save carries {} ops; the bound is {SAVE_OPS_MAX}",
            document.ops.len()
        )));
    }
    if document.play.step_count > RESTORE_STEPS_MAX {
        return Err(AppError::Format(format!(
            "save play step count {} exceeds the restore bound {RESTORE_STEPS_MAX}",
            document.play.step_count
        )));
    }
    for value in &document.ops {
        Op::from_json(value).map_err(|refusal| {
            AppError::Format(format!(
                "save op refuses to parse: {}: {}",
                refusal.code, refusal.message
            ))
        })?;
    }
    Ok(document)
}

/// Writes a save document under the output-guard rules
/// the CLI family already enforces: the output may not be
/// the model file itself (refused unconditionally), and an
/// existing output needs `force`. The write is atomic
/// (temporary file in the same directory, then rename).
pub fn write_save(
    path: &Path,
    document: &SaveDocument,
    model_path: Option<&Path>,
    force: bool,
) -> AppResult<()> {
    if let Some(model) = model_path {
        if same_file(path, model) {
            return Err(AppError::Param(
                "save output is the model file itself; refused".to_string(),
            ));
        }
    }
    if path.exists() && !force {
        return Err(AppError::Param(format!(
            "save output {} already exists; pass force to overwrite",
            path.display()
        )));
    }
    let text = serialize(document)?;
    let temp = path.with_extension("tss-partial");
    std::fs::write(&temp, text).map_err(|err| AppError::Io {
        context: "write save",
        message: err.to_string(),
    })?;
    std::fs::rename(&temp, path).map_err(|err| AppError::Io {
        context: "commit save",
        message: err.to_string(),
    })?;
    Ok(())
}

/// Reads and validates a save document from disk, with
/// the byte bound enforced before the full read lands.
pub fn read_save(path: &Path) -> AppResult<SaveDocument> {
    let file = std::fs::File::open(path).map_err(|err| AppError::Io {
        context: "open save",
        message: err.to_string(),
    })?;
    let mut text = String::new();
    file.take((SAVE_BYTES_MAX + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|err| AppError::Io {
            context: "read save",
            message: err.to_string(),
        })?;
    if text.len() > SAVE_BYTES_MAX {
        return Err(AppError::Format(format!(
            "save document exceeds the {SAVE_BYTES_MAX}-byte bound"
        )));
    }
    parse(&text)
}

/// Whether two paths name the same file (lexical absolute
/// comparison — neither path need exist).
fn same_file(a: &Path, b: &Path) -> bool {
    lexical_absolute(a) == lexical_absolute(b)
}

fn lexical_absolute(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Restores the session half of a save: genesis through
/// the canonical RON reader, then every op re-admitted
/// through the gate via `Session::apply_op`. The restored
/// session's id and state hash must equal the document's;
/// any divergence is a typed refusal.
pub fn restore_session(document: &SaveDocument) -> AppResult<Session> {
    let genesis = from_canon_ron(&document.genesis).map_err(|refusal| {
        AppError::Format(format!(
            "save genesis refuses to load: {}: {}",
            refusal.code, refusal.message
        ))
    })?;
    let mut session = Session::new(genesis, &document.defaults, &document.held_by);
    if session.id != document.session_id {
        return Err(AppError::Format(format!(
            "save session id {} does not match its genesis (restored {})",
            document.session_id, session.id
        )));
    }
    for value in &document.ops {
        let op = Op::from_json(value).map_err(|refusal| {
            AppError::Format(format!(
                "save op refuses to parse: {}: {}",
                refusal.code, refusal.message
            ))
        })?;
        session.apply_op(&op).map_err(|refusal| {
            AppError::Format(format!(
                "save op refused at restore: {}: {}",
                refusal.code, refusal.message
            ))
        })?;
    }
    let restored_hash = session.state_hash();
    if restored_hash != document.session_state_hash {
        return Err(AppError::Format(format!(
            "restored session hash {restored_hash} != saved hash {}",
            document.session_state_hash
        )));
    }
    Ok(session)
}

/// What a play restore did, for the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestorePlayReport {
    /// Host steps taken by the restore.
    pub steps_taken: u64,
    /// Whether the restore replayed a recorded trajectory
    /// (the exact path) rather than the snapshot-alignment
    /// fallback.
    pub trajectory_replayed: bool,
}

/// Replays one recorded play operation through the play
/// host's own methods.
fn replay_op(op: &PlayOp, scene: &mut Scene, host: &mut PlayHost) -> AppResult<()> {
    match op {
        PlayOp::ApplyState { state } => host.apply_state(scene, state),
        PlayOp::Play {
            animation,
            looping,
            speed,
        } => host.play(scene, animation.as_deref(), *looping, *speed),
        PlayOp::Pause => {
            host.pause();
            Ok(())
        }
        PlayOp::Stop { immediate } => {
            host.stop(scene, *immediate);
            Ok(())
        }
        PlayOp::Seek { frame } => host.seek(scene, *frame),
        PlayOp::Step { steps } => host.step(scene, *steps),
    }
}

/// Restores the play half of a save into a fresh play
/// host over `scene`. When the record carries a
/// trajectory, the restore replays it operation for
/// operation — the same sequence the live host performed
/// — and the resulting snapshot and step count must
/// equal the record's exactly. Without a trajectory
/// (windowed captures), it falls back to alignment: the
/// applied state, the animation, fixed steps up to the
/// recorded step count, and a corrective seek when the
/// playhead does not land on the recorded frame.
pub fn restore_play(
    document: &SaveDocument,
    scene: &mut Scene,
    host: &mut PlayHost,
) -> AppResult<RestorePlayReport> {
    let record = &document.play;
    if let Some(trajectory) = &record.trajectory {
        let mut report = RestorePlayReport {
            steps_taken: 0,
            trajectory_replayed: true,
        };
        for op in trajectory {
            if let PlayOp::Step { steps } = op {
                report.steps_taken += steps;
            }
            replay_op(op, scene, host)?;
        }
        let snapshot = host.snapshot(scene);
        if !record.matches(&snapshot) || snapshot.step_count != record.step_count {
            return Err(AppError::Format(format!(
                "trajectory restore landed on (animation {:?}, frame {}, steps {}) instead of the save's (animation {:?}, frame {}, steps {})",
                snapshot.animation,
                snapshot.frame,
                snapshot.step_count,
                record.animation,
                record.frame,
                record.step_count
            )));
        }
        return Ok(report);
    }
    if let Some(state) = &record.applied_state {
        host.apply_state(scene, state)?;
    }
    if let Some(animation) = &record.animation {
        host.play(scene, Some(animation), record.looping, Some(record.speed))?;
        if record.paused {
            host.pause();
        }
    }
    let mut report = RestorePlayReport {
        steps_taken: 0,
        trajectory_replayed: false,
    };
    let current = host.step_count();
    if record.step_count > current {
        let steps = record.step_count - current;
        host.step(scene, steps)?;
        report.steps_taken = steps;
    } else if record.step_count < current {
        return Err(AppError::Format(format!(
            "save play step count {} is behind the host's {current}; the restore target is unreachable",
            record.step_count
        )));
    }
    let snapshot = host.snapshot(scene);
    if snapshot.frame != record.frame && record.animation.is_some() {
        // A seek preceded the save: land the recorded
        // frame, then evaluate it with one step (the same
        // seek-then-step shape the live surface produces).
        host.seek(scene, record.frame)?;
        host.step(scene, 1)?;
        report.steps_taken += 1;
    }
    let snapshot = host.snapshot(scene);
    if !record.matches(&snapshot) {
        return Err(AppError::Format(format!(
            "restored play state (animation {:?}, frame {}, speed {}) does not match the save (animation {:?}, frame {}, speed {})",
            snapshot.animation,
            snapshot.frame,
            snapshot.speed,
            record.animation,
            record.frame,
            record.speed
        )));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::{SaveDocument, parse, serialize};
    use crate::player::PlayState;
    use std::collections::BTreeMap;
    use torogao_session::session::Session;
    use torogao_session::state::EditorState;

    fn document() -> SaveDocument {
        let genesis = EditorState {
            format: torogao_session::state::STATE_FORMAT.to_string(),
            project: torogao_session::state::ProjectMeta {
                name: "save-test".to_string(),
                source: "test".to_string(),
            },
            scene: torogao_session::state::Scene {
                name: "save-test".to_string(),
                nodes: Vec::new(),
            },
            selection: Vec::new(),
            viewport: torogao_session::state::Viewport {
                cam_x: 0.0,
                cam_y: 0.0,
                zoom: 1.0,
            },
            params: BTreeMap::new(),
            bindings: Vec::new(),
            uncapturable: Vec::new(),
        };
        let defaults = BTreeMap::new();
        let session = Session::new(genesis, &defaults, "save tests");
        super::capture(&session, &PlayState::default(), "model.inx", "0123abcd")
    }

    #[test]
    fn save_document_round_trips_byte_identical() {
        let doc = document();
        let text = serialize(&doc).expect("serialize");
        let parsed = parse(&text).expect("parse");
        let again = serialize(&parsed).expect("re-serialize");
        assert_eq!(text, again);
    }

    #[test]
    fn parse_refuses_a_foreign_format() {
        let mut doc = document();
        doc.format = "torogao-session-save/0".to_string();
        let text = serialize(&doc).expect("serialize");
        assert!(parse(&text).is_err());
    }

    #[test]
    fn restored_session_matches_the_saved_hash() {
        let doc = document();
        let restored = super::restore_session(&doc).expect("restore");
        assert_eq!(restored.state_hash(), doc.session_state_hash);
        assert_eq!(restored.id, doc.session_id);
    }
}
