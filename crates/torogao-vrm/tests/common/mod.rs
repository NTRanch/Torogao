//! Shared fixtures for the torogao-vrm integration tests: a
//! synthetic humanoid GLB (Mixamo bone names, one morph-carrying
//! mesh) and a complete profile document.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use torogao_vrm::glb::write_glb;

/// The complete test profile, every field stated.
pub const COMPLETE_PROFILE: &str = r#"(
    id: "vrm-profile/1",
    name: "Fixture Character",
    version: "1.0.0",
    authors: ["Fixture Studio"],
    copyright_information: Some("Fixture copyright"),
    license_url: "https://vrm.dev/licenses/1.0/",
    avatar_permission: OnlyAuthor,
    allow_excessively_violent_usage: false,
    allow_excessively_sexual_usage: true,
    commercial_usage: PersonalProfit,
    allow_political_or_religious_usage: false,
    allow_antisocial_or_hate_usage: false,
    credit_notation: Required,
    allow_redistribution: false,
    modification: Prohibited,
)"#;

/// The morph target names the fixture's Body mesh declares.
pub const FIXTURE_TARGETS: &[&str] = &[
    "Fac_Ebr_Sad",
    "Fac_Eye_Close",
    "Fac_Eye_L_Wink",
    "Fac_Eye_R_Wink",
    "Fac_Mth_Aa",
    "Fac_Mth_Ee",
    "Fac_Mth_Ii",
    "Fac_Mth_Oo",
    "Fac_Mth_Smile",
    "Fac_Mth_Uu",
];

/// Bone node names in the fixture, in document order.
const BONE_NAMES: &[&str] = &[
    "Hips",
    "Spine",
    "Spine1",
    "Spine2",
    "Neck",
    "Head",
    "LeftEye",
    "RightEye",
    "LeftShoulder",
    "LeftArm",
    "LeftForeArm",
    "LeftHand",
    "LeftHandThumb1",
    "LeftHandThumb2",
    "LeftHandThumb3",
    "LeftHandIndex1",
    "LeftHandIndex2",
    "LeftHandIndex3",
    "LeftHandMiddle1",
    "LeftHandMiddle2",
    "LeftHandMiddle3",
    "LeftHandRing1",
    "LeftHandRing2",
    "LeftHandRing3",
    "LeftHandPinky1",
    "LeftHandPinky2",
    "LeftHandPinky3",
    "RightShoulder",
    "RightArm",
    "RightForeArm",
    "RightHand",
    "RightHandThumb1",
    "RightHandThumb2",
    "RightHandThumb3",
    "RightHandIndex1",
    "RightHandIndex2",
    "RightHandIndex3",
    "RightHandMiddle1",
    "RightHandMiddle2",
    "RightHandMiddle3",
    "RightHandRing1",
    "RightHandRing2",
    "RightHandRing3",
    "RightHandPinky1",
    "RightHandPinky2",
    "RightHandPinky3",
    "LeftUpLeg",
    "LeftLeg",
    "LeftFoot",
    "LeftToeBase",
    "RightUpLeg",
    "RightLeg",
    "RightFoot",
    "RightToeBase",
];

/// Parent → children edges between bone names.
const EDGES: &[(&str, &str)] = &[
    ("Hips", "Spine"),
    ("Spine", "Spine1"),
    ("Spine1", "Spine2"),
    ("Spine2", "Neck"),
    ("Neck", "Head"),
    ("Head", "LeftEye"),
    ("Head", "RightEye"),
    ("Spine2", "LeftShoulder"),
    ("LeftShoulder", "LeftArm"),
    ("LeftArm", "LeftForeArm"),
    ("LeftForeArm", "LeftHand"),
    ("LeftHand", "LeftHandThumb1"),
    ("LeftHandThumb1", "LeftHandThumb2"),
    ("LeftHandThumb2", "LeftHandThumb3"),
    ("LeftHand", "LeftHandIndex1"),
    ("LeftHandIndex1", "LeftHandIndex2"),
    ("LeftHandIndex2", "LeftHandIndex3"),
    ("LeftHand", "LeftHandMiddle1"),
    ("LeftHandMiddle1", "LeftHandMiddle2"),
    ("LeftHandMiddle2", "LeftHandMiddle3"),
    ("LeftHand", "LeftHandRing1"),
    ("LeftHandRing1", "LeftHandRing2"),
    ("LeftHandRing2", "LeftHandRing3"),
    ("LeftHand", "LeftHandPinky1"),
    ("LeftHandPinky1", "LeftHandPinky2"),
    ("LeftHandPinky2", "LeftHandPinky3"),
    ("Spine2", "RightShoulder"),
    ("RightShoulder", "RightArm"),
    ("RightArm", "RightForeArm"),
    ("RightForeArm", "RightHand"),
    ("RightHand", "RightHandThumb1"),
    ("RightHandThumb1", "RightHandThumb2"),
    ("RightHandThumb2", "RightHandThumb3"),
    ("RightHand", "RightHandIndex1"),
    ("RightHandIndex1", "RightHandIndex2"),
    ("RightHandIndex2", "RightHandIndex3"),
    ("RightHand", "RightHandMiddle1"),
    ("RightHandMiddle1", "RightHandMiddle2"),
    ("RightHandMiddle2", "RightHandMiddle3"),
    ("RightHand", "RightHandRing1"),
    ("RightHandRing1", "RightHandRing2"),
    ("RightHandRing2", "RightHandRing3"),
    ("RightHand", "RightHandPinky1"),
    ("RightHandPinky1", "RightHandPinky2"),
    ("RightHandPinky2", "RightHandPinky3"),
    ("Hips", "LeftUpLeg"),
    ("LeftUpLeg", "LeftLeg"),
    ("LeftLeg", "LeftFoot"),
    ("LeftFoot", "LeftToeBase"),
    ("Hips", "RightUpLeg"),
    ("RightUpLeg", "RightLeg"),
    ("RightLeg", "RightFoot"),
    ("RightFoot", "RightToeBase"),
];

/// Build the fixture GLB. `skip` drops bone nodes by name (for
/// the missing-bone and dangling-joint cases); `rename` renames
/// one node.
pub fn fixture_glb(skip: &[&str], rename: Option<(&str, &str)>) -> Vec<u8> {
    let mut index_of: BTreeMap<&str, usize> = BTreeMap::new();
    let mut nodes: Vec<Value> = Vec::new();
    for name in BONE_NAMES {
        if skip.contains(name) {
            continue;
        }
        let shown = match rename {
            Some((from, to)) if from == *name => to,
            _ => name,
        };
        index_of.insert(name, nodes.len());
        nodes.push(json!({"name": shown}));
    }
    let mut children: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (parent, child) in EDGES {
        if let (Some(&p), Some(&c)) = (index_of.get(parent), index_of.get(child)) {
            children.entry(p).or_default().push(c);
        }
    }
    for (parent, kids) in &children {
        nodes[*parent]["children"] = json!(kids);
    }
    let body_index = nodes.len();
    nodes.push(json!({"name": "Body", "mesh": 0}));
    let joints: Vec<usize> = (0..body_index).collect();
    let targets: Vec<Value> = FIXTURE_TARGETS.iter().map(|_| json!({})).collect();
    let document = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0, body_index]}],
        "nodes": nodes,
        "meshes": [{
            "name": "Body",
            "extras": {"targetNames": FIXTURE_TARGETS},
            "primitives": [{"attributes": {"POSITION": 0}, "targets": targets}],
        }],
        "skins": [{"joints": joints}],
        "buffers": [{"byteLength": 16}],
        "bufferViews": [{"buffer": 0, "byteLength": 16}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3"}],
    });
    write_glb(&document, &[0u8; 16]).expect("fixture writes")
}
