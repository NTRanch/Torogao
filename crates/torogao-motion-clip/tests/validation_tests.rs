//! Validation-half tests: valid documents parse, defaults
//! resolve, and evaluation reproduces hand-computed
//! expected samples. Expectations are computed in the test
//! from the spec's formulas with literal arithmetic — never
//! by calling the code under test twice.

mod common;

use common::{MINIMAL, TestChannels, TestSkeleton, bind, parse};
use torogao_motion_clip::uclip::emit_canonical;
use torogao_motion_clip::{EvalError, Group, Interpolation, parse_document, sha256_hex};

/// A document with a body group carrying `tracks`, an
/// optional `layers` array body, and authored provenance.
fn body_doc(tracks: &str, layers: Option<&str>, frames: u32) -> String {
    let layers_field = match layers {
        Some(layers) => format!(",\"layers\": {layers}"),
        None => String::new(),
    };
    r##"{
  "format": "torogao-motion-clip/1",
  "id": "test-clip",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": %FRAMES%,
  "groups": [
    { "kind": "body", "tracks": [%TRACKS%] }
  ]%LAYERS%,
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "body", "source_class": "authored", "converter": "authored" }
    ]
  }
}"##
    .replace("%TRACKS%", tracks)
    .replace("%LAYERS%", &layers_field)
    .replace("%FRAMES%", &frames.to_string())
}

/// A document with a face group carrying `tracks` and
/// authored provenance.
fn face_doc(tracks: &str, layers: Option<&str>, frames: u32) -> String {
    let layers_field = match layers {
        Some(layers) => format!(",\"layers\": {layers}"),
        None => String::new(),
    };
    r##"{
  "format": "torogao-motion-clip/1",
  "id": "test-clip",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": %FRAMES%,
  "groups": [
    { "kind": "face", "tracks": [%TRACKS%] }
  ]%LAYERS%,
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "face", "source_class": "authored", "converter": "authored" }
    ]
  }
}"##
    .replace("%TRACKS%", tracks)
    .replace("%LAYERS%", &layers_field)
    .replace("%FRAMES%", &frames.to_string())
}

fn channel_value(bound: &torogao_motion_clip::BoundClip<'_>, frame: u32, channel: &str) -> f64 {
    let evaluated = bound.evaluate_frame(frame).expect("frame in range");
    evaluated
        .channels
        .iter()
        .find(|(name, _)| name == channel)
        .map(|(_, value)| *value)
        .expect("channel in evaluated frame")
}

fn bone_pose(
    bound: &torogao_motion_clip::BoundClip<'_>,
    frame: u32,
    bone: &str,
) -> torogao_motion_clip::BonePose {
    let evaluated = bound.evaluate_frame(frame).expect("frame in range");
    evaluated
        .bones
        .iter()
        .find(|(name, _)| name == bone)
        .map(|(_, pose)| *pose)
        .expect("bone in evaluated frame")
}

#[test]
fn minimal_document_parses_with_defaults_resolved() {
    let clip = parse(MINIMAL);
    assert_eq!(clip.id, "test-clip");
    assert_eq!(clip.frames, 8);
    assert!(!clip.loop_, "loop defaults to false");
    assert!(clip.layers.is_empty(), "layers default to an empty stack");
    assert_eq!(clip.groups.len(), 1);
    assert!(matches!(clip.groups[0], Group::Body { .. }));
}

#[test]
fn layer_weight_and_interpolation_defaults_resolve() {
    // Layer without `weight` and keys without
    // `interpolation`: weight 1, linear.
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [ { "frame": 0, "value": 0.0 } ] }"##,
        Some(
            r##"[ { "id": "l1", "scope": { "channels": ["blink"] },
              "tracks": [ { "channel": "blink", "keys": [ { "frame": 0, "value": 0.5 } ] } ] } ]"##,
        ),
        4,
    );
    let clip = parse(&template);
    assert_eq!(clip.layers[0].weight, 1.0);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    // Base 0 + 1 * 0.5 = 0.5.
    assert_eq!(channel_value(&bound, 0, "blink"), 0.5);
}

#[test]
fn scalar_linear_interpolation_matches_hand_computation() {
    // Keys (0, 0.0) -> (4, 1.0), linear: u = f/4 exactly.
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [
            { "frame": 0, "value": 0.0 },
            { "frame": 4, "value": 1.0 } ] }"##,
        None,
        8,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    assert_eq!(channel_value(&bound, 0, "blink"), 0.0);
    assert_eq!(channel_value(&bound, 1, "blink"), 0.25);
    assert_eq!(channel_value(&bound, 2, "blink"), 0.5);
    assert_eq!(channel_value(&bound, 3, "blink"), 0.75);
    assert_eq!(channel_value(&bound, 4, "blink"), 1.0);
}

#[test]
fn scalar_stepped_interpolation_holds() {
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [
            { "frame": 0, "value": 0.5, "interpolation": "stepped" },
            { "frame": 4, "value": 1.0 } ] }"##,
        None,
        8,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    assert_eq!(channel_value(&bound, 3, "blink"), 0.5);
    assert_eq!(channel_value(&bound, 4, "blink"), 1.0);
}

#[test]
fn keys_hold_outside_their_range() {
    // First key at frame 2, last at frame 4: the track
    // holds the endpoint values outside (SPEC §4.1).
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [
            { "frame": 2, "value": 0.25 },
            { "frame": 4, "value": 0.75 } ] }"##,
        None,
        8,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    assert_eq!(channel_value(&bound, 0, "blink"), 0.25);
    assert_eq!(channel_value(&bound, 3, "blink"), 0.5);
    assert_eq!(channel_value(&bound, 7, "blink"), 0.75);
}

#[test]
fn vector_linear_interpolation_matches_hand_computation() {
    let template = body_doc(
        r##"{ "bone": "Hips", "translation": [
            { "frame": 0, "value": [0.0, 0.0, 0.0] },
            { "frame": 4, "value": [1.0, 2.0, 4.0] } ] }"##,
        None,
        8,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    let pose = bone_pose(&bound, 2, "Hips");
    assert_eq!(pose.translation, [0.5, 1.0, 2.0]);
}

#[test]
fn quat_nlerp_matches_hand_computation() {
    // Identity -> 180 degrees about Y. At u = 0.5 the
    // nlerp is (0, sqrt(1/2), 0, sqrt(1/2)) (SPEC §7.1).
    let template = body_doc(
        r##"{ "bone": "Head", "rotation": [
            { "frame": 0, "value": [0.0, 0.0, 0.0, 1.0] },
            { "frame": 4, "value": [0.0, 1.0, 0.0, 0.0] } ] }"##,
        None,
        8,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    let pose = bone_pose(&bound, 2, "Head");
    let half_root_two = (0.5_f64).sqrt();
    let expected = [0.0, half_root_two, 0.0, half_root_two];
    for (got, want) in pose.rotation.iter().zip(expected.iter()) {
        assert!(
            (got - want).abs() < 1e-12,
            "rotation {pose:?} vs {expected:?}"
        );
    }
}

#[test]
fn untracked_bone_holds_rest_transform() {
    let template = body_doc(
        r##"{ "bone": "Hips", "translation": [ { "frame": 0, "value": [0.0, 0.5, 0.0] } ] }"##,
        None,
        4,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    let spine = bone_pose(&bound, 2, "Spine");
    assert_eq!(spine.translation, [0.0, 0.125, 0.0]);
    assert_eq!(spine.rotation, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(spine.scale, [1.0, 1.0, 1.0]);
}

#[test]
fn scalar_layer_composition_matches_hand_computation() {
    // Base constant 0.25; layer weight 0.5, delta constant
    // 0.5: 0.25 + 0.5 * 0.5 = 0.5 exactly (SPEC §7.2).
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [ { "frame": 0, "value": 0.25 } ] }"##,
        Some(
            r##"[ { "id": "l1", "weight": 0.5, "scope": { "channels": ["blink"] },
              "tracks": [ { "channel": "blink", "keys": [ { "frame": 0, "value": 0.5 } ] } ] } ]"##,
        ),
        4,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    assert_eq!(channel_value(&bound, 0, "blink"), 0.5);
}

#[test]
fn scope_only_channel_has_base_zero() {
    // jawOpen appears only in a layer scope: base value 0
    // (SPEC §7.1), so the composed value is 1 * 0.5.
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [ { "frame": 0, "value": 0.0 } ] }"##,
        Some(
            r##"[ { "id": "l1", "scope": { "channels": ["jawOpen"] },
              "tracks": [ { "channel": "jawOpen", "keys": [ { "frame": 0, "value": 0.5 } ] } ] } ]"##,
        ),
        4,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    assert_eq!(channel_value(&bound, 0, "jawOpen"), 0.5);
}

#[test]
fn scale_layer_composition_matches_hand_computation() {
    // Base scale 2; delta factor 1.5 at weight 0.5:
    // factor 1 + 0.5 * (1.5 - 1) = 1.25; 2 * 1.25 = 2.5.
    let template = body_doc(
        r##"{ "bone": "Hips", "scale": [ { "frame": 0, "value": [2.0, 2.0, 2.0] } ] }"##,
        Some(
            r##"[ { "id": "l1", "weight": 0.5, "scope": { "bones": ["Hips"] },
              "tracks": [ { "bone": "Hips", "scale": [ { "frame": 0, "value": [1.5, 1.5, 1.5] } ] } ] } ]"##,
        ),
        4,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    let pose = bone_pose(&bound, 0, "Hips");
    assert_eq!(pose.scale, [2.5, 2.5, 2.5]);
}

#[test]
fn translation_layer_composition_matches_hand_computation() {
    let template = body_doc(
        r##"{ "bone": "Hips", "translation": [ { "frame": 0, "value": [0.0, 1.0, 0.0] } ] }"##,
        Some(
            r##"[ { "id": "l1", "scope": { "bones": ["Hips"] },
              "tracks": [ { "bone": "Hips", "translation": [ { "frame": 0, "value": [0.25, 0.0, 0.0] } ] } ] } ]"##,
        ),
        4,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    let pose = bone_pose(&bound, 0, "Hips");
    assert_eq!(pose.translation, [0.25, 1.0, 0.0]);
}

#[test]
fn rotation_layer_composes_in_local_frame() {
    // Base identity; delta 90 degrees about Z at weight 1:
    // the composed rotation is the delta itself (§7.2).
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let layers = r##"[ { "id": "l1", "scope": { "bones": ["Head"] },
      "tracks": [ { "bone": "Head", "rotation": [ { "frame": 0, "value": [0.0, 0.0, %HALF%, %HALF%] } ] } ] } ]"##
        .replace("%HALF%", &half.to_string());
    let template = body_doc(
        r##"{ "bone": "Head", "rotation": [ { "frame": 0, "value": [0.0, 0.0, 0.0, 1.0] } ] }"##,
        Some(&layers),
        4,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    let pose = bone_pose(&bound, 0, "Head");
    for (got, want) in pose.rotation.iter().zip([0.0, 0.0, half, half].iter()) {
        assert!(
            (got - want).abs() < 1e-12,
            "rotation {:?} vs delta",
            pose.rotation
        );
    }
}

#[test]
fn composed_channel_value_clamps_to_declared_range() {
    // smile is declared [-1, 1]. Base 0.75 + delta 0.5 at
    // weight 1 composes to 1.25 and clamps to 1.0 — the
    // one sanctioned clamp (SPEC §10.3).
    let template = face_doc(
        r##"{ "channel": "smile", "keys": [ { "frame": 0, "value": 0.75 } ] }"##,
        Some(
            r##"[ { "id": "l1", "scope": { "channels": ["smile"] },
              "tracks": [ { "channel": "smile", "keys": [ { "frame": 0, "value": 0.5 } ] } ] } ]"##,
        ),
        4,
    );
    let clip = parse(&template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    assert_eq!(channel_value(&bound, 0, "smile"), 1.0);
}

#[test]
fn all_four_group_kinds_combine_in_one_take() {
    // Body and face carry tracks; camera and hands are
    // declarations; provenance covers all four, each with
    // its own source class (SPEC §§2.3, 6.1).
    let template = r##"{
  "format": "torogao-motion-clip/1",
  "id": "test-clip",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": 4,
  "groups": [
    { "kind": "body", "tracks": [
      { "bone": "Hips", "translation": [ { "frame": 0, "value": [0.0, 1.0, 0.0] } ] } ] },
    { "kind": "camera", "mode": "static" },
    { "kind": "face", "tracks": [
      { "channel": "blink", "keys": [ { "frame": 0, "value": 0.0 } ] } ] },
    { "kind": "hands" }
  ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "body", "source_class": "converted", "converter": "bvh-import/1" },
      { "group": "camera", "source_class": "authored", "converter": "authored" },
      { "group": "face", "source_class": "generated", "converter": "lumen-face/1" },
      { "group": "hands", "source_class": "authored", "converter": "authored" }
    ]
  }
}"##;
    let clip = parse(template);
    assert_eq!(clip.groups.len(), 4);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    assert_eq!(channel_value(&bound, 0, "blink"), 0.0);
}

#[test]
fn empty_layers_array_is_an_empty_stack() {
    let with_empty = body_doc("", Some("[]"), 4);
    let clip = parse(&with_empty);
    assert!(clip.layers.is_empty());
    assert!(!clip.to_canonical_json().contains("layers"));
}

#[test]
fn evaluate_frame_out_of_range_is_a_typed_error() {
    let clip = parse(MINIMAL);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    assert_eq!(
        bound.evaluate_frame(8),
        Err(EvalError::FrameOutOfRange {
            frame: 8,
            frames: 8
        })
    );
}

#[test]
fn self_hash_round_trip_verifies() {
    // Take a valid clip, stamp it with the self-hash of
    // its emptied canonical form (SPEC §3.4), and confirm
    // the stamped document parses; a tampered copy fails
    // with TMC-705 (asserted in the adversarial suite).
    let clip = parse(MINIMAL);
    let emptied = emit_canonical(&clip.to_uclip(), Some(""));
    let mut stamped = clip.clone();
    stamped.sha256 = Some(sha256_hex(emptied.as_bytes()));
    let text = stamped.to_canonical_json();
    let reparsed = parse_document(text.as_bytes()).expect("stamped clip verifies");
    assert_eq!(reparsed.sha256, stamped.sha256);
}

#[test]
fn interpolation_token_resolves_in_model() {
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [
            { "frame": 0, "value": 0.0, "interpolation": "stepped" } ] }"##,
        None,
        4,
    );
    let clip = parse(&template);
    let face = clip.face_tracks().expect("face group");
    assert_eq!(face[0].keys[0].interpolation, Interpolation::Stepped);
}
