//! Phase C gate tests for torogao-session: the record
//! contract carried from the Slice-1 harness (canonical RON
//! round-trip, chain verification, compile + fresh replay),
//! plus the decide() swap evidence — refusals now come from
//! torogao-author's real AUTH-D2 `decide()`, and the op the
//! authoring manifest cannot express is refused by name.

use std::collections::BTreeMap;
use torogao_session::canon::fmt_f32;
use torogao_session::host::{Host, LiveBridge};
use torogao_session::op::Op;
use torogao_session::refusal::{R, Refusal};
use torogao_session::session::{Session, compile_session, replay_record};
use torogao_session::state::{
    Binding, EditorState, Node, NodeKind, ParamState, ProjectMeta, STATE_FORMAT, Scene, Transform,
    Viewport,
};
use torogao_session::{from_canon_ron, state_hash, to_canon_ron};

fn rig() -> (EditorState, BTreeMap<String, f32>) {
    let mut params = BTreeMap::new();
    params.insert(
        "ParamAngleX".to_string(),
        ParamState {
            value: 0.0,
            min: -30.0,
            max: 30.0,
        },
    );
    params.insert(
        "ParamMouthOpenY".to_string(),
        ParamState {
            value: 0.25,
            min: 0.0,
            max: 1.0,
        },
    );
    let mut defaults = BTreeMap::new();
    defaults.insert("ParamAngleX".to_string(), 0.0);
    defaults.insert("ParamMouthOpenY".to_string(), 0.0);
    let tr = Transform {
        tx: 0.0,
        ty: 0.0,
        rot: 0.0,
        sx: 1.0,
        sy: 1.0,
    };
    (
        EditorState {
            format: STATE_FORMAT.to_string(),
            project: ProjectMeta {
                name: "TestRig".to_string(),
                source: "unit".to_string(),
            },
            scene: Scene {
                name: "TestRig".to_string(),
                nodes: vec![
                    Node {
                        id: 10,
                        name: "Root".to_string(),
                        parent: None,
                        kind: NodeKind::Node,
                        visible: true,
                        zsort: 0.0,
                        transform: tr,
                    },
                    Node {
                        id: 20,
                        name: "Face".to_string(),
                        parent: Some(10),
                        kind: NodeKind::Part,
                        visible: true,
                        zsort: 1.5,
                        transform: tr,
                    },
                    Node {
                        id: 30,
                        name: "HairSway".to_string(),
                        parent: Some(10),
                        kind: NodeKind::SimplePhysics,
                        visible: true,
                        zsort: 2.0,
                        transform: tr,
                    },
                ],
            },
            selection: vec![20],
            viewport: Viewport {
                cam_x: 0.0,
                cam_y: 0.0,
                zoom: 1.0,
            },
            params,
            bindings: vec![Binding {
                param: "ParamAngleX".to_string(),
                node: 20,
                property: "angle".to_string(),
                kind: "value".to_string(),
            }],
            uncapturable: vec![],
        },
        defaults,
    )
}

fn session() -> Session {
    let (st, defaults) = rig();
    Session::new(st, &defaults, "gate tests")
}

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tg-session-gate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn canon_roundtrip_and_hash_stable() {
    let (st, _) = rig();
    let text = to_canon_ron(&st);
    let back = from_canon_ron(&text).unwrap();
    assert_eq!(state_hash(&back), state_hash(&st));
    assert_eq!(to_canon_ron(&back), text);
}

#[test]
fn float_canon_cases() {
    assert_eq!(fmt_f32(0.0), "0.0");
    assert_eq!(fmt_f32(-0.0), "0.0");
    assert_eq!(fmt_f32(12.5), "12.5");
    assert_eq!(fmt_f32(-30.0), "-30.0");
}

#[test]
fn out_of_range_param_is_refused_by_the_author_and_leaves_no_trace() {
    let mut s = session();
    let before = s.state_hash();
    let err = s
        .apply_op(&Op::ParamSet {
            param: "ParamAngleX".to_string(),
            value: 45.0,
        })
        .unwrap_err();
    // The refusal is torogao-author's in its own words — the
    // Slice-1 stub's message ("param X: 45 outside [-30, 30]")
    // cannot produce this text; only the real decide() can.
    assert_eq!(err.code, "TG-AUTH-RANGE");
    assert!(
        err.message
            .contains("is outside parameter 'ParamAngleX' range [-30, 30]"),
        "unexpected refusal message: {}",
        err.message
    );
    assert_eq!(s.state_hash(), before);
    assert!(s.log.is_empty());
}

#[test]
fn node_transform_op_is_refused_by_name() {
    let mut s = session();
    let err = s
        .apply_op(&Op::NodeSetTransform {
            node: 20,
            tx: 5.0,
            ty: 0.0,
            rot: 0.0,
            sx: 1.0,
            sy: 1.0,
        })
        .unwrap_err();
    assert_eq!(err.code, "TG-AUTH-UNKNOWN-KIND");
    assert!(s.log.is_empty());
}

#[test]
fn unknown_op_kind_and_unknown_targets_refuse_typed() {
    let mut s = session();
    let err = Op::from_json(&serde_json::json!({"op": "explode"})).unwrap_err();
    assert_eq!(err.code, "TG-AUTH-UNKNOWN-KIND");
    let err = s
        .apply_op(&Op::ParamSet {
            param: "ParamNope".to_string(),
            value: 1.0,
        })
        .unwrap_err();
    assert_eq!(err.code, "TG-AUTH-UNKNOWN-TARGET");
    let err = s
        .apply_op(&Op::NodeSetVisible {
            node: 999,
            visible: true,
        })
        .unwrap_err();
    assert_eq!(err.code, "TG-AUTH-UNKNOWN-TARGET");
}

#[test]
fn nonfinite_op_and_duplicate_selection_refuse() {
    let mut s = session();
    let err = s
        .apply_op(&Op::ParamSet {
            param: "ParamAngleX".to_string(),
            value: f32::NAN,
        })
        .unwrap_err();
    assert_eq!(err.code, "TG-SES-NONFINITE");
    let err = s
        .apply_op(&Op::SelectionSet {
            nodes: vec![20, 20],
        })
        .unwrap_err();
    assert_eq!(err.code, "TG-AUTH-CONFLICT");
    assert!(s.log.is_empty());
}

#[test]
fn unknown_field_in_state_refuses() {
    let (st, _) = rig();
    let mut v = serde_json::to_value(&st).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("surprise".to_string(), serde_json::json!(1));
    let err = EditorState::from_json(&v.to_string()).unwrap_err();
    assert_eq!(err.code, "TG-SES-UNKNOWN-FIELD");
}

#[test]
fn applied_ops_chain_and_verify() {
    let mut s = session();
    s.apply_op(&Op::ParamSet {
        param: "ParamAngleX".to_string(),
        value: 12.5,
    })
    .unwrap();
    s.apply_op(&Op::NodeSetVisible {
        node: 20,
        visible: false,
    })
    .unwrap();
    s.apply_op(&Op::SelectionSet { nodes: vec![30] }).unwrap();
    s.apply_op(&Op::ViewportSet {
        cam_x: 4.0,
        cam_y: -2.0,
        zoom: 1.25,
    })
    .unwrap();
    // Re-posing the current value is the author's NoOp; the
    // op still logs (the record is of ops, not of effects).
    let e = s
        .apply_op(&Op::ParamSet {
            param: "ParamAngleX".to_string(),
            value: 12.5,
        })
        .unwrap();
    assert_eq!(e.pre, e.post);
    let (replayed, live, equal) = s.verify().unwrap();
    assert!(equal, "replayed {replayed} != live {live}");
    assert_eq!(s.log.len(), 5);
}

#[test]
fn compile_then_fresh_replay_x3_identical() {
    let mut s = session();
    s.apply_op(&Op::ParamSet {
        param: "ParamAngleX".to_string(),
        value: -7.25,
    })
    .unwrap();
    s.apply_op(&Op::ParamSet {
        param: "ParamMouthOpenY".to_string(),
        value: 0.5,
    })
    .unwrap();
    let dir = tmpdir("compile");
    let outcome = compile_session(&s, &dir).unwrap();
    assert_eq!(outcome.final_hash, s.state_hash());
    for _ in 0..3 {
        let h = replay_record(&dir).unwrap();
        assert_eq!(h, outcome.final_hash);
    }
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("session.json")).unwrap()).unwrap();
    assert_eq!(meta["held_by"], "gate tests");
}

#[test]
fn uncapturable_state_compiles_to_refusal_and_writes_nothing() {
    let (mut st, defaults) = rig();
    st.uncapturable.push("params/ParamVec2".to_string());
    let s = Session::new(st, &defaults, "gate tests");
    let dir = tmpdir("uncapturable");
    let err = compile_session(&s, &dir).unwrap_err();
    assert_eq!(err.code, "TG-SES-UNCAPTURABLE");
    assert!(!dir.join("project.ron").exists());
}

#[test]
fn tampered_log_breaks_replay() {
    let mut s = session();
    s.apply_op(&Op::ParamSet {
        param: "ParamAngleX".to_string(),
        value: 3.0,
    })
    .unwrap();
    let mut log = s.log.clone();
    log[0].post = "0".repeat(64);
    let err = torogao_session::session::replay_ops(&s.genesis, &log, &s.id).unwrap_err();
    assert_eq!(err.code, "TG-SES-LOG-CHAIN");
}

// ------------------------------------------------------------ host level

struct AcceptBridge;

impl LiveBridge for AcceptBridge {
    fn apply_live(&mut self, _op: &Op) -> R<()> {
        Ok(())
    }
    fn evaluated_hash(&mut self) -> Option<String> {
        Some("0123456789abcdef".to_string())
    }
    fn play_state(&mut self) -> Option<torogao_session::PlaySnapshot> {
        Some(torogao_session::PlaySnapshot {
            animation: Some("Blink".to_string()),
            speed: 1.0,
            ..Default::default()
        })
    }
    fn play_control(
        &mut self,
        action: &str,
        _args: &serde_json::Value,
    ) -> R<torogao_session::PlaySnapshot> {
        let mut snapshot = self.play_state().expect("accept bridge has a player");
        snapshot.playing = action == "play";
        Ok(snapshot)
    }
}

/// WS2-W5 re-expression: this flow drove the Phase C wire
/// dialect (`Host::handle_line`) and asserted its handshake
/// refusals. Framing and auth are the era guard's contract
/// now (unit-tested in `mcp_guard`: TG-SES-AUTH on missing /
/// wrong token, TG-SES-NO-SESSION retired with the
/// handshake), so this gate drives the domain dispatcher the
/// guard forwards to and asserts the same Phase C outcomes:
/// stale → refused-op → live eval hash → admitted op.
fn call(
    host: &mut Host,
    bridge: &mut dyn LiveBridge,
    name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, Refusal> {
    host.call_tool(name, &arguments, bridge)
}

#[test]
fn host_stale_and_refused_op_flow() {
    let mut host = Host::new(session());
    let mut bridge = AcceptBridge;
    let genesis = host.session.genesis_hash();
    // Stale base_hash refuses before the gate runs.
    let err = call(
        &mut host,
        &mut bridge,
        "session_apply_op",
        serde_json::json!({
            "base_hash": "deadbeef",
            "op": {"op": "param_set", "param": "ParamAngleX", "value": 1.0}}),
    )
    .unwrap_err();
    assert_eq!(err.code, "TG-SES-STALE");
    // The author's range refusal passes through the host.
    let err = call(
        &mut host,
        &mut bridge,
        "session_apply_op",
        serde_json::json!({
            "base_hash": genesis,
            "op": {"op": "param_set", "param": "ParamAngleX", "value": 45.0}}),
    )
    .unwrap_err();
    assert_eq!(err.code, "TG-AUTH-RANGE");
    // play_state serves the bridge's live evaluation hash
    // and its live player snapshot.
    let r = call(&mut host, &mut bridge, "play_state", serde_json::json!({})).unwrap();
    assert_eq!(r["eval_hash"], "0123456789abcdef");
    assert_eq!(r["current_animation"], "Blink");
    assert_eq!(r["player"], "live");
    // play_control drives the player and never op-logs.
    let log_len = host.session.log.len();
    let r = call(
        &mut host,
        &mut bridge,
        "play_control",
        serde_json::json!({"action": "play"}),
    )
    .unwrap();
    assert_eq!(r["playing"], true);
    assert_eq!(host.session.log.len(), log_len);
    // An admitted op reports the author's verdict.
    let r = call(
        &mut host,
        &mut bridge,
        "session_apply_op",
        serde_json::json!({
            "base_hash": genesis,
            "op": {"op": "param_set", "param": "ParamAngleX", "value": 12.5}}),
    )
    .unwrap();
    assert_eq!(r["verdict"], "apply");
    // An unknown tool name refuses as an unknown op kind,
    // exactly as Phase C's dispatcher answered it.
    let err = call(
        &mut host,
        &mut bridge,
        "no_such_tool",
        serde_json::json!({}),
    )
    .unwrap_err();
    assert_eq!(err.code, "TG-AUTH-UNKNOWN-KIND");
}
