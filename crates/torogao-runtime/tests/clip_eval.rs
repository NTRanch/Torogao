//! The clip evaluator's gate tests (hand-animation
//! programme Rung H1): half validation (Appendix C mathematics
//! against hand-computed values and the H0 goldens), half
//! adversarial (the planted evaluation-order control, refusal
//! inheritance, clamping, overshoot).

use std::path::PathBuf;
use torogao_runtime::clip_eval::{Clip, evaluate_channel};

fn golden_text(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../torogao-format/tests/golden/motion-clip-2")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn golden(name: &str) -> Clip {
    Clip::from_text(&golden_text(&format!("{name}.clip.json")))
        .unwrap_or_else(|error| panic!("golden {name} refused: {error}"))
}

const RIG: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn scalar_hermite_doc(out_tangent: f64, in_tangent: f64) -> String {
    format!(
        "{{\"format\":\"torogao-motion-clip/2\",\"name\":\"hermite-probe\",\"rig\":\"{RIG}\",\
         \"rate\":{{\"num\":24,\"den\":1}},\"range\":{{\"start\":0,\"end\":10}},\"loop\":null,\
         \"channels\":[{{\"target\":{{\"kind\":\"parameter\",\"name\":\"Probe\",\"property\":\"value\"}},\
         \"keys\":[{{\"frame\":0,\"value\":0.0,\"interp\":\"cubic\",\"out_tangent\":{out_tangent}}},\
         {{\"frame\":10,\"value\":0.0,\"interp\":\"step\",\"in_tangent\":{in_tangent}}}]}}],\
         \"shaders\":[],\"op_log\":[{{\"seq\":0,\"actor\":\"human\",\"session\":\"test\",\"op\":\"author\"}}]}}"
    )
}

// ----------------------------------------------------------------
// Validation half.

#[test]
fn keyframes_evaluate_exactly() {
    for name in ["minimal", "loop-hold", "fac", "shader", "pose"] {
        let clip = golden(name);
        for channel in &clip.channels {
            for key in &channel.keys {
                assert_eq!(
                    evaluate_channel(channel, key.frame),
                    key.value,
                    "{name} {} at frame {}",
                    channel.target_id,
                    key.frame
                );
            }
        }
    }
}

#[test]
fn step_holds_and_linear_midpoints() {
    let fac = golden("fac");
    let smile = fac
        .channels
        .iter()
        .find(|channel| channel.target_id == "morph:Fac_MouthSmile:weight")
        .expect("fac carries the smile channel");
    // Step: 0.25 holds from frame 0 until the frame-36 key.
    assert_eq!(evaluate_channel(smile, 35), vec![0.25]);
    assert_eq!(evaluate_channel(smile, 36), vec![0.625]);
    let desire = fac
        .channels
        .iter()
        .find(|channel| channel.target_id == "parameter:Desire:value")
        .expect("fac carries the Desire channel");
    // Linear from 0.0 at frame 0 to 0.5 at frame 36: the
    // midpoint frame 18 is 0.25.
    let mid = evaluate_channel(desire, 18);
    assert!((mid[0] - 0.25).abs() < 1e-12, "got {}", mid[0]);
}

#[test]
fn cubic_hermite_matches_the_hand_computation() {
    // p0 = p1 = 0, m0 = 1, m1 = -1, delta = 10: at t = 0.5
    // the value is 0.125*10*1 + (-0.125)*10*(-1) = 2.5.
    let clip = Clip::from_text(&scalar_hermite_doc(1.0, -1.0)).expect("probe doc");
    let value = evaluate_channel(&clip.channels[0], 5);
    assert!((value[0] - 2.5).abs() < 1e-12, "got {}", value[0]);
}

#[test]
fn slerp_midpoint_of_a_quarter_turn() {
    let doc = format!(
        "{{\"format\":\"torogao-motion-clip/2\",\"name\":\"slerp-probe\",\"rig\":\"{RIG}\",\
         \"rate\":{{\"num\":24,\"den\":1}},\"range\":{{\"start\":0,\"end\":10}},\"loop\":null,\
         \"channels\":[{{\"target\":{{\"kind\":\"joint\",\"name\":\"Spine\",\"property\":\"rotation\"}},\
         \"keys\":[{{\"frame\":0,\"value\":[0,0,0,1],\"interp\":\"linear\"}},\
         {{\"frame\":10,\"value\":[0,1,0,0],\"interp\":\"step\"}}]}}],\
         \"shaders\":[],\"op_log\":[{{\"seq\":0,\"actor\":\"human\",\"session\":\"test\",\"op\":\"author\"}}]}}"
    );
    let clip = Clip::from_text(&doc).expect("slerp doc");
    let mid = evaluate_channel(&clip.channels[0], 5);
    let half = std::f64::consts::FRAC_1_SQRT_2;
    assert!((mid[1] - half).abs() < 1e-9, "y: {}", mid[1]);
    assert!((mid[3] - half).abs() < 1e-9, "w: {}", mid[3]);
}

#[test]
fn golden_evaluation_digests_are_pinned() {
    // The planted-change control: any change to the
    // evaluation order or the curve mathematics moves these
    // digests, even when individual values still look
    // plausible. Pinned from the reference run of
    // 2026-11-27; the cross-architecture band is the twin
    // gate's business, this pin is the within-architecture
    // tripwire.
    assert_eq!(
        golden("loop-hold").digest(0, 49),
        "30d70dbf2f6bffd8c8ddbf9d2050b4a7c782607a9b5ae76be8778ffbddce7b4b"
    );
    assert_eq!(
        golden("fac").digest(0, 49),
        "6fd17615b8d637fff89d75ade9182b122d88ef4cb2926fc74410c21731989929"
    );
    assert_eq!(
        golden("shader").digest(0, 25),
        "01fce3039c0b8c25ed30ef6e5f9ebbeb1acc9cf6aeaf73c7efe55d1af7bea67c"
    );
}

#[test]
fn looping_goldens_close_their_loops_exactly() {
    for name in ["loop-hold", "shader"] {
        let clip = golden(name);
        let closure = clip.loop_closure().expect("looping golden");
        for (target, worst) in closure {
            assert_eq!(worst, 0.0, "{name} {target} does not close");
        }
    }
    assert!(golden("fac").loop_closure().is_none());
    assert!(golden("minimal").loop_closure().is_none());
}

// ----------------------------------------------------------------
// Adversarial half.

#[test]
fn swapping_the_tangents_changes_the_answer() {
    // The evaluation-order plant, in miniature: if the
    // evaluator consumed a segment's tangents in the wrong
    // order, document A would produce document B's values.
    // The two documents differ only in which key carries
    // which tangent.
    let a = Clip::from_text(&scalar_hermite_doc(1.0, -1.0)).expect("doc a");
    let b = Clip::from_text(&scalar_hermite_doc(-1.0, 1.0)).expect("doc b");
    let a_mid = evaluate_channel(&a.channels[0], 5);
    let b_mid = evaluate_channel(&b.channels[0], 5);
    assert!((a_mid[0] - 2.5).abs() < 1e-12, "a: {}", a_mid[0]);
    assert!((b_mid[0] + 2.5).abs() < 1e-12, "b: {}", b_mid[0]);
    assert_ne!(a.digest(0, 11), b.digest(0, 11));
}

#[test]
fn hermite_overshoot_is_not_clamped() {
    // The spline's midpoint (2.5) exceeds both endpoint
    // values (0.0): an evaluator that clamped to the segment
    // range would hide exactly the motion the tangents
    // author.
    let clip = Clip::from_text(&scalar_hermite_doc(1.0, -1.0)).expect("probe doc");
    let value = evaluate_channel(&clip.channels[0], 5);
    assert!(value[0] > 0.0, "overshoot was clamped: {}", value[0]);
}

#[test]
fn the_evaluator_refuses_what_the_validator_refuses() {
    for name in ["float-frame", "unresolved-tangent", "wallclock-uniform"] {
        let text = golden_text(&format!("malformed/{name}.json"));
        assert!(
            Clip::from_text(&text).is_err(),
            "malformed {name} was accepted"
        );
    }
}

#[test]
fn frames_clamp_outside_the_key_range() {
    let pose = golden("pose");
    for channel in &pose.channels {
        let only = &channel.keys[0];
        assert_eq!(evaluate_channel(channel, -100), only.value);
        assert_eq!(evaluate_channel(channel, 10_000), only.value);
    }
    let clip = golden("loop-hold");
    let spine = &clip.channels[0];
    assert_eq!(evaluate_channel(spine, -5), spine.keys[0].value);
    assert_eq!(
        evaluate_channel(spine, 999),
        spine.keys[spine.keys.len() - 1].value
    );
}

#[test]
fn frame_at_wraps_the_loop_and_clamps_the_tail() {
    let clip = golden("loop-hold");
    assert_eq!(clip.frame_at(0), 0);
    assert_eq!(clip.frame_at(48), 48);
    assert_eq!(clip.frame_at(49), 0);
    assert_eq!(clip.frame_at(50), 1);
    assert_eq!(clip.frame_at(98), 0);
    let fac = golden("fac");
    assert_eq!(fac.frame_at(1_000), 36);
}
