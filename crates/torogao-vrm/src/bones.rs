//! Bone vocabularies and the maps between them.
//!
//! Three name spaces meet here:
//! - **torogao-humanoid/1**: the estate's skeleton vocabulary (the
//!   VMC humanoid set, as enumerated by torogao-format's MMD ingest
//!   validator, plus the two gaze bones the cast rigs carry).
//! - **VRM 1.0 humanoid**: the camelCase names `VRMC_vrm` requires.
//! - **Source rig names**: the node names the cast GLBs actually
//!   carry today (Mixamo-style, `mixamorig:` prefixed).
//!
//! Maps are data in this module, stated once, matched exactly. A
//! name in none of the maps resolves to nothing, and callers turn
//! that into a named error — never a guess.

/// The torogao-humanoid/1 bone vocabulary: the VMC humanoid set.
/// `Root` is the motion root; it has no VRM humanoid counterpart
/// and never appears in a `humanBones` block.
pub const HUMANOID_BONES: &[&str] = &[
    "Root",
    "Hips",
    "Spine",
    "Chest",
    "UpperChest",
    "Neck",
    "Head",
    "LeftShoulder",
    "LeftUpperArm",
    "LeftLowerArm",
    "LeftHand",
    "RightShoulder",
    "RightUpperArm",
    "RightLowerArm",
    "RightHand",
    "LeftUpperLeg",
    "LeftLowerLeg",
    "LeftFoot",
    "LeftToes",
    "RightUpperLeg",
    "RightLowerLeg",
    "RightFoot",
    "RightToes",
    "LeftThumbProximal",
    "LeftThumbIntermediate",
    "LeftThumbDistal",
    "LeftIndexProximal",
    "LeftIndexIntermediate",
    "LeftIndexDistal",
    "LeftMiddleProximal",
    "LeftMiddleIntermediate",
    "LeftMiddleDistal",
    "LeftRingProximal",
    "LeftRingIntermediate",
    "LeftRingDistal",
    "LeftLittleProximal",
    "LeftLittleIntermediate",
    "LeftLittleDistal",
    "RightThumbProximal",
    "RightThumbIntermediate",
    "RightThumbDistal",
    "RightIndexProximal",
    "RightIndexIntermediate",
    "RightIndexDistal",
    "RightMiddleProximal",
    "RightMiddleIntermediate",
    "RightMiddleDistal",
    "RightRingProximal",
    "RightRingIntermediate",
    "RightRingDistal",
    "RightLittleProximal",
    "RightLittleIntermediate",
    "RightLittleDistal",
];

/// Gaze bones the cast rigs carry (`mixamorig:LeftEye` and kin).
/// They sit outside the torogao-humanoid/1 core set — the MMD
/// profile ledgers eye bones as gaze, not skeleton — but VRM 1.0
/// has optional eye bones, so the exporter recognizes them.
pub const GAZE_BONES: &[&str] = &["LeftEye", "RightEye"];

/// The union skeleton's skin joints, in source-rig (Mixamo) names
/// and lumen joint-table order (Owner ruling D-1282, 2026-11-11).
///
/// The union is the articulation set shared by the lumen factory
/// skeleton and the torogao cast rigs: the 52 joints both sides
/// already carried, plus `LeftEye` / `RightEye` from the cast
/// side. A GLB on the union skeleton skins exactly these 54
/// joints; every one of them must resolve through
/// [`resolve_source_node`].
pub const UNION_SKIN_JOINTS: &[&str] = &[
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
    "LeftUpLeg",
    "LeftLeg",
    "LeftFoot",
    "LeftToeBase",
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
    "RightUpLeg",
    "RightLeg",
    "RightFoot",
    "RightToeBase",
];

/// The union skeleton's landmark nodes: terminal `*_End` leaves
/// the lumen factory keeps as plain nodes for construction
/// endpoints (head height, leg-tube and finger chain ends). They
/// are never skin joints and never resolve to a bone; a loader
/// meets them as nodes only and skips them by design.
pub const UNION_LANDMARK_NODES: &[&str] = &[
    "HeadTop_End",
    "LeftHandThumb_End",
    "LeftHandIndex_End",
    "LeftHandMiddle_End",
    "LeftHandRing_End",
    "LeftHandPinky_End",
    "LeftToe_End",
    "RightHandThumb_End",
    "RightHandIndex_End",
    "RightHandMiddle_End",
    "RightHandRing_End",
    "RightHandPinky_End",
    "RightToe_End",
];

/// One torogao → VRM humanoid bone correspondence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoneDef {
    /// The torogao-humanoid/1 name.
    pub torogao: &'static str,
    /// The VRM 1.0 `humanBones` key.
    pub vrm: &'static str,
    /// Whether VRM 1.0 requires this bone in every humanoid.
    pub required: bool,
}

/// The torogao → VRM map, alphabetical by torogao name. The one
/// non-mechanical row is the thumb: torogao/VMC counts the thumb
/// proximal/intermediate/distal, VRM counts
/// metacarpal/proximal/distal for the same three joints.
pub const BONE_DEFS: &[BoneDef] = &[
    BoneDef {
        torogao: "Chest",
        vrm: "chest",
        required: false,
    },
    BoneDef {
        torogao: "Head",
        vrm: "head",
        required: true,
    },
    BoneDef {
        torogao: "Hips",
        vrm: "hips",
        required: true,
    },
    BoneDef {
        torogao: "LeftEye",
        vrm: "leftEye",
        required: false,
    },
    BoneDef {
        torogao: "LeftFoot",
        vrm: "leftFoot",
        required: true,
    },
    BoneDef {
        torogao: "LeftHand",
        vrm: "leftHand",
        required: true,
    },
    BoneDef {
        torogao: "LeftIndexDistal",
        vrm: "leftIndexDistal",
        required: false,
    },
    BoneDef {
        torogao: "LeftIndexIntermediate",
        vrm: "leftIndexIntermediate",
        required: false,
    },
    BoneDef {
        torogao: "LeftIndexProximal",
        vrm: "leftIndexProximal",
        required: false,
    },
    BoneDef {
        torogao: "LeftLittleDistal",
        vrm: "leftLittleDistal",
        required: false,
    },
    BoneDef {
        torogao: "LeftLittleIntermediate",
        vrm: "leftLittleIntermediate",
        required: false,
    },
    BoneDef {
        torogao: "LeftLittleProximal",
        vrm: "leftLittleProximal",
        required: false,
    },
    BoneDef {
        torogao: "LeftLowerArm",
        vrm: "leftLowerArm",
        required: true,
    },
    BoneDef {
        torogao: "LeftLowerLeg",
        vrm: "leftLowerLeg",
        required: true,
    },
    BoneDef {
        torogao: "LeftMiddleDistal",
        vrm: "leftMiddleDistal",
        required: false,
    },
    BoneDef {
        torogao: "LeftMiddleIntermediate",
        vrm: "leftMiddleIntermediate",
        required: false,
    },
    BoneDef {
        torogao: "LeftMiddleProximal",
        vrm: "leftMiddleProximal",
        required: false,
    },
    BoneDef {
        torogao: "LeftRingDistal",
        vrm: "leftRingDistal",
        required: false,
    },
    BoneDef {
        torogao: "LeftRingIntermediate",
        vrm: "leftRingIntermediate",
        required: false,
    },
    BoneDef {
        torogao: "LeftRingProximal",
        vrm: "leftRingProximal",
        required: false,
    },
    BoneDef {
        torogao: "LeftShoulder",
        vrm: "leftShoulder",
        required: false,
    },
    BoneDef {
        torogao: "LeftThumbDistal",
        vrm: "leftThumbDistal",
        required: false,
    },
    BoneDef {
        torogao: "LeftThumbIntermediate",
        vrm: "leftThumbProximal",
        required: false,
    },
    BoneDef {
        torogao: "LeftThumbProximal",
        vrm: "leftThumbMetacarpal",
        required: false,
    },
    BoneDef {
        torogao: "LeftToes",
        vrm: "leftToes",
        required: false,
    },
    BoneDef {
        torogao: "LeftUpperArm",
        vrm: "leftUpperArm",
        required: true,
    },
    BoneDef {
        torogao: "LeftUpperLeg",
        vrm: "leftUpperLeg",
        required: true,
    },
    BoneDef {
        torogao: "Neck",
        vrm: "neck",
        required: false,
    },
    BoneDef {
        torogao: "RightEye",
        vrm: "rightEye",
        required: false,
    },
    BoneDef {
        torogao: "RightFoot",
        vrm: "rightFoot",
        required: true,
    },
    BoneDef {
        torogao: "RightHand",
        vrm: "rightHand",
        required: true,
    },
    BoneDef {
        torogao: "RightIndexDistal",
        vrm: "rightIndexDistal",
        required: false,
    },
    BoneDef {
        torogao: "RightIndexIntermediate",
        vrm: "rightIndexIntermediate",
        required: false,
    },
    BoneDef {
        torogao: "RightIndexProximal",
        vrm: "rightIndexProximal",
        required: false,
    },
    BoneDef {
        torogao: "RightLittleDistal",
        vrm: "rightLittleDistal",
        required: false,
    },
    BoneDef {
        torogao: "RightLittleIntermediate",
        vrm: "rightLittleIntermediate",
        required: false,
    },
    BoneDef {
        torogao: "RightLittleProximal",
        vrm: "rightLittleProximal",
        required: false,
    },
    BoneDef {
        torogao: "RightLowerArm",
        vrm: "rightLowerArm",
        required: true,
    },
    BoneDef {
        torogao: "RightLowerLeg",
        vrm: "rightLowerLeg",
        required: true,
    },
    BoneDef {
        torogao: "RightMiddleDistal",
        vrm: "rightMiddleDistal",
        required: false,
    },
    BoneDef {
        torogao: "RightMiddleIntermediate",
        vrm: "rightMiddleIntermediate",
        required: false,
    },
    BoneDef {
        torogao: "RightMiddleProximal",
        vrm: "rightMiddleProximal",
        required: false,
    },
    BoneDef {
        torogao: "RightRingDistal",
        vrm: "rightRingDistal",
        required: false,
    },
    BoneDef {
        torogao: "RightRingIntermediate",
        vrm: "rightRingIntermediate",
        required: false,
    },
    BoneDef {
        torogao: "RightRingProximal",
        vrm: "rightRingProximal",
        required: false,
    },
    BoneDef {
        torogao: "RightShoulder",
        vrm: "rightShoulder",
        required: false,
    },
    BoneDef {
        torogao: "RightThumbDistal",
        vrm: "rightThumbDistal",
        required: false,
    },
    BoneDef {
        torogao: "RightThumbIntermediate",
        vrm: "rightThumbProximal",
        required: false,
    },
    BoneDef {
        torogao: "RightThumbProximal",
        vrm: "rightThumbMetacarpal",
        required: false,
    },
    BoneDef {
        torogao: "RightToes",
        vrm: "rightToes",
        required: false,
    },
    BoneDef {
        torogao: "RightUpperArm",
        vrm: "rightUpperArm",
        required: true,
    },
    BoneDef {
        torogao: "RightUpperLeg",
        vrm: "rightUpperLeg",
        required: true,
    },
    BoneDef {
        torogao: "Spine",
        vrm: "spine",
        required: true,
    },
    BoneDef {
        torogao: "UpperChest",
        vrm: "upperChest",
        required: false,
    },
];

/// VRM humanoid names with no torogao counterpart. Used only to
/// phrase the unknown-bone diagnostic: a caller who writes `jaw`
/// is speaking VRM, not torogao-humanoid/1.
pub const VRM_ONLY_BONE_NAMES: &[&str] = &["jaw"];

/// The def for a torogao bone name, if it has a VRM counterpart.
pub fn bone_def(torogao: &str) -> Option<&'static BoneDef> {
    BONE_DEFS.iter().find(|def| def.torogao == torogao)
}

/// The torogao bone for a VRM humanoid name, if any.
pub fn torogao_for_vrm(vrm: &str) -> Option<&'static str> {
    BONE_DEFS
        .iter()
        .find(|def| def.vrm == vrm)
        .map(|def| def.torogao)
}

/// Whether `name` is written in the VRM name space.
pub fn is_vrm_bone_name(name: &str) -> bool {
    VRM_ONLY_BONE_NAMES.contains(&name) || BONE_DEFS.iter().any(|def| def.vrm == name)
}

/// Whether `name` belongs to the torogao name space (the core
/// vocabulary or the recognized gaze bones).
pub fn is_torogao_bone_name(name: &str) -> bool {
    HUMANOID_BONES.contains(&name) || GAZE_BONES.contains(&name)
}

/// Resolve a source-rig node name to a torogao bone. Accepts the
/// bare torogao name, the bare Mixamo name, and either behind one
/// `prefix:` segment (the cast GLBs use `mixamorig:`). Anything
/// else resolves to `None`.
pub fn resolve_source_node(name: &str) -> Option<&'static str> {
    let base = name.rsplit(':').next().unwrap_or(name);
    match base {
        // Identity rows: torogao-named rigs resolve to themselves.
        "Chest" => Some("Chest"),
        "Head" => Some("Head"),
        "Hips" => Some("Hips"),
        "LeftEye" => Some("LeftEye"),
        "LeftFoot" => Some("LeftFoot"),
        "LeftHand" => Some("LeftHand"),
        "LeftIndexDistal" => Some("LeftIndexDistal"),
        "LeftIndexIntermediate" => Some("LeftIndexIntermediate"),
        "LeftIndexProximal" => Some("LeftIndexProximal"),
        "LeftLittleDistal" => Some("LeftLittleDistal"),
        "LeftLittleIntermediate" => Some("LeftLittleIntermediate"),
        "LeftLittleProximal" => Some("LeftLittleProximal"),
        "LeftLowerArm" => Some("LeftLowerArm"),
        "LeftLowerLeg" => Some("LeftLowerLeg"),
        "LeftMiddleDistal" => Some("LeftMiddleDistal"),
        "LeftMiddleIntermediate" => Some("LeftMiddleIntermediate"),
        "LeftMiddleProximal" => Some("LeftMiddleProximal"),
        "LeftRingDistal" => Some("LeftRingDistal"),
        "LeftRingIntermediate" => Some("LeftRingIntermediate"),
        "LeftRingProximal" => Some("LeftRingProximal"),
        "LeftShoulder" => Some("LeftShoulder"),
        "LeftThumbDistal" => Some("LeftThumbDistal"),
        "LeftThumbIntermediate" => Some("LeftThumbIntermediate"),
        "LeftThumbProximal" => Some("LeftThumbProximal"),
        "LeftToes" => Some("LeftToes"),
        "LeftUpperArm" => Some("LeftUpperArm"),
        "LeftUpperLeg" => Some("LeftUpperLeg"),
        "Neck" => Some("Neck"),
        "RightEye" => Some("RightEye"),
        "RightFoot" => Some("RightFoot"),
        "RightHand" => Some("RightHand"),
        "RightIndexDistal" => Some("RightIndexDistal"),
        "RightIndexIntermediate" => Some("RightIndexIntermediate"),
        "RightIndexProximal" => Some("RightIndexProximal"),
        "RightLittleDistal" => Some("RightLittleDistal"),
        "RightLittleIntermediate" => Some("RightLittleIntermediate"),
        "RightLittleProximal" => Some("RightLittleProximal"),
        "RightLowerArm" => Some("RightLowerArm"),
        "RightLowerLeg" => Some("RightLowerLeg"),
        "RightMiddleDistal" => Some("RightMiddleDistal"),
        "RightMiddleIntermediate" => Some("RightMiddleIntermediate"),
        "RightMiddleProximal" => Some("RightMiddleProximal"),
        "RightRingDistal" => Some("RightRingDistal"),
        "RightRingIntermediate" => Some("RightRingIntermediate"),
        "RightRingProximal" => Some("RightRingProximal"),
        "RightShoulder" => Some("RightShoulder"),
        "RightThumbDistal" => Some("RightThumbDistal"),
        "RightThumbIntermediate" => Some("RightThumbIntermediate"),
        "RightThumbProximal" => Some("RightThumbProximal"),
        "RightToes" => Some("RightToes"),
        "RightUpperArm" => Some("RightUpperArm"),
        "RightUpperLeg" => Some("RightUpperLeg"),
        "Root" => Some("Root"),
        "Spine" => Some("Spine"),
        "UpperChest" => Some("UpperChest"),
        // Mixamo rows: the cast fleet's rig names.
        "LeftArm" => Some("LeftUpperArm"),
        "LeftForeArm" => Some("LeftLowerArm"),
        "LeftHandIndex1" => Some("LeftIndexProximal"),
        "LeftHandIndex2" => Some("LeftIndexIntermediate"),
        "LeftHandIndex3" => Some("LeftIndexDistal"),
        "LeftHandMiddle1" => Some("LeftMiddleProximal"),
        "LeftHandMiddle2" => Some("LeftMiddleIntermediate"),
        "LeftHandMiddle3" => Some("LeftMiddleDistal"),
        "LeftHandPinky1" => Some("LeftLittleProximal"),
        "LeftHandPinky2" => Some("LeftLittleIntermediate"),
        "LeftHandPinky3" => Some("LeftLittleDistal"),
        "LeftHandRing1" => Some("LeftRingProximal"),
        "LeftHandRing2" => Some("LeftRingIntermediate"),
        "LeftHandRing3" => Some("LeftRingDistal"),
        "LeftHandThumb1" => Some("LeftThumbProximal"),
        "LeftHandThumb2" => Some("LeftThumbIntermediate"),
        "LeftHandThumb3" => Some("LeftThumbDistal"),
        "LeftLeg" => Some("LeftLowerLeg"),
        "LeftToeBase" => Some("LeftToes"),
        "LeftUpLeg" => Some("LeftUpperLeg"),
        "RightArm" => Some("RightUpperArm"),
        "RightForeArm" => Some("RightLowerArm"),
        "RightHandIndex1" => Some("RightIndexProximal"),
        "RightHandIndex2" => Some("RightIndexIntermediate"),
        "RightHandIndex3" => Some("RightIndexDistal"),
        "RightHandMiddle1" => Some("RightMiddleProximal"),
        "RightHandMiddle2" => Some("RightMiddleIntermediate"),
        "RightHandMiddle3" => Some("RightMiddleDistal"),
        "RightHandPinky1" => Some("RightLittleProximal"),
        "RightHandPinky2" => Some("RightLittleIntermediate"),
        "RightHandPinky3" => Some("RightLittleDistal"),
        "RightHandRing1" => Some("RightRingProximal"),
        "RightHandRing2" => Some("RightRingIntermediate"),
        "RightHandRing3" => Some("RightRingDistal"),
        "RightHandThumb1" => Some("RightThumbProximal"),
        "RightHandThumb2" => Some("RightThumbIntermediate"),
        "RightHandThumb3" => Some("RightThumbDistal"),
        "RightLeg" => Some("RightLowerLeg"),
        "RightToeBase" => Some("RightToes"),
        "RightUpLeg" => Some("RightUpperLeg"),
        "Spine1" => Some("Chest"),
        "Spine2" => Some("UpperChest"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{BONE_DEFS, bone_def, is_vrm_bone_name, resolve_source_node, torogao_for_vrm};

    /// Every def round-trips through the reverse map.
    #[test]
    fn bone_defs_round_trip() {
        for def in BONE_DEFS {
            assert_eq!(bone_def(def.torogao), Some(def));
            assert_eq!(torogao_for_vrm(def.vrm), Some(def.torogao));
        }
    }

    /// VRM requires exactly fifteen bones.
    #[test]
    fn required_set_has_fifteen_bones() {
        assert_eq!(BONE_DEFS.iter().filter(|def| def.required).count(), 15);
    }

    /// The thumb's torogao proximal lands on VRM's metacarpal.
    #[test]
    fn thumb_rows_use_vrm_metacarpal_naming() {
        assert_eq!(
            bone_def("LeftThumbProximal").map(|def| def.vrm),
            Some("leftThumbMetacarpal")
        );
        assert_eq!(
            bone_def("RightThumbIntermediate").map(|def| def.vrm),
            Some("rightThumbProximal")
        );
    }

    /// Prefixed, bare-Mixamo, and bare-torogao names all resolve.
    #[test]
    fn source_names_resolve_across_conventions() {
        assert_eq!(
            resolve_source_node("mixamorig:LeftArm"),
            Some("LeftUpperArm")
        );
        assert_eq!(resolve_source_node("Spine2"), Some("UpperChest"));
        assert_eq!(resolve_source_node("LeftHand"), Some("LeftHand"));
        assert_eq!(resolve_source_node("mixamorig:Hips"), Some("Hips"));
        assert_eq!(resolve_source_node("Prop_Controller"), None);
    }

    /// VRM-space names are recognized for diagnostics.
    #[test]
    fn vrm_space_names_are_recognized() {
        assert!(is_vrm_bone_name("leftUpperArm"));
        assert!(is_vrm_bone_name("jaw"));
        assert!(!is_vrm_bone_name("LeftUpperArm"));
    }

    /// The union contract (D-1282): exactly 54 skin joints, every
    /// one resolving to a torogao bone, none of them a landmark.
    #[test]
    fn union_skin_joints_all_resolve() {
        use super::UNION_SKIN_JOINTS;
        assert_eq!(UNION_SKIN_JOINTS.len(), 54);
        for (index, name) in UNION_SKIN_JOINTS.iter().enumerate() {
            assert!(
                resolve_source_node(name).is_some(),
                "union skin joint {name} must resolve"
            );
            assert!(
                !name.ends_with("_End"),
                "union skin joint {name} is a landmark"
            );
            assert!(
                !UNION_SKIN_JOINTS[..index].contains(name),
                "union skin joint {name} is duplicated"
            );
        }
    }

    /// Union landmarks are node-only: 13 of them, none resolving
    /// to a bone and none in the skin list.
    #[test]
    fn union_landmarks_never_resolve() {
        use super::{UNION_LANDMARK_NODES, UNION_SKIN_JOINTS};
        assert_eq!(UNION_LANDMARK_NODES.len(), 13);
        for name in UNION_LANDMARK_NODES {
            assert!(name.ends_with("_End"), "landmark {name} is no End leaf");
            assert_eq!(
                resolve_source_node(name),
                None,
                "landmark {name} must not resolve to a bone"
            );
            assert!(
                !UNION_SKIN_JOINTS.contains(name),
                "landmark {name} is in the skin list"
            );
        }
    }
}
