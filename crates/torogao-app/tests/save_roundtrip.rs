//! Phase E save-path gate: a scripted session driven
//! through the real host dispatch (the same `call_tool`
//! path the transports serve) is captured into a save
//! document, restored into a fresh scene + session + play
//! host, and proven equal on every recorded hash; the
//! document itself must re-serialize byte-identically
//! after a parse (the save -> load -> save idempotence).
//!
//! The script deliberately includes a seek, so the
//! restore exercises the corrective-seek leg, and one
//! gate-denied op, so the log provably contains only
//! admitted ops.
//!
//! Fixture: the Phase D performance fixture
//! (prima-greeting.inx), resolved through
//! TOROGAO_PHASE_D_FIXTURES; skipped when the gate
//! environment does not provide it.

use std::path::PathBuf;

use serde_json::{Value, json};
use torogao_app::player::PlayHost;
use torogao_app::save;
use torogao_app::scene::Scene;
use torogao_app::session_host::{AppBridge, project_genesis};
use torogao_app::state::ShellState;
use torogao_app::state_hash::evaluated_state_hash;
use torogao_session::host::{Host, LiveBridge};
use torogao_session::session::Session;

fn phase_d_fixture(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("TOROGAO_PHASE_D_FIXTURES")?;
    let path = PathBuf::from(dir).join(name);
    path.exists().then_some(path)
}

fn tool(host: &mut Host, bridge: &mut AppBridge, name: &str, args: Value) -> Value {
    host.call_tool(name, &args, bridge)
        .unwrap_or_else(|refusal| panic!("{name} refused: {}: {}", refusal.code, refusal.message))
}

#[test]
fn save_round_trip_through_the_real_host() {
    let Some(path) = phase_d_fixture("prima-greeting.inx") else {
        return;
    };
    // ---- live side: scene, session, scripted work ----
    let mut scene = Scene::load_file(&path).expect("fixture loads");
    scene.evaluate_fresh(&[]).expect("fixture evaluates");
    let mut shell = ShellState::new();
    shell.install_scene(scene);
    let (genesis, defaults) = project_genesis(&shell).expect("genesis projects");
    let genesis_hash = torogao_session::state_hash(&genesis);
    let session = Session::new(genesis, &defaults, "save roundtrip");
    let mut host = Host::new(session);
    let mut bridge = AppBridge::with_player(shell, PlayHost::new());

    let applied = tool(
        &mut host,
        &mut bridge,
        "session_apply_op",
        json!({"base_hash": genesis_hash, "op": {"op": "param_set", "param": "ParamAngleX", "value": 12.5}}),
    );
    let post = applied["post"].as_str().expect("post hash").to_string();
    tool(
        &mut host,
        &mut bridge,
        "session_apply_op",
        json!({"base_hash": post, "op": {"op": "selection_set", "nodes": [1020]}}),
    );
    // A denied op must not enter the log (range refusal).
    let denied = host.call_tool(
        "session_apply_op",
        &json!({"base_hash": post, "op": {"op": "param_set", "param": "ParamAngleX", "value": 45.0}}),
        &mut bridge,
    );
    assert!(denied.is_err(), "the range violation must refuse");

    tool(
        &mut host,
        &mut bridge,
        "play_control",
        json!({"action": "apply_state", "state": "joy"}),
    );
    tool(
        &mut host,
        &mut bridge,
        "play_control",
        json!({"action": "play", "animation": "Greeting", "loop": true}),
    );
    tool(
        &mut host,
        &mut bridge,
        "play_control",
        json!({"action": "step", "steps": 45}),
    );
    tool(
        &mut host,
        &mut bridge,
        "play_control",
        json!({"action": "seek", "frame": 60}),
    );
    tool(
        &mut host,
        &mut bridge,
        "play_control",
        json!({"action": "step", "steps": 1}),
    );

    // ---- capture ----
    let evaluated = bridge.evaluated_hash().expect("bridge evaluates");
    let trajectory = bridge.play_log().to_vec();
    let (shell, player) = bridge.into_parts();
    let scene = shell.scene.as_ref().expect("scene installed");
    let play_state = player.snapshot(scene);
    let mut document = save::capture_with_defaults(
        &host.session,
        &play_state,
        &path.display().to_string(),
        &evaluated,
        &defaults,
    );
    document.play.trajectory = Some(trajectory);
    assert_eq!(document.ops.len(), 2, "only admitted ops are saved");
    let text = save::serialize(&document).expect("serialize");

    // ---- the document survives a file round trip ----
    let dir = std::env::temp_dir().join(format!("torogao-save-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let save_path = dir.join("roundtrip.tss.json");
    save::write_save(&save_path, &document, Some(&path), false).expect("write save");
    // The output guard: saving over the model refuses, and
    // overwriting the save without force refuses.
    assert!(save::write_save(&path, &document, Some(&path), true).is_err());
    assert!(save::write_save(&save_path, &document, Some(&path), false).is_err());
    let loaded = save::read_save(&save_path).expect("read save");
    let reserialized = save::serialize(&loaded).expect("re-serialize");
    assert_eq!(text, reserialized, "save -> load -> save is byte-identical");

    // ---- restore into a fresh triple and compare ----
    let restored_session = save::restore_session(&loaded).expect("session restores");
    assert_eq!(restored_session.state_hash(), loaded.session_state_hash);
    assert_eq!(restored_session.log.len(), loaded.ops.len());

    let mut scene2 = Scene::load_file(&path).expect("fixture reloads");
    scene2.evaluate_fresh(&[]).expect("fixture re-evaluates");
    // The live scene restore replays the ops through a
    // fresh session + bridge (the restored session above
    // is already at final state; it is the hash proof,
    // not the live driver).
    let genesis2 = torogao_session::from_canon_ron(&loaded.genesis).expect("genesis reloads");
    let live_session = Session::new(genesis2, &loaded.defaults, &loaded.held_by);
    let mut shell2 = ShellState::new();
    shell2.install_scene(scene2);
    let mut host2 = Host::new(live_session);
    let mut bridge2 = AppBridge::with_player(shell2, PlayHost::new());
    let mut base = torogao_session::state_hash(&host2.session.genesis);
    for op in &loaded.ops {
        let applied = tool(
            &mut host2,
            &mut bridge2,
            "session_apply_op",
            json!({"base_hash": base, "op": op}),
        );
        base = applied["post"].as_str().expect("post hash").to_string();
    }
    assert_eq!(base, loaded.session_state_hash);
    let (shell2, mut player2) = bridge2.into_parts();
    let mut scene2 = shell2.scene.expect("scene installed");
    let report = save::restore_play(&loaded, &mut scene2, &mut player2).expect("play restores");
    assert!(
        report.trajectory_replayed,
        "the recorded trajectory (with its seek) must be the restore path"
    );
    let restored_evaluated = format!("{:016x}", evaluated_state_hash(&mut scene2.puppet));
    assert_eq!(
        restored_evaluated, loaded.evaluated_state_hash,
        "restored evaluated hash equals the captured one"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
