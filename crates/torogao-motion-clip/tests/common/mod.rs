//! Shared fixtures for the torogao-motion-clip test suites.
//!
//! The test SkeletonDef is a small humanoid in the shape of
//! SPEC.md Appendix B (Hips root, Spine/Chest/Neck/Head
//! chain, both arms). Its "presented document" is a fixed
//! byte string; clips in these tests bind to its real
//! SHA-256 via the `%SHA%` placeholder, so the binding
//! checks run against genuine hashes, not stubs. Rest
//! transforms are exact binary fractions wherever a test
//! compares bits.

#![allow(dead_code)]

use torogao_motion_clip::{
    BoneDef, BoundClip, ChannelDecls, ChannelDef, Clip, EvaluatedFrame, SkeletonDef,
    parse_document, sha256_hex,
};

/// The presented SkeletonDef document (test fixture form).
pub const SKELETON_DOC: &str = "{\"format\":\"torogao-skeleton-def-fixture/1\",\
\"def\":\"torogao-humanoid/1\",\"bones\":[\"Hips\",\"Spine\",\"Chest\",\"Neck\",\"Head\",\
\"LeftUpperArm\",\"LeftLowerArm\",\"RightUpperArm\"]}";

/// The SHA-256 the fixture clips bind to.
pub fn skeleton_sha() -> String {
    sha256_hex(SKELETON_DOC.as_bytes())
}

/// Substitute the skeleton hash into a clip template.
pub fn render(template: &str) -> Vec<u8> {
    template.replace("%SHA%", &skeleton_sha()).into_bytes()
}

/// Parse a template that must be valid; panics with the
/// error if not (a test-authoring failure, not a gate).
pub fn parse(template: &str) -> Clip {
    match parse_document(&render(template)) {
        Ok(clip) => clip,
        Err(error) => panic!("template must parse, got {error}"),
    }
}

/// Parse a template that must fail, and return the error's
/// code for exact-code assertions.
pub fn parse_code(template: &str) -> &'static str {
    match parse_document(&render(template)) {
        Ok(_) => panic!("template must fail validation"),
        Err(error) => error.code(),
    }
}

/// A small humanoid SkeletonDef (Appendix B shape).
pub struct TestSkeleton {
    bones: Vec<BoneDef>,
}

impl TestSkeleton {
    pub fn new() -> TestSkeleton {
        let identity = [0.0, 0.0, 0.0, 1.0];
        let unit = [1.0, 1.0, 1.0];
        let bone = |name: &str, translation: [f64; 3]| BoneDef {
            name: name.to_string(),
            rest_translation: translation,
            rest_rotation: identity,
            rest_scale: unit,
        };
        TestSkeleton {
            bones: vec![
                bone("Hips", [0.0, 1.0, 0.0]),
                bone("Spine", [0.0, 0.125, 0.0]),
                bone("Chest", [0.0, 0.125, 0.0]),
                bone("Neck", [0.0, 0.125, 0.0]),
                bone("Head", [0.0, 0.0625, 0.0]),
                bone("LeftUpperArm", [0.25, 0.0, 0.0]),
                bone("LeftLowerArm", [0.25, 0.0, 0.0]),
                bone("RightUpperArm", [-0.25, 0.0, 0.0]),
            ],
        }
    }
}

impl SkeletonDef for TestSkeleton {
    fn def_token(&self) -> &str {
        "torogao-humanoid/1"
    }

    fn document_bytes(&self) -> &[u8] {
        SKELETON_DOC.as_bytes()
    }

    fn bones(&self) -> &[BoneDef] {
        &self.bones
    }
}

/// The rig's channel declarations for tests.
pub struct TestChannels {
    declarations: Vec<(String, ChannelDef)>,
}

impl TestChannels {
    pub fn new() -> TestChannels {
        let declaration = |default: f64, min: f64, max: f64| ChannelDef { default, min, max };
        TestChannels {
            declarations: vec![
                ("blink".to_string(), declaration(0.0, 0.0, 1.0)),
                ("jawOpen".to_string(), declaration(0.0, 0.0, 1.0)),
                ("browInnerUp".to_string(), declaration(0.0, 0.0, 1.0)),
                ("smile".to_string(), declaration(0.0, -1.0, 1.0)),
            ],
        }
    }
}

impl ChannelDecls for TestChannels {
    fn channel(&self, name: &str) -> Option<ChannelDef> {
        self.declarations
            .iter()
            .find(|(declared, _)| declared == name)
            .map(|(_, declaration)| *declaration)
    }
}

/// Bind a clip to the test skeleton and channels.
pub fn bind<'a>(
    clip: &'a Clip,
    skeleton: &'a TestSkeleton,
    channels: &'a TestChannels,
) -> BoundClip<'a> {
    match BoundClip::bind(clip, skeleton, channels) {
        Ok(bound) => bound,
        Err(error) => panic!("clip must bind, got {error}"),
    }
}

/// Assert two evaluated-frame sequences are BIT-identical
/// (f64 bit patterns, so signed zeros count) — the identity
/// PT-2/PT-3/PT-6 promise.
pub fn assert_frames_bit_identical(a: &[EvaluatedFrame], b: &[EvaluatedFrame]) {
    assert_eq!(a.len(), b.len(), "frame counts differ");
    for (index, (frame_a, frame_b)) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(frame_a.bones.len(), frame_b.bones.len(), "frame {index}");
        for ((name_a, pose_a), (name_b, pose_b)) in frame_a.bones.iter().zip(frame_b.bones.iter()) {
            assert_eq!(name_a, name_b, "frame {index} bone order");
            for (label, values_a, values_b) in [
                (
                    "translation",
                    pose_a.translation.as_slice(),
                    pose_b.translation.as_slice(),
                ),
                (
                    "rotation",
                    pose_a.rotation.as_slice(),
                    pose_b.rotation.as_slice(),
                ),
                ("scale", pose_a.scale.as_slice(), pose_b.scale.as_slice()),
            ] {
                for (component, (value_a, value_b)) in
                    values_a.iter().zip(values_b.iter()).enumerate()
                {
                    assert_eq!(
                        value_a.to_bits(),
                        value_b.to_bits(),
                        "frame {index} bone {name_a} {label}[{component}] bits differ"
                    );
                }
            }
        }
        assert_eq!(
            frame_a.channels.len(),
            frame_b.channels.len(),
            "frame {index}"
        );
        for ((name_a, value_a), (name_b, value_b)) in
            frame_a.channels.iter().zip(frame_b.channels.iter())
        {
            assert_eq!(name_a, name_b, "frame {index} channel order");
            assert_eq!(
                value_a.to_bits(),
                value_b.to_bits(),
                "frame {index} channel {name_a} bits differ"
            );
        }
    }
}

/// The smallest valid clip: one body group, no tracks, one
/// authored provenance entry. `%SHA%` is the skeleton hash.
pub const MINIMAL: &str = r##"{
  "format": "torogao-motion-clip/1",
  "id": "test-clip",
  "skeleton": { "def": "torogao-humanoid/1", "sha256": "%SHA%" },
  "frames": 8,
  "groups": [
    { "kind": "body", "tracks": [] }
  ],
  "provenance": {
    "format": "torogao-take-provenance/1",
    "groups": [
      { "group": "body", "source_class": "authored", "converter": "authored" }
    ]
  }
}"##;
