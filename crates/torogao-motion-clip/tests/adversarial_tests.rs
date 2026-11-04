//! Adversarial-half tests: every case must fail, with
//! exactly the error code SPEC.md section 10.1 names for
//! the condition. A panic anywhere is a gate failure, so
//! these call the public API and match on the typed error.

mod common;

use common::{MINIMAL, TestChannels, TestSkeleton, parse, parse_code, render};
use torogao_motion_clip::{BoundClip, parse_document};

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

/// Parse (must succeed) then bind (must fail); return the
/// binding error's code.
fn bind_code(template: &str) -> &'static str {
    let clip = parse(template);
    let skeleton = TestSkeleton::new();
    let channels = TestChannels::new();
    match BoundClip::bind(&clip, &skeleton, &channels) {
        Ok(_) => panic!("template must fail binding"),
        Err(error) => error.code(),
    }
}

#[test]
fn unknown_top_level_field_is_104() {
    let template = MINIMAL.replace("\"frames\": 8,", "\"frames\": 8, \"bogus\": 1,");
    assert_eq!(parse_code(&template), "TMC-104");
}

#[test]
fn unknown_field_inside_layer_is_104() {
    let template = body_doc(
        "",
        Some(r##"[ { "id": "l1", "bogus": true, "tracks": [] } ]"##),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-104");
}

#[test]
fn layer_field_naming_a_base_track_is_104() {
    // The empty-by-construction tripwire (SPEC §5.4): a
    // layer has no field that could reference base data.
    let template = body_doc(
        "",
        Some(r##"[ { "id": "l1", "base_track": "Hips", "tracks": [] } ]"##),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-104");
}

#[test]
fn unknown_field_inside_key_is_104() {
    let template = body_doc(
        r##"{ "bone": "Hips", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0], "easing": "smooth" } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-104");
}

#[test]
fn format_version_two_is_106() {
    let template = MINIMAL.replace("torogao-motion-clip/1", "torogao-motion-clip/2");
    assert_eq!(parse_code(&template), "TMC-106");
}

#[test]
fn format_other_token_is_105() {
    let template = MINIMAL.replace("torogao-motion-clip/1", "lumen-rig-motion/1");
    assert_eq!(parse_code(&template), "TMC-105");
}

#[test]
fn format_missing_is_105() {
    let template = MINIMAL.replace("  \"format\": \"torogao-motion-clip/1\",\n", "");
    assert_eq!(parse_code(&template), "TMC-105");
}

#[test]
fn duplicate_key_is_103() {
    let template = MINIMAL.replace("\"frames\": 8,", "\"frames\": 8, \"frames\": 8,");
    assert_eq!(parse_code(&template), "TMC-103");
}

#[test]
fn byte_order_mark_is_102() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(&render(MINIMAL));
    let error = parse_document(&bytes).expect_err("BOM must fail");
    assert_eq!(error.code(), "TMC-102");
}

#[test]
fn malformed_json_is_102() {
    let error = parse_document(b"{ not json").expect_err("garbage must fail");
    assert_eq!(error.code(), "TMC-102");
}

#[test]
fn oversize_input_is_101() {
    let bytes = vec![b' '; torogao_motion_clip::limits::CLIP_BYTES_MAX + 1];
    let error = parse_document(&bytes).expect_err("oversize must fail");
    assert_eq!(error.code(), "TMC-101");
}

#[test]
fn non_unit_base_quaternion_is_303() {
    let template = body_doc(
        r##"{ "bone": "Head", "rotation": [ { "frame": 0, "value": [0.0, 0.0, 0.0, 2.0] } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-303");
}

#[test]
fn non_unit_delta_quaternion_is_303() {
    let template = body_doc(
        "",
        Some(
            r##"[ { "id": "l1", "scope": { "bones": ["Head"] },
              "tracks": [ { "bone": "Head", "rotation": [ { "frame": 0, "value": [0.0, 0.0, 0.0, 0.5] } ] } ] } ]"##,
        ),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-303");
}

#[test]
fn weight_above_one_is_302() {
    let template = body_doc(
        "",
        Some(r##"[ { "id": "l1", "weight": 1.5, "tracks": [] } ]"##),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-302");
}

#[test]
fn weight_below_zero_is_302() {
    let template = body_doc(
        "",
        Some(r##"[ { "id": "l1", "weight": -0.25, "tracks": [] } ]"##),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-302");
}

#[test]
fn duplicate_layer_id_is_202() {
    let template = body_doc(
        "",
        Some(r##"[ { "id": "l1", "tracks": [] }, { "id": "l1", "tracks": [] } ]"##),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-202");
}

#[test]
fn duplicate_group_kind_is_202() {
    let template = r##"{
  "format": "torogao-motion-clip/1",
  "id": "test-clip",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": 4,
  "groups": [
    { "kind": "body", "tracks": [] },
    { "kind": "body", "tracks": [] }
  ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "body", "source_class": "authored", "converter": "authored" }
    ]
  }
}"##;
    assert_eq!(parse_code(template), "TMC-202");
}

#[test]
fn duplicate_track_target_in_group_is_202() {
    let template = body_doc(
        r##"{ "bone": "Hips", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0] } ] },
            { "bone": "Hips", "translation": [ { "frame": 0, "value": [0.0, 1.0, 0.0] } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-202");
}

#[test]
fn duplicate_scope_entry_is_202() {
    let template = body_doc(
        "",
        Some(r##"[ { "id": "l1", "scope": { "bones": ["Head", "Head"] }, "tracks": [] } ]"##),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-202");
}

#[test]
fn hands_group_with_tracks_is_108() {
    let template = r##"{
  "format": "torogao-motion-clip/1",
  "id": "test-clip",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": 4,
  "groups": [ { "kind": "hands", "tracks": [] } ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "hands", "source_class": "authored", "converter": "authored" }
    ]
  }
}"##;
    assert_eq!(parse_code(template), "TMC-108");
}

#[test]
fn camera_group_with_tracks_is_108() {
    let template = r##"{
  "format": "torogao-motion-clip/1",
  "id": "test-clip",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": 4,
  "groups": [ { "kind": "camera", "mode": "static", "tracks": [] } ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "camera", "source_class": "authored", "converter": "authored" }
    ]
  }
}"##;
    assert_eq!(parse_code(template), "TMC-108");
}

/// A body clip whose provenance carries `frame_map`, for
/// the frame-map cases.
fn frame_map_doc(frame_map: &str, frames: u32) -> String {
    r##"{
  "format": "torogao-motion-clip/1",
  "id": "test-clip",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": %FRAMES%,
  "groups": [ { "kind": "body", "tracks": [] } ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "body", "source_class": "converted", "converter": "vmd-import/1",
        "frame_map": %MAP% }
    ]
  }
}"##
    .replace("%MAP%", frame_map)
    .replace("%FRAMES%", &frames.to_string())
}

#[test]
fn frame_map_decreasing_is_702() {
    let template = frame_map_doc(
        r##"{ "source_frame_count": 4, "source_rate": { "num": 30, "den": 1 },
             "entries": [0, 0, 1, 1, 2, 2, 1, 1] }"##,
        8,
    );
    assert_eq!(parse_code(&template), "TMC-702");
}

#[test]
fn frame_map_wrong_length_is_702() {
    let template = frame_map_doc(
        r##"{ "source_frame_count": 4, "source_rate": { "num": 30, "den": 1 },
             "entries": [0, 0, 1, 1, 2, 2, 3] }"##,
        8,
    );
    assert_eq!(parse_code(&template), "TMC-702");
}

#[test]
fn frame_map_entry_beyond_source_is_702() {
    let template = frame_map_doc(
        r##"{ "source_frame_count": 4, "source_rate": { "num": 30, "den": 1 },
             "entries": [0, 0, 1, 1, 2, 2, 3, 4] }"##,
        8,
    );
    assert_eq!(parse_code(&template), "TMC-702");
}

#[test]
fn frame_map_zero_denominator_is_702() {
    let template = frame_map_doc(
        r##"{ "source_frame_count": 4, "source_rate": { "num": 30, "den": 0 },
             "entries": [0, 0, 1, 1, 2, 2, 3, 3] }"##,
        8,
    );
    assert_eq!(parse_code(&template), "TMC-702");
}

#[test]
fn key_frame_equal_to_frames_is_501() {
    let template = body_doc(
        r##"{ "bone": "Hips", "translation": [ { "frame": 8, "value": [0.0, 0.0, 0.0] } ] }"##,
        None,
        8,
    );
    assert_eq!(parse_code(&template), "TMC-501");
}

#[test]
fn key_frames_decreasing_is_501() {
    let template = body_doc(
        r##"{ "bone": "Hips", "translation": [
            { "frame": 3, "value": [0.0, 0.0, 0.0] },
            { "frame": 2, "value": [0.0, 1.0, 0.0] } ] }"##,
        None,
        8,
    );
    assert_eq!(parse_code(&template), "TMC-501");
}

#[test]
fn layer_scale_factor_zero_is_304() {
    let template = body_doc(
        "",
        Some(
            r##"[ { "id": "l1", "scope": { "bones": ["Hips"] },
              "tracks": [ { "bone": "Hips", "scale": [ { "frame": 0, "value": [0.0, 1.0, 1.0] } ] } ] } ]"##,
        ),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-304");
}

#[test]
fn layer_scale_factor_above_max_is_304() {
    let template = body_doc(
        "",
        Some(
            r##"[ { "id": "l1", "scope": { "bones": ["Hips"] },
              "tracks": [ { "bone": "Hips", "scale": [ { "frame": 0, "value": [1000.5, 1.0, 1.0] } ] } ] } ]"##,
        ),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-304");
}

#[test]
fn base_scale_zero_is_304() {
    let template = body_doc(
        r##"{ "bone": "Hips", "scale": [ { "frame": 0, "value": [1.0, 0.0, 1.0] } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-304");
}

#[test]
fn skeleton_hash_mismatch_at_binding_is_204() {
    // A well-formed hash that is not the def's hash.
    let template = MINIMAL.replace("%SHA%", &"b".repeat(64));
    assert_eq!(bind_code(&template), "TMC-204");
}

#[test]
fn unknown_bone_at_binding_is_203() {
    let template = body_doc(
        r##"{ "bone": "Toe", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0] } ] }"##,
        None,
        4,
    );
    assert_eq!(bind_code(&template), "TMC-203");
}

#[test]
fn unknown_channel_at_binding_is_203() {
    let template = face_doc(
        r##"{ "channel": "noseWrinkle", "keys": [ { "frame": 0, "value": 0.0 } ] }"##,
        None,
        4,
    );
    assert_eq!(bind_code(&template), "TMC-203");
}

#[test]
fn scope_unknown_bone_at_binding_is_602() {
    let template = body_doc(
        "",
        Some(r##"[ { "id": "l1", "scope": { "bones": ["Toe"] }, "tracks": [] } ]"##),
        4,
    );
    assert_eq!(bind_code(&template), "TMC-602");
}

#[test]
fn scope_unknown_channel_at_binding_is_602() {
    let template = body_doc(
        "",
        Some(r##"[ { "id": "l1", "scope": { "channels": ["noseWrinkle"] }, "tracks": [] } ]"##),
        4,
    );
    assert_eq!(bind_code(&template), "TMC-602");
}

#[test]
fn layer_track_outside_scope_is_601() {
    let template = body_doc(
        "",
        Some(
            r##"[ { "id": "l1", "scope": { "bones": ["Head"] },
              "tracks": [ { "bone": "Spine", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0] } ] } ] } ]"##,
        ),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-601");
}

#[test]
fn duplicate_target_within_layer_is_603() {
    let template = body_doc(
        "",
        Some(
            r##"[ { "id": "l1", "scope": { "bones": ["Head"] },
              "tracks": [
                { "bone": "Head", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0] } ] },
                { "bone": "Head", "rotation": [ { "frame": 0, "value": [0.0, 0.0, 0.0, 1.0] } ] } ] } ]"##,
        ),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-603");
}

#[test]
fn unknown_source_class_is_701() {
    let template = MINIMAL.replace("authored", "teleported");
    // Both `source_class` and the authored `converter`
    // change; the source-class token check fires.
    assert_eq!(parse_code(&template), "TMC-701");
}

#[test]
fn provenance_missing_entry_is_703() {
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [ { "frame": 0, "value": 0.0 } ] }"##,
        None,
        4,
    )
    .replace(
        "{ \"group\": \"face\", \"source_class\": \"authored\", \"converter\": \"authored\" }",
        "",
    );
    assert_eq!(parse_code(&template), "TMC-703");
}

#[test]
fn provenance_extra_entry_is_703() {
    let template = MINIMAL.replace(
        "{ \"group\": \"body\", \"source_class\": \"authored\", \"converter\": \"authored\" }",
        "{ \"group\": \"body\", \"source_class\": \"authored\", \"converter\": \"authored\" },\n      { \"group\": \"face\", \"source_class\": \"authored\", \"converter\": \"authored\" }",
    );
    assert_eq!(parse_code(&template), "TMC-703");
}

#[test]
fn malformed_model_hash_is_704() {
    let template = MINIMAL.replace(
        "\"converter\": \"authored\" }",
        "\"converter\": \"authored\", \"model_sha256\": \"xyz\" }",
    );
    assert_eq!(parse_code(&template), "TMC-704");
}

#[test]
fn malformed_skeleton_hash_is_704() {
    let template = MINIMAL.replace("%SHA%", "abc");
    assert_eq!(parse_code(&template), "TMC-704");
}

#[test]
fn self_hash_mismatch_is_705() {
    use torogao_motion_clip::sha256_hex;
    use torogao_motion_clip::uclip::emit_canonical;
    let clip = parse(MINIMAL);
    let emptied = emit_canonical(&clip.to_uclip(), Some(""));
    let mut stamped = clip.clone();
    stamped.sha256 = Some(sha256_hex(emptied.as_bytes()));
    let text = stamped.to_canonical_json();
    let tampered = text.replace("\"frames\": 8", "\"frames\": 9");
    let error = parse_document(tampered.as_bytes()).expect_err("tampered must fail");
    assert_eq!(error.code(), "TMC-705");
}

#[test]
fn frames_zero_is_306() {
    let template = MINIMAL.replace("\"frames\": 8", "\"frames\": 0");
    assert_eq!(parse_code(&template), "TMC-306");
}

#[test]
fn frames_above_max_is_306() {
    let template = MINIMAL.replace("\"frames\": 8", "\"frames\": 36001");
    assert_eq!(parse_code(&template), "TMC-306");
}

#[test]
fn negative_frames_is_103() {
    let template = MINIMAL.replace("\"frames\": 8", "\"frames\": -1");
    assert_eq!(parse_code(&template), "TMC-103");
}

#[test]
fn fractional_frames_is_103() {
    let template = MINIMAL.replace("\"frames\": 8", "\"frames\": 8.5");
    assert_eq!(parse_code(&template), "TMC-103");
}

#[test]
fn translation_magnitude_too_large_is_305() {
    let template = body_doc(
        r##"{ "bone": "Hips", "translation": [ { "frame": 0, "value": [1000001.0, 0.0, 0.0] } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-305");
}

#[test]
fn scalar_magnitude_too_large_is_305() {
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [ { "frame": 0, "value": 1000001.0 } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-305");
}

#[test]
fn scalar_delta_magnitude_too_large_is_305() {
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [ { "frame": 0, "value": 0.0 } ] }"##,
        Some(
            r##"[ { "id": "l1", "scope": { "channels": ["blink"] },
              "tracks": [ { "channel": "blink", "keys": [ { "frame": 0, "value": -1000001.0 } ] } ] } ]"##,
        ),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-305");
}

#[test]
fn float_overflow_literal_is_301() {
    // 1e999 overflows f64 in parsing (SPEC §1.1, TMC-301).
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [ { "frame": 0, "value": 1e999 } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-301");
}

#[test]
fn cubic_interpolation_is_502() {
    let template = face_doc(
        r##"{ "channel": "blink", "keys": [ { "frame": 0, "value": 0.0, "interpolation": "cubic" } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-502");
}

#[test]
fn invalid_clip_id_is_201() {
    let template = MINIMAL.replace("\"test-clip\"", "\"bad id\"");
    assert_eq!(parse_code(&template), "TMC-201");
}

#[test]
fn overlong_layer_id_is_201() {
    let long_id = "l".repeat(65);
    let layers = format!(r##"[ {{ "id": "{long_id}", "tracks": [] }} ]"##);
    let template = body_doc("", Some(&layers), 4);
    assert_eq!(parse_code(&template), "TMC-201");
}

#[test]
fn invalid_bone_name_is_201() {
    let template = body_doc(
        r##"{ "bone": "Left Arm", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0] } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-201");
}

#[test]
fn too_many_layers_is_107() {
    let mut layers = String::from("[");
    for index in 0..65 {
        if index > 0 {
            layers.push(',');
        }
        layers.push_str(&format!(r##"{{ "id": "layer-{index}", "tracks": [] }}"##));
    }
    layers.push(']');
    let template = body_doc("", Some(&layers), 4);
    assert_eq!(parse_code(&template), "TMC-107");
}

#[test]
fn authored_group_with_wrong_converter_is_103() {
    let template = MINIMAL.replace(
        "\"converter\": \"authored\"",
        "\"converter\": \"someone/1\"",
    );
    assert_eq!(parse_code(&template), "TMC-103");
}

#[test]
fn camera_group_without_mode_is_103() {
    let template = r##"{
  "format": "torogao-motion-clip/1",
  "id": "test-clip",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": 4,
  "groups": [ { "kind": "camera" } ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "camera", "source_class": "authored", "converter": "authored" }
    ]
  }
}"##;
    assert_eq!(parse_code(template), "TMC-103");
}

#[test]
fn body_group_with_mode_is_104() {
    let template = MINIMAL.replace(
        "{ \"kind\": \"body\", \"tracks\": [] }",
        "{ \"kind\": \"body\", \"mode\": \"static\", \"tracks\": [] }",
    );
    assert_eq!(parse_code(&template), "TMC-104");
}

#[test]
fn track_with_no_component_lists_is_103() {
    let template = body_doc(r##"{ "bone": "Hips" }"##, None, 4);
    assert_eq!(parse_code(&template), "TMC-103");
}

#[test]
fn track_with_empty_key_list_is_103() {
    let template = body_doc(r##"{ "bone": "Hips", "rotation": [] }"##, None, 4);
    assert_eq!(parse_code(&template), "TMC-103");
}

#[test]
fn channel_track_in_body_group_is_103() {
    // A track of the wrong kind for its group violates
    // the track grammar (SPEC sections 4.1, 10.1).
    let template = body_doc(
        r##"{ "channel": "blink", "keys": [ { "frame": 0, "value": 0.0 } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-103");
}

#[test]
fn bone_track_in_face_group_is_103() {
    let template = face_doc(
        r##"{ "bone": "Hips", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0] } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-103");
}

#[test]
fn groups_out_of_canonical_order_is_205() {
    let template = r##"{
  "format": "torogao-motion-clip/1",
  "id": "test-clip",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": 4,
  "groups": [
    { "kind": "face", "tracks": [] },
    { "kind": "body", "tracks": [] }
  ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "body", "source_class": "authored", "converter": "authored" },
      { "group": "face", "source_class": "authored", "converter": "authored" }
    ]
  }
}"##;
    assert_eq!(parse_code(template), "TMC-205");
}

#[test]
fn tracks_out_of_canonical_order_is_205() {
    let template = body_doc(
        r##"{ "bone": "Hips", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0] } ] },
            { "bone": "Head", "translation": [ { "frame": 0, "value": [0.0, 0.0, 0.0] } ] }"##,
        None,
        4,
    );
    assert_eq!(parse_code(&template), "TMC-205");
}

#[test]
fn scope_out_of_canonical_order_is_205() {
    let template = body_doc(
        "",
        Some(r##"[ { "id": "l1", "scope": { "bones": ["Spine", "Head"] }, "tracks": [] } ]"##),
        4,
    );
    assert_eq!(parse_code(&template), "TMC-205");
}

#[test]
fn provenance_format_token_wrong_is_103() {
    let template = MINIMAL.replace("torogao-take-provenance/1", "torogao-take-provenance/2");
    assert_eq!(parse_code(&template), "TMC-103");
}
