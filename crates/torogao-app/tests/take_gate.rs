//! Stage 3.1 gate: takes captured through the real
//! session host (the save-roundtrip path: gate-admitted
//! ops plus bridge-driven play, trajectory recorded by the
//! bridge itself) replay deterministically through the
//! session host, and a ≥ 120 s take exports at the
//! 1080p60 preset with a provenance block whose hashes
//! match the replay's.
//!
//! Fixture: rich.inx (in-tree; animation "Blink", 60
//! frames, looping gives the long take its length class).
//! The Phase E timeline-hash anchor for the Greeting
//! fixture resolves through TOROGAO_PHASE_D_FIXTURES and
//! skips when the gate environment does not provide it
//! (the estate's fixture pattern).

use std::path::PathBuf;

use serde_json::{Value, json};
use sha2::Digest;
use torogao_app::player::PlayHost;
use torogao_app::scene::Scene;
use torogao_app::session_host::{AppBridge, project_genesis};
use torogao_app::state::ShellState;
use torogao_app::take::{self, PRESET_HD_1080P60, TakeDocument};
use torogao_app::video_bridge::{BridgeOptions, run_bridge};
use torogao_session::host::{Host, LiveBridge};
use torogao_session::session::Session;

fn rich_model() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/p15/rich.inx")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Captures a take of `steps` fixed steps of the looping
/// Blink performance on rich.inx, through the real host:
/// one gate-admitted ParamSet op (the genesis' first
/// scalar parameter, set to its standing value) followed
/// by bridge play_control actions, whose play log is the
/// take's trajectory.
fn capture_rich_take(steps: u64) -> (TakeDocument, PathBuf) {
    let path = rich_model();
    let model_bytes = std::fs::read(&path).expect("fixture is readable");
    let model_sha256 = sha256_hex(&model_bytes);
    let mut scene = Scene::load_file(&path).expect("fixture loads");
    scene.evaluate_fresh(&[]).expect("fixture evaluates");
    let mut shell = ShellState::new();
    shell.install_scene(scene);
    let (genesis, defaults) = project_genesis(&shell).expect("genesis projects");
    let genesis_hash = torogao_session::state_hash(&genesis);
    let session = Session::new(genesis.clone(), &defaults, "take gate");
    let mut host = Host::new(session);
    let mut bridge = AppBridge::with_player(shell, PlayHost::new());
    if let Some((name, param)) = genesis.params.iter().next() {
        let applied = host
            .call_tool(
                "session_apply_op",
                &json!({"base_hash": genesis_hash, "op": {"op": "param_set", "param": name, "value": param.value}}),
                &mut bridge,
            )
            .unwrap_or_else(|refusal| {
                panic!("op refused: {}: {}", refusal.code, refusal.message)
            });
        assert!(applied["post"].as_str().is_some(), "the op lands");
    }
    for args in [
        json!({"action": "play", "animation": "Blink", "loop": true}),
        json!({"action": "step", "steps": steps}),
    ] {
        host.call_tool("play_control", &args, &mut bridge)
            .unwrap_or_else(|refusal| {
                panic!("play refused: {}: {}", refusal.code, refusal.message)
            });
    }
    let evaluated = bridge.evaluated_hash().expect("bridge evaluates");
    let trajectory = bridge.play_log().to_vec();
    let (shell, player) = bridge.into_parts();
    let scene = shell.scene.as_ref().expect("scene installed");
    let play_state = player.snapshot(scene);
    let document = take::capture(
        &host.session,
        &play_state,
        trajectory,
        &path.display().to_string(),
        &model_sha256,
        &evaluated,
        &defaults,
    )
    .expect("take captures");
    (document, path)
}

fn replay_hash(take: &TakeDocument, model: &std::path::Path) -> take::ReplayReport {
    let mut scene = Scene::load_file(model).expect("fixture reloads");
    scene.evaluate_fresh(&[]).expect("fixture re-evaluates");
    let (report, _) = take::replay(take, scene, &mut |_, _| Ok(())).expect("take replays");
    report
}

#[test]
fn take_replay_is_deterministic_x3() {
    let (document, path) = capture_rich_take(180);
    assert_eq!(document.frames, 180);
    assert!(
        !document.save.ops.is_empty(),
        "the gate-admitted op is in the take's log"
    );
    // The file is the record: replays run on the parsed
    // document, not the in-memory one.
    let text = take::serialize(&document).expect("serialize");
    let parsed = take::parse(&text).expect("parse");
    let mut hashes = Vec::new();
    for _ in 0..3 {
        let report = replay_hash(&parsed, &path);
        assert_eq!(report.frames, 180);
        assert_eq!(report.session_state_hash, parsed.save.session_state_hash);
        hashes.push(report.frame_sequence_sha256);
    }
    eprintln!("frame-sequence-sha256: {}", hashes[0]);
    assert_eq!(hashes[0], hashes[1], "replay 1 == replay 2");
    assert_eq!(hashes[1], hashes[2], "replay 2 == replay 3");
}

#[test]
fn take_export_1080p60_meets_the_ffprobe_gate() {
    let (document, path) = capture_rich_take(7_200);
    assert_eq!(document.frames, 7_200);
    // The 120 s replay itself is CPU work and always runs;
    // only the render is GPU-bound.
    let expected = replay_hash(&document, &path);
    eprintln!("frame-sequence-sha256: {}", expected.frame_sequence_sha256);
    if torogao_app::render::Renderer::new().is_err() {
        // The estate's GPU-gate pattern (texture_8k, the
        // Phase E primo gate): without an adapter the
        // render leg is reported, not faked. Run this test
        // on primo for the full gate.
        eprintln!("skipping render leg: no GPU adapter in this environment");
        return;
    }
    let out_dir = std::env::temp_dir().join(format!("torogao-take-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    let report = take::export_take(&document, &path, &out_dir, &PRESET_HD_1080P60, false)
        .expect("take exports");
    assert_eq!(report.frames, 7_200);
    assert_eq!(
        report.frame_sequence_sha256, expected.frame_sequence_sha256,
        "the export rendered the same replay the hash gate folded"
    );
    assert_eq!(report.session_state_hash, document.save.session_state_hash);

    // ffprobe: the exported final clip is 1920x1080,
    // 60 fps, h264, within one frame of the authored
    // 120 s.
    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,avg_frame_rate",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(out_dir.join("final.mp4"))
        .output()
        .expect("ffprobe runs");
    assert!(probe.status.success(), "ffprobe succeeds");
    let probed: Value = serde_json::from_slice(&probe.stdout).expect("ffprobe json");
    let stream = &probed["streams"][0];
    assert_eq!(stream["codec_name"], "h264");
    assert_eq!(stream["width"], 1920);
    assert_eq!(stream["height"], 1080);
    assert_eq!(stream["avg_frame_rate"], "60/1");
    let duration: f64 = probed["format"]["duration"]
        .as_str()
        .expect("duration present")
        .parse()
        .expect("duration numeric");
    assert!(
        (duration - 120.0).abs() <= 1.0 / 60.0 + 0.001,
        "duration {duration} is within one frame of 120 s"
    );

    // The provenance block cites the same hashes.
    let provenance: Value = serde_json::from_str(
        &std::fs::read_to_string(out_dir.join("provenance.json")).expect("provenance written"),
    )
    .expect("provenance parses");
    assert_eq!(provenance["format"], "torogao-take-provenance/1");
    assert_eq!(provenance["take_sha256"], document.take_sha256);
    assert_eq!(
        provenance["frame_sequence_sha256"],
        expected.frame_sequence_sha256
    );
    assert_eq!(
        provenance["session_replay_hash"],
        document.save.session_state_hash
    );
    assert_eq!(provenance["rig_motion_sha256"], document.model_sha256);
    assert_eq!(provenance["frames"], 7_200);
    let _ = std::fs::remove_dir_all(&out_dir);
}

#[test]
fn greeting_timeline_hash_anchor_still_reproduces() {
    let Some(dir) = std::env::var_os("TOROGAO_PHASE_D_FIXTURES") else {
        eprintln!("skipping: TOROGAO_PHASE_D_FIXTURES not provided");
        return;
    };
    let path = PathBuf::from(dir).join("prima-greeting.inx");
    if !path.exists() {
        eprintln!("skipping: prima-greeting.inx not in the fixtures dir");
        return;
    }
    let out_dir =
        std::env::temp_dir().join(format!("torogao-bridge-anchor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    let report = run_bridge(&BridgeOptions {
        model: path,
        animation: "Greeting".to_string(),
        out_dir: out_dir.clone(),
        force: false,
    })
    .expect("bridge runs");
    // PROGRAM.md Stage 3.1 records the Phase E timeline
    // hash in truncated form: 7f754463…0419.
    assert!(
        report.timeline_sha256.starts_with("7f754463") && report.timeline_sha256.ends_with("0419"),
        "timeline hash {} must still reproduce the Phase E anchor",
        report.timeline_sha256
    );
    let _ = std::fs::remove_dir_all(&out_dir);
}
