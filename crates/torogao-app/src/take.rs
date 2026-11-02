//! Takes (Stage 3.1, PROGRAM §5.1): a take is the session
//! host's native record of one performance — the session
//! genesis plus the admitted op log that shaped it (the
//! save path's session half) and the recorded play
//! trajectory (the performance itself: curve data enters
//! through the session ops, motion through the trajectory).
//!
//! Replay is not a video trick — it re-evaluates. The
//! session half is restored through `save::restore_session`,
//! which re-admits every op through the session `decide()`
//! gate and proves the state hash; the admitted ops are
//! then applied to the live scene through the session
//! bridge's own `apply_live` (the same code the live host
//! serves), and the trajectory is driven through the play
//! host one fixed step at a time. A frame of the take is
//! the evaluated state after one advancing step (`Step`,
//! or an `ApplyState`, which advances exactly one step);
//! the per-frame evaluated-state hashes fold into the
//! take's frame-sequence hash, so replay determinism is a
//! hash equality, not an eyeball.
//!
//! Export renders that same replay and encodes it under a
//! named preset (1920×1080 @ 60 fps, h264) with a
//! provenance block beside it: take hash, session replay
//! hash, and the rig + motion document hash (the model
//! file's SHA-256, verified against the bytes actually
//! loaded — an export against a different model refuses).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use torogao_format::jsonfmt::to_d_json;
use torogao_session::host::LiveBridge;
use torogao_session::op::Op;
use torogao_session::session::Session;

use crate::error::{AppError, AppResult};
use crate::player::{PlayHost, PlayState};
use crate::render::Renderer;
use crate::save::{PlayOp, SAVE_FORMAT, SaveDocument};
use crate::scene::{STEP_SECONDS, Scene};
use crate::session_host::AppBridge;
use crate::state::ShellState;
use crate::state_hash::evaluated_state_hash;
use crate::textures::encode_png;
use crate::video_bridge::{ffmpeg_has_libx264, run_ffmpeg, sha256_hex};

/// The take document format tag.
pub const TAKE_FORMAT: &str = "torogao-take/1";
/// The provenance block format tag.
pub const PROVENANCE_FORMAT: &str = "torogao-take-provenance/1";
/// Largest take document read from disk, in bytes (the
/// save path's bound; a take embeds a save document).
pub const TAKE_BYTES_MAX: usize = crate::save::SAVE_BYTES_MAX;
/// Largest frame count one take may carry, in fixed steps
/// (the video bridge's frame bound: 600 s at 60 steps/s).
pub const TAKE_FRAMES_MAX: u64 = crate::video_bridge::VIDEO_FRAMES_MAX as u64;
/// The performance clip's file name inside an export dir.
pub const TAKE_CLIP: &str = "performance.mp4";
/// The final concatenated clip's file name.
pub const TAKE_FINAL_CLIP: &str = "final.mp4";
/// The take document's file name inside an export dir.
pub const TAKE_FILE: &str = "take.json";
/// The provenance block's file name inside an export dir.
pub const PROVENANCE_FILE: &str = "provenance.json";
/// The timeline document's file name inside an export dir.
pub const TAKE_TIMELINE_FILE: &str = "timeline.json";

/// One export preset: an output geometry, rate, and codec
/// class. The take's step rate is fixed at 60 steps/s, so
/// a preset's rate must equal it — a mismatch refuses
/// rather than resampling a deterministic record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportPreset {
    /// The preset's name in reports and provenance.
    pub name: &'static str,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Output frame rate (frames per second).
    pub fps: u32,
}

/// The Stage 3.1 export preset: 1920×1080 @ 60 fps, h264.
pub const PRESET_HD_1080P60: ExportPreset = ExportPreset {
    name: "hd-1080p60",
    width: 1920,
    height: 1080,
    fps: 60,
};

/// One take document (`torogao-take/1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeDocument {
    /// The format tag; must equal `TAKE_FORMAT`.
    pub format: String,
    /// The model the performance was captured on (display
    /// and reload label, from the save document).
    pub model: String,
    /// SHA-256 of the model file's bytes: the rig + motion
    /// document hash the export verifies and the provenance
    /// block cites.
    pub model_sha256: String,
    /// The take's authored length, in advancing steps: the
    /// trajectory's `Step` counts plus one per `ApplyState`.
    /// Must equal the play record's step count.
    pub frames: u64,
    /// The session half plus the play record (with its
    /// trajectory): a full save document.
    pub save: SaveDocument,
    /// SHA-256 of this document's canonical serialization
    /// with this field emptied (see `take_hash`).
    pub take_sha256: String,
}

/// Counts a trajectory's advancing steps: `Step` advances
/// its count, `ApplyState` advances exactly one (the play
/// host's own accounting), the transport ops advance none.
pub fn trajectory_frames(trajectory: &[PlayOp]) -> AppResult<u64> {
    let mut frames: u64 = 0;
    for op in trajectory {
        let advance = match op {
            PlayOp::Step { steps } => *steps,
            PlayOp::ApplyState { .. } => 1,
            PlayOp::Pause | PlayOp::Play { .. } | PlayOp::Seek { .. } | PlayOp::Stop { .. } => 0,
        };
        frames = frames
            .checked_add(advance)
            .ok_or_else(|| AppError::Format("trajectory step count overflows u64".to_string()))?;
    }
    Ok(frames)
}

/// The take hash: SHA-256 of the canonical serialization
/// with the hash field emptied. The save layer's
/// round-trip discipline (re-serializing a parsed document
/// yields identical bytes) is what makes this stable.
pub fn take_hash(document: &TakeDocument) -> AppResult<String> {
    let mut unsigned = document.clone();
    unsigned.take_sha256 = String::new();
    let text = serialize(&unsigned)?;
    Ok(sha256_hex(text.as_bytes()))
}

/// Captures a take from a live session and play snapshot.
/// `trajectory` is the bridge's recorded play log for the
/// same performance; the play record's step count must
/// equal the trajectory's advancing steps — a snapshot and
/// a trajectory that disagree are refused, never reconciled
/// silently. `model_sha256` is the SHA-256 of the model
/// file's bytes (`sha256_hex`).
pub fn capture(
    session: &Session,
    play: &PlayState,
    trajectory: Vec<PlayOp>,
    model: &str,
    model_sha256: &str,
    evaluated_state_hash: &str,
    defaults: &BTreeMap<String, f32>,
) -> AppResult<TakeDocument> {
    if trajectory.is_empty() {
        return Err(AppError::Format(
            "a take requires a recorded play trajectory".to_string(),
        ));
    }
    let frames = trajectory_frames(&trajectory)?;
    if frames == 0 || frames > TAKE_FRAMES_MAX {
        return Err(AppError::Format(format!(
            "take length {frames} steps is outside 1..={TAKE_FRAMES_MAX}"
        )));
    }
    if frames != play.step_count {
        return Err(AppError::Format(format!(
            "trajectory advances {frames} steps but the play record counts {}; \
             the snapshot and the trajectory are not one performance",
            play.step_count
        )));
    }
    let mut save =
        crate::save::capture_with_defaults(session, play, model, evaluated_state_hash, defaults);
    save.play.trajectory = Some(trajectory);
    let mut document = TakeDocument {
        format: TAKE_FORMAT.to_string(),
        model: model.to_string(),
        model_sha256: model_sha256.to_string(),
        frames,
        save,
        take_sha256: String::new(),
    };
    document.take_sha256 = take_hash(&document)?;
    Ok(document)
}

/// Validates a take document's invariants: format tags,
/// the embedded save's bounds and op parseability, a
/// present trajectory whose advancing steps equal both the
/// declared frame count and the play record's step count,
/// and the take hash. Capture and parse both run it.
pub fn validate(document: &TakeDocument) -> AppResult<()> {
    if document.format != TAKE_FORMAT {
        return Err(AppError::Format(format!(
            "take document format is {:?}; this build reads {TAKE_FORMAT:?}",
            document.format
        )));
    }
    if document.save.format != SAVE_FORMAT {
        return Err(AppError::Format(format!(
            "take's save document format is {:?}; expected {SAVE_FORMAT:?}",
            document.save.format
        )));
    }
    if document.frames == 0 || document.frames > TAKE_FRAMES_MAX {
        return Err(AppError::Format(format!(
            "take length {} steps is outside 1..={TAKE_FRAMES_MAX}",
            document.frames
        )));
    }
    let Some(trajectory) = &document.save.play.trajectory else {
        return Err(AppError::Format(
            "take carries no play trajectory; a snapshot alone is not a take".to_string(),
        ));
    };
    let frames = trajectory_frames(trajectory)?;
    if frames != document.frames {
        return Err(AppError::Format(format!(
            "take declares {} frames but its trajectory advances {frames}",
            document.frames
        )));
    }
    if document.save.play.step_count != document.frames {
        return Err(AppError::Format(format!(
            "take declares {} frames but its play record counts {} steps",
            document.frames, document.save.play.step_count
        )));
    }
    if document.save.ops.len() > crate::save::SAVE_OPS_MAX {
        return Err(AppError::Format(format!(
            "take's save carries {} ops; the bound is {}",
            document.save.ops.len(),
            crate::save::SAVE_OPS_MAX
        )));
    }
    for value in &document.save.ops {
        Op::from_json(value).map_err(|refusal| {
            AppError::Format(format!(
                "take op refuses to parse: {}: {}",
                refusal.code, refusal.message
            ))
        })?;
    }
    let computed = take_hash(document)?;
    if computed != document.take_sha256 {
        return Err(AppError::Format(format!(
            "take hash mismatch: document claims {}, its bytes hash to {computed}",
            document.take_sha256
        )));
    }
    Ok(())
}

/// Serializes a take document (the save layer's pretty
/// form; re-serializing a parsed document yields the
/// identical bytes).
pub fn serialize(document: &TakeDocument) -> AppResult<String> {
    serde_json::to_string_pretty(document)
        .map_err(|err| AppError::Format(format!("take document does not serialize: {err}")))
}

/// Parses and validates a take document.
pub fn parse(text: &str) -> AppResult<TakeDocument> {
    let document: TakeDocument = serde_json::from_str(text)
        .map_err(|err| AppError::Format(format!("take document does not parse: {err}")))?;
    validate(&document)?;
    Ok(document)
}

/// Writes a take document under the save path's output
/// guard rules: the output may not be the model file
/// itself, an existing output needs `force`, and the write
/// is atomic (temporary file in the same directory, then
/// rename).
pub fn write_take(
    path: &Path,
    document: &TakeDocument,
    model_path: Option<&Path>,
    force: bool,
) -> AppResult<()> {
    if let Some(model) = model_path {
        if same_file(path, model) {
            return Err(AppError::Param(
                "take output is the model file itself; refused".to_string(),
            ));
        }
    }
    if path.exists() && !force {
        return Err(AppError::Param(format!(
            "take output {} already exists; pass force to overwrite",
            path.display()
        )));
    }
    let text = serialize(document)?;
    let temp = path.with_extension("take-partial");
    std::fs::write(&temp, text).map_err(|err| AppError::Io {
        context: "write take",
        message: err.to_string(),
    })?;
    std::fs::rename(&temp, path).map_err(|err| AppError::Io {
        context: "commit take",
        message: err.to_string(),
    })?;
    Ok(())
}

/// Reads and validates a take document from disk, with the
/// byte bound enforced before the full read lands.
pub fn read_take(path: &Path) -> AppResult<TakeDocument> {
    let file = std::fs::File::open(path).map_err(|err| AppError::Io {
        context: "open take",
        message: err.to_string(),
    })?;
    let mut text = String::new();
    file.take((TAKE_BYTES_MAX + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|err| AppError::Io {
            context: "read take",
            message: err.to_string(),
        })?;
    if text.len() > TAKE_BYTES_MAX {
        return Err(AppError::Format(format!(
            "take document exceeds the {TAKE_BYTES_MAX}-byte bound"
        )));
    }
    parse(&text)
}

/// Whether two paths name the same file (lexical absolute
/// comparison — neither path need exist). Mirrors the save
/// path's helper.
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

/// What one replay proved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayReport {
    /// Frames replayed (advancing steps driven).
    pub frames: u64,
    /// SHA-256 over the per-frame evaluated-state hashes
    /// (little-endian u64s, in frame order).
    pub frame_sequence_sha256: String,
    /// The restored session's state hash (proven equal to
    /// the take's recorded hash by the restore itself).
    pub session_state_hash: String,
}

/// Replays a take into `scene` (fresh-loaded and
/// fresh-evaluated by the caller, exactly as at capture):
/// restores the session through the gate, applies the
/// admitted ops to the live scene through the bridge, then
/// drives the trajectory through a fresh play host one
/// fixed step at a time. `visit` is called with the scene
/// and the frame index after every advancing step — the
/// hash replays fold the evaluated state, the export
/// renders it; both are the same replay. The closing
/// snapshot must equal the take's play record exactly, or
/// the replay refuses. Returns the report and the scene.
pub fn replay(
    take: &TakeDocument,
    scene: Scene,
    visit: &mut dyn FnMut(&mut Scene, u64) -> AppResult<()>,
) -> AppResult<(ReplayReport, Scene)> {
    validate(take)?;
    let session = crate::save::restore_session(&take.save)?;
    let mut shell = ShellState::new();
    shell.install_scene(scene);
    let mut bridge = AppBridge::with_player(shell, PlayHost::new());
    for value in &take.save.ops {
        let op = Op::from_json(value).map_err(|refusal| {
            AppError::Format(format!(
                "take op refuses to parse: {}: {}",
                refusal.code, refusal.message
            ))
        })?;
        bridge.apply_live(&op).map_err(|refusal| {
            AppError::Format(format!(
                "take op refused at replay: {}: {}",
                refusal.code, refusal.message
            ))
        })?;
    }
    let (shell, mut player) = bridge.into_parts();
    let mut scene = shell
        .scene
        .ok_or_else(|| AppError::Format("the replay shell holds no scene".to_string()))?;
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    let mut frames: u64 = 0;
    let mut emit =
        |scene: &mut Scene, hasher: &mut sha2::Sha256, frames: &mut u64| -> AppResult<()> {
            let hash = evaluated_state_hash(&mut scene.puppet);
            hasher.update(hash.to_le_bytes());
            visit(scene, *frames)?;
            *frames += 1;
            Ok(())
        };
    let trajectory = take
        .save
        .play
        .trajectory
        .as_ref()
        .expect("validate proved the trajectory present");
    for op in trajectory {
        match op {
            PlayOp::ApplyState { state } => {
                player.apply_state(&mut scene, state)?;
                emit(&mut scene, &mut hasher, &mut frames)?;
            }
            PlayOp::Pause => player.pause(),
            PlayOp::Play {
                animation,
                looping,
                speed,
            } => player.play(&scene, animation.as_deref(), *looping, *speed)?,
            PlayOp::Seek { frame } => player.seek(&scene, *frame)?,
            PlayOp::Step { steps } => {
                for _ in 0..*steps {
                    player.step(&mut scene, 1)?;
                    emit(&mut scene, &mut hasher, &mut frames)?;
                }
            }
            PlayOp::Stop { immediate } => player.stop(&scene, *immediate),
        }
    }
    let record = &take.save.play;
    let snapshot = player.snapshot(&scene);
    if !record.matches(&snapshot) || snapshot.step_count != record.step_count {
        return Err(AppError::Format(format!(
            "take replay landed on (animation {:?}, frame {}, steps {}) instead of the take's \
             (animation {:?}, frame {}, steps {})",
            snapshot.animation,
            snapshot.frame,
            snapshot.step_count,
            record.animation,
            record.frame,
            record.step_count
        )));
    }
    if frames != take.frames {
        return Err(AppError::Format(format!(
            "take replay emitted {frames} frames; the take declares {}",
            take.frames
        )));
    }
    let report = ReplayReport {
        frames,
        frame_sequence_sha256: format!("{:x}", hasher.finalize()),
        session_state_hash: session.state_hash(),
    };
    Ok((report, scene))
}

/// What one take export produced.
#[derive(Debug, Clone)]
pub struct TakeExportReport {
    /// Frames replayed and rendered.
    pub frames: u64,
    /// The take's authored duration in seconds.
    pub duration_seconds: f64,
    /// The take's hash (as recorded in the document).
    pub take_sha256: String,
    /// The replay's frame-sequence hash (equals a
    /// hash-mode replay of the same take).
    pub frame_sequence_sha256: String,
    /// The restored session's state hash.
    pub session_state_hash: String,
    /// Output file names, in production order.
    pub files: Vec<String>,
}

/// Exports a take: verifies the model bytes against the
/// take's rig + motion hash, replays through the session
/// host while rendering every frame, encodes under
/// `preset` (h264 required — a preset export never falls
/// back to another codec silently), builds the timeline
/// through the CLI crate's video implementation, and
/// writes the provenance block beside the export.
pub fn export_take(
    take: &TakeDocument,
    model_path: &Path,
    out_dir: &Path,
    preset: &ExportPreset,
    force: bool,
) -> AppResult<TakeExportReport> {
    validate(take)?;
    if (f64::from(preset.fps) * STEP_SECONDS - 1.0).abs() > 1e-9 {
        return Err(AppError::Param(format!(
            "preset {} runs at {} fps; a take's step rate is {} fps and is not resampled",
            preset.name,
            preset.fps,
            1.0 / STEP_SECONDS
        )));
    }
    if !ffmpeg_has_libx264() {
        return Err(AppError::Render(format!(
            "preset {} requires libx264 (h264); this ffmpeg does not offer it",
            preset.name
        )));
    }
    let model_bytes = std::fs::read(model_path).map_err(|err| AppError::Io {
        context: "read model",
        message: err.to_string(),
    })?;
    let model_sha256 = sha256_hex(&model_bytes);
    if model_sha256 != take.model_sha256 {
        return Err(AppError::Param(format!(
            "model {} hashes to {model_sha256}; the take was captured on {} — \
             an export never substitutes a different rig + motion document",
            model_path.display(),
            take.model_sha256
        )));
    }
    if out_dir.exists() && !force {
        let occupied = std::fs::read_dir(out_dir)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if occupied {
            return Err(AppError::Param(format!(
                "take export dir {} is not empty; pass force to reuse it",
                out_dir.display()
            )));
        }
    }
    let frames_dir = out_dir.join("frames");
    std::fs::create_dir_all(&frames_dir).map_err(|err| AppError::Io {
        context: "create take export",
        message: err.to_string(),
    })?;
    let mut scene = Scene::load_bytes(&model_bytes, &model_path.display().to_string())?;
    scene.evaluate_fresh(&[])?;
    write_take(&out_dir.join(TAKE_FILE), take, Some(model_path), true)?;
    let mut renderer = Renderer::new()?;
    let mut render_frame = |scene: &mut Scene, frame: u64| -> AppResult<()> {
        let (image, _) = renderer.render(scene)?;
        let encoded = encode_png(&image)?;
        let name = format!("frame-{frame:05}.png");
        std::fs::write(frames_dir.join(&name), encoded).map_err(|err| AppError::Io {
            context: "write frame",
            message: err.to_string(),
        })?;
        Ok(())
    };
    let (replay_report, _) = replay(take, scene, &mut render_frame)?;

    // Encode the performance clip under the preset: fit
    // the native frame inside the preset geometry,
    // preserving aspect, pad centred on black.
    let performance_path = out_dir.join(TAKE_CLIP);
    run_ffmpeg(&[
        "-y".into(),
        "-framerate".into(),
        format!("{}", preset.fps),
        "-i".into(),
        frames_dir.join("frame-%05d.png").display().to_string(),
        "-vf".into(),
        format!(
            "scale={}:{}:force_original_aspect_ratio=decrease:force_divisible_by=2,\
             pad={}:{}:-1:-1:black,setsar=1",
            preset.width, preset.height, preset.width, preset.height
        ),
        "-c:v".into(),
        "libx264".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-r".into(),
        format!("{}", preset.fps),
        performance_path.display().to_string(),
    ])?;

    // The timeline, through the CLI crate's own video
    // implementation (the bridge's path).
    let duration = replay_report.frames as f64 * STEP_SECONDS;
    let clip_list = json!({
        "clips": [{
            "id": "performance",
            "in": 0.0,
            "out": duration,
            "source": TAKE_CLIP,
        }],
    });
    let timeline = torogao_cli::video::build_timeline(&clip_list);
    let base_dir = out_dir.display().to_string();
    let findings = torogao_cli::video::validate_timeline(&timeline, &base_dir);
    let errors: Vec<String> = findings
        .iter()
        .filter(|finding| finding.severity == "error")
        .map(|finding| format!("{}: {}", finding.code, finding.message))
        .collect();
    if !errors.is_empty() {
        return Err(AppError::Format(format!(
            "the built timeline does not validate: {}",
            errors.join("; ")
        )));
    }
    std::fs::write(
        out_dir.join(TAKE_TIMELINE_FILE),
        format!("{}\n", to_d_json(&timeline)),
    )
    .map_err(|err| AppError::Io {
        context: "write timeline",
        message: err.to_string(),
    })?;
    let concat = torogao_cli::video::concat_list(&timeline);
    std::fs::write(out_dir.join("concat.txt"), concat).map_err(|err| AppError::Io {
        context: "write concat list",
        message: err.to_string(),
    })?;
    let edl = json!({"format": "torogao-edl/1", "timeline": timeline});
    std::fs::write(out_dir.join("edl.json"), format!("{}\n", to_d_json(&edl))).map_err(|err| {
        AppError::Io {
            context: "write edl",
            message: err.to_string(),
        }
    })?;
    let chapters = torogao_cli::video::chapters_file(&timeline);
    std::fs::write(out_dir.join("chapters.txt"), chapters).map_err(|err| AppError::Io {
        context: "write chapters",
        message: err.to_string(),
    })?;
    let final_path = out_dir.join(TAKE_FINAL_CLIP);
    run_ffmpeg(&[
        "-y".into(),
        "-f".into(),
        "concat".into(),
        "-safe".into(),
        "0".into(),
        "-i".into(),
        out_dir.join("concat.txt").display().to_string(),
        "-c".into(),
        "copy".into(),
        final_path.display().to_string(),
    ])?;

    // The provenance block, beside the export.
    let duration_seconds = torogao_cli::video::timeline_duration(&timeline);
    let provenance = json!({
        "format": PROVENANCE_FORMAT,
        "take_sha256": take.take_sha256,
        "session_replay_hash": replay_report.session_state_hash,
        "rig_motion_sha256": take.model_sha256,
        "frame_sequence_sha256": replay_report.frame_sequence_sha256,
        "frames": replay_report.frames,
        "fps": preset.fps,
        "duration_seconds": duration_seconds,
        "preset": {
            "name": preset.name,
            "width": preset.width,
            "height": preset.height,
            "codec": "h264",
        },
    });
    std::fs::write(
        out_dir.join(PROVENANCE_FILE),
        format!("{}\n", to_d_json(&provenance)),
    )
    .map_err(|err| AppError::Io {
        context: "write provenance",
        message: err.to_string(),
    })?;

    Ok(TakeExportReport {
        frames: replay_report.frames,
        duration_seconds,
        take_sha256: take.take_sha256.clone(),
        frame_sequence_sha256: replay_report.frame_sequence_sha256,
        session_state_hash: replay_report.session_state_hash,
        files: vec![
            TAKE_FILE.to_string(),
            TAKE_CLIP.to_string(),
            TAKE_TIMELINE_FILE.to_string(),
            "concat.txt".to_string(),
            "edl.json".to_string(),
            "chapters.txt".to_string(),
            TAKE_FINAL_CLIP.to_string(),
            PROVENANCE_FILE.to_string(),
        ],
    })
}

/// The take export's headless entry: prints the report as
/// `key: value` lines (the gate reads them) and returns
/// the process exit code.
pub fn run_take_export(
    take_path: &Path,
    model: &Path,
    out_dir: &Path,
    force: bool,
) -> AppResult<i32> {
    let take = read_take(take_path)?;
    let report = export_take(&take, model, out_dir, &PRESET_HD_1080P60, force)?;
    println!("take-model: {}", take.model);
    println!("take-frames: {}", report.frames);
    println!("take-duration-seconds: {}", report.duration_seconds);
    println!("take-sha256: {}", report.take_sha256);
    println!(
        "take-frame-sequence-sha256: {}",
        report.frame_sequence_sha256
    );
    println!("take-session-replay-hash: {}", report.session_state_hash);
    println!("take-preset: {}", PRESET_HD_1080P60.name);
    for file in &report.files {
        println!("take-file: {file}");
    }
    println!("take-out: {}", out_dir.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{PlayOp, TakeDocument, capture, parse, serialize, trajectory_frames, validate};
    use crate::player::PlayState;
    use std::collections::BTreeMap;
    use torogao_session::session::Session;
    use torogao_session::state::EditorState;

    fn genesis() -> EditorState {
        EditorState {
            format: torogao_session::state::STATE_FORMAT.to_string(),
            project: torogao_session::state::ProjectMeta {
                name: "take-test".to_string(),
                source: "test".to_string(),
            },
            scene: torogao_session::state::Scene {
                name: "take-test".to_string(),
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
        }
    }

    fn document() -> TakeDocument {
        let session = Session::new(genesis(), &BTreeMap::new(), "take tests");
        let play = PlayState {
            animation: Some("Blink".to_string()),
            playing: true,
            looping: true,
            step_count: 3,
            ..Default::default()
        };
        let trajectory = vec![
            PlayOp::Play {
                animation: Some("Blink".to_string()),
                looping: true,
                speed: None,
            },
            PlayOp::Step { steps: 3 },
        ];
        capture(
            &session,
            &play,
            trajectory,
            "model.inx",
            "0123abcd",
            "0123abcd",
            &BTreeMap::new(),
        )
        .expect("capture")
    }

    #[test]
    fn trajectory_frames_counts_only_advancing_ops() {
        let trajectory = vec![
            PlayOp::Play {
                animation: None,
                looping: false,
                speed: None,
            },
            PlayOp::Step { steps: 45 },
            PlayOp::Pause,
            PlayOp::Seek { frame: 60 },
            PlayOp::Step { steps: 1 },
            PlayOp::ApplyState {
                state: "joy".to_string(),
            },
            PlayOp::Stop { immediate: true },
        ];
        assert_eq!(trajectory_frames(&trajectory).expect("counts"), 47);
    }

    #[test]
    fn take_document_round_trips_and_hash_verifies() {
        let doc = document();
        let text = serialize(&doc).expect("serialize");
        let parsed = parse(&text).expect("parse (the hash verifies)");
        assert_eq!(parsed.take_sha256, doc.take_sha256);
        assert_eq!(serialize(&parsed).expect("re-serialize"), text);
    }

    #[test]
    fn parse_refuses_a_tampered_take() {
        let doc = document();
        let text = serialize(&doc).expect("serialize");
        let tampered = text.replace("model.inx", "other.inx");
        assert!(parse(&tampered).is_err());
    }

    #[test]
    fn parse_refuses_a_foreign_format() {
        let mut doc = document();
        doc.format = "torogao-take/0".to_string();
        doc.take_sha256 = super::take_hash(&doc).expect("hash");
        assert!(validate(&doc).is_err());
    }

    #[test]
    fn capture_refuses_a_snapshot_trajectory_mismatch() {
        let session = Session::new(genesis(), &BTreeMap::new(), "take tests");
        let play = PlayState {
            step_count: 2,
            ..Default::default()
        };
        let trajectory = vec![PlayOp::Step { steps: 3 }];
        assert!(
            capture(
                &session,
                &play,
                trajectory,
                "model.inx",
                "0123abcd",
                "0123abcd",
                &BTreeMap::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn capture_refuses_a_frameless_trajectory() {
        let session = Session::new(genesis(), &BTreeMap::new(), "take tests");
        let play = PlayState::default();
        let trajectory = vec![PlayOp::Play {
            animation: Some("Blink".to_string()),
            looping: false,
            speed: None,
        }];
        assert!(
            capture(
                &session,
                &play,
                trajectory,
                "model.inx",
                "0123abcd",
                "0123abcd",
                &BTreeMap::new(),
            )
            .is_err()
        );
    }
}
