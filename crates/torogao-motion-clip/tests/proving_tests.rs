//! The spec's own proving tests (SPEC.md section 13),
//! implemented as gates. PT-1..PT-9 map one-to-one onto
//! the section's list; the brief's determinism item is
//! `double_evaluation_is_byte_identical`.

mod common;

use common::{MINIMAL, TestChannels, TestSkeleton, assert_frames_bit_identical, bind, parse};
use torogao_motion_clip::uclip::emit_canonical;
use torogao_motion_clip::{
    Clip, Layer, Scope, frame_map_source_at, frame_map_take_frames_for_source, parse_document,
};

/// PT-7 cross-machine pin: the base replay hash of the
/// PT-1 base clip, pinned after the first primo run. The
/// sandbox run of this suite must reproduce it exactly
/// (SPEC PT-7, section 8.3).
const EXPECTED_PT1_BASE_REPLAY_HASH: &str =
    "5798d6a13647266f0ce8ec8b00c71f31911ee0269048a319d86005fcb9551933";

/// A body+face document: `body_tracks`, `face_tracks`,
/// and an optional `layers` array body.
fn take_doc(body_tracks: &str, face_tracks: &str, layers: Option<&str>, frames: u32) -> String {
    let layers_field = match layers {
        Some(layers) => format!(",\"layers\": {layers}"),
        None => String::new(),
    };
    r##"{
  "format": "torogao-motion-clip/1",
  "id": "take-1",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": %FRAMES%,
  "groups": [
    { "kind": "body", "tracks": [%BODY%] },
    { "kind": "face", "tracks": [%FACE%] }
  ]%LAYERS%,
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "body", "source_class": "authored", "converter": "authored" },
      { "group": "face", "source_class": "authored", "converter": "authored" }
    ]
  }
}"##
    .replace("%BODY%", body_tracks)
    .replace("%FACE%", face_tracks)
    .replace("%LAYERS%", &layers_field)
    .replace("%FRAMES%", &frames.to_string())
}

const PT1_BODY: &str = r##"{ "bone": "Head", "rotation": [
    { "frame": 0, "value": [0.0, 0.0, 0.0, 1.0] },
    { "frame": 5, "value": [0.0, 0.7071067811865476, 0.0, 0.7071067811865476] } ] },
  { "bone": "Hips", "translation": [
    { "frame": 0, "value": [0.0, 1.0, 0.0] },
    { "frame": 5, "value": [0.25, 1.0, 0.0] } ] }"##;

const PT1_FACE: &str = r##"{ "channel": "blink", "keys": [
    { "frame": 0, "value": 0.0 },
    { "frame": 5, "value": 1.0 } ] }"##;

const PT1_LAYER_BOT: &str = r##"{ "id": "pt1-bot", "scope": { "bones": ["Hips"] },
    "tracks": [ { "bone": "Hips", "translation": [ { "frame": 0, "value": [0.125, 0.0, 0.0] } ] } ] }"##;

const PT1_LAYER_MID: &str = r##"{ "id": "pt1-mid", "weight": 0.5, "scope": { "channels": ["blink"] },
    "tracks": [ { "channel": "blink", "keys": [
      { "frame": 0, "value": 0.25 },
      { "frame": 5, "value": -0.25 } ] } ] }"##;

const PT1_LAYER_TOP: &str = r##"{ "id": "pt1-top", "weight": 0.75, "scope": { "bones": ["Head"] },
    "tracks": [ { "bone": "Head", "rotation": [
      { "frame": 0, "value": [0.0, 0.0, 0.0, 1.0] },
      { "frame": 5, "value": [0.0, 0.0, 0.7071067811865476, 0.7071067811865476] } ] } ] }"##;

fn pt1_base() -> Clip {
    parse(&take_doc(PT1_BODY, PT1_FACE, None, 6))
}

fn pt1_full() -> Clip {
    let layers = format!("[ {PT1_LAYER_BOT}, {PT1_LAYER_MID}, {PT1_LAYER_TOP} ]");
    parse(&take_doc(PT1_BODY, PT1_FACE, Some(&layers), 6))
}

/// The base replay hash of a clip: layers removed, then
/// bound and hashed (SPEC PT-1's definition).
fn base_replay_hash(clip: &Clip) -> String {
    let mut stripped = clip.clone();
    stripped.layers.clear();
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&stripped, &skeleton, &channels);
    bound.replay_hash()
}

#[test]
fn pt1_base_replay_hash_is_invariant_across_layer_edits() {
    let base = pt1_base();
    let base_hash = base_replay_hash(&base);
    let full = pt1_full();

    // (a) Add a layer (base -> full, and full stripped).
    assert_eq!(base_replay_hash(&full), base_hash);

    // (b) Delete the top layer.
    let mut deleted_top = full.clone();
    deleted_top.layers.retain(|layer| layer.id != "pt1-top");
    assert_eq!(base_replay_hash(&deleted_top), base_hash);

    // (c) Delete a middle layer.
    let mut deleted_mid = full.clone();
    deleted_mid.layers.retain(|layer| layer.id != "pt1-mid");
    assert_eq!(base_replay_hash(&deleted_mid), base_hash);

    // (d) Change a layer's weight.
    let mut reweighted = full.clone();
    reweighted.layers[1].weight = 0.0;
    assert_eq!(base_replay_hash(&reweighted), base_hash);

    // And the stripped frames themselves are identical.
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let base_frames = bind(&base, &skeleton, &channels).evaluate_all();
    let mut stripped = full.clone();
    stripped.layers.clear();
    let stripped_frames = bind(&stripped, &skeleton, &channels).evaluate_all();
    assert_frames_bit_identical(&base_frames, &stripped_frames);
}

#[test]
fn pt2_zero_weight_layers_reproduce_base_bit_exactly() {
    let base = pt1_base();
    let mut zeroed = pt1_full();
    for layer in &mut zeroed.layers {
        layer.weight = 0.0;
    }
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let base_frames = bind(&base, &skeleton, &channels).evaluate_all();
    let zero_frames = bind(&zeroed, &skeleton, &channels).evaluate_all();
    assert_frames_bit_identical(&base_frames, &zero_frames);
    assert_eq!(
        bind(&zeroed, &skeleton, &channels).replay_hash(),
        bind(&base, &skeleton, &channels).replay_hash()
    );
}

#[test]
fn pt3_layer_scope_isolation_is_measured() {
    // A Head-scoped layer: every other bone and every
    // channel must be bit-identical with and without it,
    // and Head itself must actually move (the layer is
    // not inert).
    let layer = r##"[ { "id": "head-nudge", "scope": { "bones": ["Head"] },
      "tracks": [ { "bone": "Head", "translation": [ { "frame": 0, "value": [0.25, 0.0, 0.0] } ] } ] } ]"##;
    let without = parse(&take_doc(PT1_BODY, PT1_FACE, None, 6));
    let with = parse(&take_doc(PT1_BODY, PT1_FACE, Some(layer), 6));
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let frames_without = bind(&without, &skeleton, &channels).evaluate_all();
    let frames_with = bind(&with, &skeleton, &channels).evaluate_all();
    let mut head_moved = false;
    for (frame_without, frame_with) in frames_without.iter().zip(frames_with.iter()) {
        for ((name, pose_without), (_, pose_with)) in
            frame_without.bones.iter().zip(frame_with.bones.iter())
        {
            if name == "Head" {
                if pose_with.translation[0].to_bits() != pose_without.translation[0].to_bits() {
                    head_moved = true;
                }
            } else {
                assert_eq!(
                    pose_with.translation.map(f64::to_bits),
                    pose_without.translation.map(f64::to_bits),
                    "bone {name} translation changed outside the layer scope"
                );
                assert_eq!(
                    pose_with.rotation.map(f64::to_bits),
                    pose_without.rotation.map(f64::to_bits),
                    "bone {name} rotation changed outside the layer scope"
                );
                assert_eq!(
                    pose_with.scale.map(f64::to_bits),
                    pose_without.scale.map(f64::to_bits),
                    "bone {name} scale changed outside the layer scope"
                );
            }
        }
        for ((name, value_without), (_, value_with)) in frame_without
            .channels
            .iter()
            .zip(frame_with.channels.iter())
        {
            assert_eq!(
                value_with.to_bits(),
                value_without.to_bits(),
                "channel {name} changed outside the layer scope"
            );
        }
    }
    assert!(head_moved, "the Head layer must actually move Head");
}

#[test]
fn pt3_adversarial_scope_escape_is_601() {
    let layer = r##"[ { "id": "escape", "scope": { "bones": ["Head"] },
      "tracks": [ { "bone": "Spine", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0] } ] } ] } ]"##;
    let template = take_doc(PT1_BODY, PT1_FACE, Some(layer), 6);
    let error = parse_document(&common::render(&template)).expect_err("escape must fail");
    assert_eq!(error.code(), "TMC-601");
}

#[test]
fn pt4_thirty_fps_frame_map_round_trip() {
    // A 30 fps source doubled to 60 Hz canonical frames
    // (SPEC section 6.2's worked example shape).
    let template = r##"{
  "format": "torogao-motion-clip/1",
  "id": "take-30fps",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": 8,
  "groups": [
    { "kind": "face", "tracks": [
      { "channel": "blink", "keys": [
        { "frame": 0, "value": 0.0, "interpolation": "stepped" },
        { "frame": 2, "value": 0.25, "interpolation": "stepped" },
        { "frame": 4, "value": 0.5, "interpolation": "stepped" },
        { "frame": 6, "value": 0.75, "interpolation": "stepped" } ] } ] }
  ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "face", "source_class": "converted", "converter": "vmd-import/1",
        "frame_map": { "source_frame_count": 4, "source_rate": { "num": 30, "den": 1 },
                       "entries": [0, 0, 1, 1, 2, 2, 3, 3] } }
    ]
  }
}"##;
    let clip = parse(template);
    let map = clip
        .provenance
        .groups
        .iter()
        .find(|entry| entry.group == torogao_motion_clip::GroupKind::Face)
        .and_then(|entry| entry.frame_map.as_ref())
        .expect("frame map recorded in provenance");
    for frame in 0..8_u32 {
        assert_eq!(
            frame_map_source_at(map, frame),
            Some(map.entries[frame as usize])
        );
    }
    assert_eq!(frame_map_take_frames_for_source(map, 0), vec![0, 1]);
    assert_eq!(frame_map_take_frames_for_source(map, 2), vec![4, 5]);
    assert_eq!(frame_map_take_frames_for_source(map, 3), vec![6, 7]);

    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    let evaluated: Vec<f64> = bound
        .evaluate_all()
        .iter()
        .map(|frame| {
            frame
                .channels
                .iter()
                .find(|(name, _)| name == "blink")
                .map(|(_, value)| *value)
                .expect("blink evaluated")
        })
        .collect();
    assert_eq!(evaluated, vec![0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75]);
}

#[test]
fn pt4_variable_rate_frame_map_round_trip() {
    // A variable-rate source with a skipped source frame
    // (frame 2 has no take frame; SPEC section 6.2 calls
    // such maps valid).
    let template = r##"{
  "format": "torogao-motion-clip/1",
  "id": "take-vfr",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": 6,
  "groups": [
    { "kind": "face", "tracks": [
      { "channel": "blink", "keys": [
        { "frame": 0, "value": 0.0, "interpolation": "stepped" },
        { "frame": 2, "value": 0.25, "interpolation": "stepped" },
        { "frame": 3, "value": 0.5, "interpolation": "stepped" },
        { "frame": 5, "value": 0.75, "interpolation": "stepped" } ] } ] }
  ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "face", "source_class": "converted", "converter": "vmd-import/1",
        "frame_map": { "source_frame_count": 6, "source_rate": "variable",
                       "entries": [0, 0, 1, 3, 3, 4] } }
    ]
  }
}"##;
    let clip = parse(template);
    let map = clip
        .provenance
        .groups
        .iter()
        .find(|entry| entry.group == torogao_motion_clip::GroupKind::Face)
        .and_then(|entry| entry.frame_map.as_ref())
        .expect("frame map recorded in provenance");
    assert_eq!(frame_map_take_frames_for_source(map, 2), Vec::<u32>::new());
    assert_eq!(frame_map_take_frames_for_source(map, 3), vec![3, 4]);

    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    let evaluated: Vec<f64> = bound
        .evaluate_all()
        .iter()
        .map(|frame| {
            frame
                .channels
                .iter()
                .find(|(name, _)| name == "blink")
                .map(|(_, value)| *value)
                .expect("blink evaluated")
        })
        .collect();
    assert_eq!(evaluated, vec![0.0, 0.0, 0.25, 0.5, 0.5, 0.75]);
}

#[test]
fn pt5_golden_fixture_round_trips_byte_identically() {
    let bytes = include_bytes!("fixtures/golden_clip.json");
    let clip = parse_document(bytes).expect("golden fixture parses");
    let emitted = clip.to_canonical_json();
    assert_eq!(
        emitted.as_bytes(),
        bytes,
        "canonical emission must reproduce the fixture byte for byte"
    );
    let reparsed = parse_document(emitted.as_bytes()).expect("emission re-parses");
    assert_eq!(reparsed.to_canonical_json(), emitted);
    // The fixture binds to the test skeleton and evaluates.
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let bound = bind(&clip, &skeleton, &channels);
    let frame0 = bound.evaluate_frame(0).expect("frame 0");
    let blink = frame0
        .channels
        .iter()
        .find(|(name, _)| name == "blink")
        .map(|(_, value)| *value)
        .expect("blink evaluated");
    assert_eq!(blink, 0.125, "base 0 plus the blink layer's 0.125 delta");
}

#[test]
fn pt6_zero_track_layers_are_inert() {
    let plain = parse(MINIMAL);
    let mut reserved = plain.clone();
    reserved.layers.push(Layer {
        id: "reserved".to_string(),
        name: Some("held for a later pass".to_string()),
        weight: 1.0,
        scope: Scope::default(),
        tracks: Vec::new(),
    });
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let plain_frames = bind(&plain, &skeleton, &channels).evaluate_all();
    let reserved_frames = bind(&reserved, &skeleton, &channels).evaluate_all();
    assert_frames_bit_identical(&plain_frames, &reserved_frames);
    assert_eq!(
        bind(&plain, &skeleton, &channels).replay_hash(),
        bind(&reserved, &skeleton, &channels).replay_hash()
    );
}

#[test]
fn pt7_replay_hash_is_stable_across_machines() {
    let hash = base_replay_hash(&pt1_base());
    assert_eq!(
        hash, EXPECTED_PT1_BASE_REPLAY_HASH,
        "PT-7: the base replay hash must match the pinned cross-machine value"
    );
}

#[test]
fn pt9_middle_layer_deletion_is_byte_identical() {
    // Author base + [bot, top] directly; delete the
    // middle layer from the full stack by surgery: the
    // canonical emissions must agree byte for byte
    // (SPEC PT-9), and so must the evaluated frames.
    let direct_layers = format!("[ {PT1_LAYER_BOT}, {PT1_LAYER_TOP} ]");
    let direct = parse(&take_doc(PT1_BODY, PT1_FACE, Some(&direct_layers), 6));
    let mut deleted = pt1_full();
    deleted.layers.retain(|layer| layer.id != "pt1-mid");
    assert_eq!(
        emit_canonical(&deleted.to_uclip(), None),
        emit_canonical(&direct.to_uclip(), None)
    );
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let deleted_frames = bind(&deleted, &skeleton, &channels).evaluate_all();
    let direct_frames = bind(&direct, &skeleton, &channels).evaluate_all();
    assert_frames_bit_identical(&deleted_frames, &direct_frames);
}

#[test]
fn double_evaluation_is_byte_identical() {
    let full = pt1_full();
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    let first = bind(&full, &skeleton, &channels);
    let second = bind(&full, &skeleton, &channels);
    assert_frames_bit_identical(&first.evaluate_all(), &second.evaluate_all());
    assert_eq!(first.replay_hash(), second.replay_hash());
    // Re-parsing the same bytes yields the same hash too.
    let reparsed = pt1_full();
    assert_eq!(
        bind(&reparsed, &skeleton, &channels).replay_hash(),
        first.replay_hash()
    );
}

/// PT-8: the spec's named adversarial cases, each with
/// its section 10.1 code. (The full adversarial suite
/// lives in adversarial_tests.rs; these are the ten-plus
/// cases section 13 names.)
mod pt8 {
    use super::common::{MINIMAL, parse_code};
    use super::take_doc;
    use torogao_motion_clip::parse_document;

    #[test]
    fn byte_order_mark_is_102() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(&super::common::render(MINIMAL));
        let error = parse_document(&bytes).expect_err("BOM must fail");
        assert_eq!(error.code(), "TMC-102");
    }

    #[test]
    fn format_version_two_is_106() {
        let template = MINIMAL.replace("torogao-motion-clip/1", "torogao-motion-clip/2");
        assert_eq!(parse_code(&template), "TMC-106");
    }

    #[test]
    fn unknown_field_is_104() {
        let template = MINIMAL.replace("\"frames\": 8,", "\"frames\": 8, \"zzz\": 0,");
        assert_eq!(parse_code(&template), "TMC-104");
    }

    #[test]
    fn duplicate_layer_id_is_202() {
        let layers = r##"[ { "id": "x", "tracks": [] }, { "id": "x", "tracks": [] } ]"##;
        let template = take_doc("", "", Some(layers), 4);
        assert_eq!(parse_code(&template), "TMC-202");
    }

    #[test]
    fn weight_out_of_range_is_302() {
        let layers = r##"[ { "id": "x", "weight": 1.5, "tracks": [] } ]"##;
        let template = take_doc("", "", Some(layers), 4);
        assert_eq!(parse_code(&template), "TMC-302");
    }

    #[test]
    fn non_unit_delta_quaternion_is_303() {
        let layers = r##"[ { "id": "x", "scope": { "bones": ["Head"] },
          "tracks": [ { "bone": "Head", "rotation": [ { "frame": 0, "value": [0.0, 0.0, 0.0, 0.0] } ] } ] } ]"##;
        let template = take_doc("", "", Some(layers), 4);
        assert_eq!(parse_code(&template), "TMC-303");
    }

    #[test]
    fn layer_scale_factor_zero_is_304() {
        let layers = r##"[ { "id": "x", "scope": { "bones": ["Hips"] },
          "tracks": [ { "bone": "Hips", "scale": [ { "frame": 0, "value": [0.0, 1.0, 1.0] } ] } ] } ]"##;
        let template = take_doc("", "", Some(layers), 4);
        assert_eq!(parse_code(&template), "TMC-304");
    }

    #[test]
    fn key_frame_at_frames_is_501() {
        let body =
            r##"{ "bone": "Hips", "translation": [ { "frame": 4, "value": [0.0, 0.0, 0.0] } ] }"##;
        let template = take_doc(body, "", None, 4);
        assert_eq!(parse_code(&template), "TMC-501");
    }

    #[test]
    fn unknown_interpolation_is_502() {
        let face = r##"{ "channel": "blink", "keys": [ { "frame": 0, "value": 0.0, "interpolation": "cubic" } ] }"##;
        let template = take_doc("", face, None, 4);
        assert_eq!(parse_code(&template), "TMC-502");
    }

    #[test]
    fn track_outside_scope_is_601() {
        let layers = r##"[ { "id": "x", "scope": { "bones": ["Head"] },
          "tracks": [ { "bone": "Spine", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0] } ] } ] } ]"##;
        let template = take_doc("", "", Some(layers), 4);
        assert_eq!(parse_code(&template), "TMC-601");
    }

    #[test]
    fn frame_map_count_mismatch_is_702() {
        let template = r##"{
  "format": "torogao-motion-clip/1",
  "id": "take-1",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": 4,
  "groups": [ { "kind": "body", "tracks": [] } ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "body", "source_class": "converted", "converter": "vmd-import/1",
        "frame_map": { "source_frame_count": 2, "source_rate": { "num": 30, "den": 1 },
                       "entries": [0, 0, 1] } }
    ]
  }
}"##;
        assert_eq!(parse_code(template), "TMC-702");
    }
}
