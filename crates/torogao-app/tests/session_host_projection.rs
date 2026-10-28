//! Phase C projection gate: the session host's genesis is
//! projected from the shell's live scene (Prima / argenta),
//! and admitted ops land back in the same puppet — proven by
//! the live evaluation hash moving when a parameter op is
//! applied through the host.
//!
//! Fixtures come from TOROGAO_GATE_FIXTURES when set;
//! otherwise the cast-rig directory next to the port
//! workspace is used when present. With no fixture the test
//! skips with a printed reason, like the parity harness.

use torogao_app::scene::Scene;
use torogao_app::session_host::{AppBridge, project_genesis};
use torogao_app::state::ShellState;
use torogao_session::host::Host;
use torogao_session::session::{Session, replay_record};

fn fixture(name: &str) -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("TOROGAO_GATE_FIXTURES") {
        let path = std::path::Path::new(&dir).join(name);
        if path.exists() {
            return Some(path);
        }
    }
    let candidate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../torogao-cast-rigs-20261026/rigs")
        .join(name);
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

fn prima_shell() -> Option<ShellState> {
    let path = fixture("argenta.inx")?;
    let mut scene = Scene::load_file(&path).unwrap();
    scene.evaluate_fresh(&[]).unwrap();
    let mut shell = ShellState::new();
    shell.install_scene(scene);
    Some(shell)
}

#[test]
fn projection_is_the_live_prima_state() {
    let Some(shell) = prima_shell() else {
        eprintln!("skip: argenta.inx fixture not present");
        return;
    };
    let (genesis, defaults) = project_genesis(&shell).unwrap();
    assert_eq!(genesis.project.name, "Prima");
    assert_eq!(genesis.scene.nodes.len(), 39, "live node census");
    assert_eq!(
        genesis.params.len() + genesis.uncapturable.len(),
        25,
        "live param census (scalar projected + vec2 uncapturable): {:?}",
        genesis.uncapturable
    );
    assert_eq!(genesis.bindings.len(), 30, "live binding census");
    let head = genesis
        .scene
        .nodes
        .iter()
        .find(|n| n.name == "HeadAnchor")
        .expect("HeadAnchor is in the live rig");
    assert_eq!(head.id, 1020);
    let angle = &genesis.params["ParamAngleX"];
    assert_eq!((angle.min, angle.max), (-30.0, 30.0));
    assert!(defaults.contains_key("ParamAngleX"));
    assert!(genesis.selection.is_empty(), "shell starts unselected");
    assert!(genesis.uncapturable.is_empty(), "Prima projects fully");
}

#[test]
fn host_ops_move_the_live_evaluation() {
    let Some(shell) = prima_shell() else {
        eprintln!("skip: argenta.inx fixture not present");
        return;
    };
    let (genesis, defaults) = project_genesis(&shell).unwrap();
    let genesis_hash = torogao_session::state_hash(&genesis);
    let session = Session::new(genesis, &defaults, "projection gate");
    let mut host = Host::new(session);
    let mut bridge = AppBridge::new(shell);
    // WS2-W5 re-expression: the Phase C version drove the
    // wire dialect (handle_line + initialize handshake) and
    // indexed [result]/[error] frames. The handshake is the
    // era guard's contract now; here the domain dispatcher is
    // driven directly and outcomes are payloads / Refusals.
    let call = |host: &mut Host, bridge: &mut AppBridge, name: &str, args: serde_json::Value| {
        host.call_tool(name, &args, bridge)
    };

    let before = call(&mut host, &mut bridge, "play_state", serde_json::json!({})).unwrap();
    let eval_before = before["eval_hash"].as_str().unwrap().to_string();

    // Select HeadAnchor through the session, read its context.
    let r = call(
        &mut host,
        &mut bridge,
        "session_apply_op",
        serde_json::json!({"base_hash": genesis_hash, "op": {"op": "selection_set", "nodes": [1020]}}),
    )
    .unwrap();
    let after_select = r["post"].as_str().unwrap().to_string();
    let ctx = call(
        &mut host,
        &mut bridge,
        "selection_context",
        serde_json::json!({}),
    )
    .unwrap();
    assert_eq!(ctx["selection"][0]["node"]["name"], "HeadAnchor");

    // The author's range refusal, through the host, live.
    let r = call(
        &mut host,
        &mut bridge,
        "session_apply_op",
        serde_json::json!({"base_hash": after_select, "op": {"op": "param_set", "param": "ParamAngleX", "value": 45.0}}),
    );
    assert_eq!(r.unwrap_err().code, "TG-AUTH-RANGE");

    // The admitted op lands in the live puppet: the
    // evaluation hash must move.
    let r = call(
        &mut host,
        &mut bridge,
        "session_apply_op",
        serde_json::json!({"base_hash": after_select, "op": {"op": "param_set", "param": "ParamAngleX", "value": 12.5}}),
    )
    .unwrap();
    let post = r["post"].as_str().unwrap().to_string();
    assert_eq!(r["verdict"], "apply");
    let after = call(&mut host, &mut bridge, "play_state", serde_json::json!({})).unwrap();
    let eval_after = after["eval_hash"].as_str().unwrap();
    assert_ne!(
        eval_before, eval_after,
        "the live evaluation must move when an op is applied"
    );

    // Cross-check the posed value against the shell's own
    // edit trajectory: a second scene taken through the same
    // steps the bridge took (fresh evaluation, then the
    // panel's incremental set_parameter path — driver state
    // carried, one more step) hashes identically to the
    // live scene. (The fresh protocol alone is a different
    // trajectory: Prima carries live pendulum drivers, so
    // step counts are part of the evaluated state.)
    let path = fixture("argenta.inx").unwrap();
    let mut check = Scene::load_file(&path).unwrap();
    check.evaluate_fresh(&[]).unwrap();
    let index = check.parameter_index("ParamAngleX").unwrap();
    let y = check.puppet.params[index].value.y;
    check
        .set_parameter(index, torogao_engine::Vec2::new(12.5, y))
        .unwrap();
    let check_hash = format!(
        "{:016x}",
        torogao_app::evaluated_state_hash(&mut check.puppet)
    );
    assert_eq!(
        eval_after, check_hash,
        "session-posed evaluation == same-trajectory evaluation"
    );

    // Compile + fresh replay x3, byte-stable.
    let dir = std::env::temp_dir().join(format!("tg-app-host-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let r = call(
        &mut host,
        &mut bridge,
        "session_compile",
        serde_json::json!({"out_dir": dir.to_string_lossy()}),
    )
    .unwrap();
    assert_eq!(r["final_hash"], post);
    for _ in 0..3 {
        assert_eq!(replay_record(&dir).unwrap(), post);
    }
}
