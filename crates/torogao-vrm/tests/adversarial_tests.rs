//! Adversarial half: malformed inputs and refusals. Every case
//! must fail with the named error, never a silent default.

mod common;

use torogao_vrm::glb::{parse_glb, write_glb};
use torogao_vrm::{
    ExportOptions, ExpressionMap, VrmError, VrmProfile, export_vrm, prepare_spring, validate_vrm,
};

use common::{COMPLETE_PROFILE, fixture_glb};

fn profile() -> VrmProfile {
    VrmProfile::load(COMPLETE_PROFILE).expect("fixture profile checks")
}

/// Export without a profile is the crate's headline refusal.
#[test]
fn export_without_profile_is_refused_by_name() {
    let source = fixture_glb(&[], None);
    let err = export_vrm(&source, None, &ExportOptions::default()).expect_err("must refuse");
    assert!(matches!(err, VrmError::MissingProfile), "{err}");
}

/// A profile with fields unset names exactly the unset fields.
#[test]
fn partial_profile_names_every_unset_field() {
    let text = r#"(id: "vrm-profile/1", name: "Only A Name")"#;
    let err = VrmProfile::load(text).expect_err("must refuse");
    match err {
        VrmError::ProfileUnset { fields } => {
            assert_eq!(fields.len(), 12, "{fields:?}");
            assert!(fields.contains(&"avatar_permission".to_string()));
            assert!(fields.contains(&"modification".to_string()));
            assert!(fields.contains(&"allow_redistribution".to_string()));
            assert!(!fields.contains(&"name".to_string()));
        }
        other => panic!("expected ProfileUnset, got {other}"),
    }
}

/// Malformed RON is a parse failure, not a defaulted profile.
#[test]
fn malformed_profile_is_a_parse_failure() {
    let err = VrmProfile::load("(id: \"vrm-profile/1\", name: 42").expect_err("must fail");
    assert!(matches!(err, VrmError::ProfileParse { .. }), "{err}");
}

/// Wrong format token is refused before any field is read.
#[test]
fn wrong_profile_id_is_refused() {
    let text = COMPLETE_PROFILE.replace("vrm-profile/1", "vrm-profile/9");
    let err = VrmProfile::load(&text).expect_err("must refuse");
    assert!(matches!(err, VrmError::ProfileWrongId { .. }), "{err}");
}

/// A source missing a required bone is refused with the VRM
/// names of what is absent.
#[test]
fn missing_required_bone_is_refused() {
    let source = fixture_glb(&[], Some(("LeftHand", "LeftHandX")));
    let err =
        export_vrm(&source, Some(&profile()), &ExportOptions::default()).expect_err("must refuse");
    match err {
        VrmError::MissingRequiredBones { bones } => {
            assert!(bones.contains(&"leftHand".to_string()), "{bones:?}");
        }
        other => panic!("expected MissingRequiredBones, got {other}"),
    }
}

/// A spring joint named in VRM space is unknown, loudly.
#[test]
fn spring_joint_in_vrm_space_is_unknown() {
    let source = fixture_glb(&[], None);
    let text = r#"(
        id: "torogao-vrm-spring/1",
        chains: [(name: "c", joints: [
            (node: "leftHand", hit_radius: 0.1, stiffness: 1.0, gravity_power: 0.0, gravity_dir: (0.0, -1.0, 0.0), drag_force: 0.5),
        ])],
    )"#;
    let err = prepare_spring(text, &source).expect_err("must refuse");
    assert!(matches!(err, VrmError::UnknownBoneName { .. }), "{err}");
}

/// A spring joint naming a bone the model does not carry is a
/// dangling reference, named with its chain.
#[test]
fn dangling_spring_joint_is_named() {
    let source = fixture_glb(&["Spine2"], None);
    let text = r#"(
        id: "torogao-vrm-spring/1",
        chains: [(name: "spine sway", joints: [
            (node: "Spine", hit_radius: 0.1, stiffness: 1.0, gravity_power: 0.0, gravity_dir: (0.0, -1.0, 0.0), drag_force: 0.5),
            (node: "UpperChest", hit_radius: 0.1, stiffness: 1.0, gravity_power: 0.0, gravity_dir: (0.0, -1.0, 0.0), drag_force: 0.5),
        ])],
    )"#;
    let err = prepare_spring(text, &source).expect_err("must refuse");
    match err {
        VrmError::DanglingSpringJoint { bone, chain } => {
            assert_eq!(bone, "UpperChest");
            assert_eq!(chain, "spine sway");
        }
        other => panic!("expected DanglingSpringJoint, got {other}"),
    }
}

/// A chain whose tip precedes its root violates the spec's
/// ancestry rule.
#[test]
fn spring_chain_out_of_order_is_refused() {
    let source = fixture_glb(&[], None);
    let text = r#"(
        id: "torogao-vrm-spring/1",
        chains: [(name: "c", joints: [
            (node: "Head", hit_radius: 0.1, stiffness: 1.0, gravity_power: 0.0, gravity_dir: (0.0, -1.0, 0.0), drag_force: 0.5),
            (node: "Spine", hit_radius: 0.1, stiffness: 1.0, gravity_power: 0.0, gravity_dir: (0.0, -1.0, 0.0), drag_force: 0.5),
        ])],
    )"#;
    let err = prepare_spring(text, &source).expect_err("must refuse");
    assert!(matches!(err, VrmError::SpringChainOrder { .. }), "{err}");
}

/// A governed expression referencing a morph the model lacks
/// is a hard error — the bind would point at nothing.
#[test]
fn governed_expression_with_absent_morph_is_refused() {
    let source = fixture_glb(&[], None);
    let map = ExpressionMap::parse(
        r#"{"character":"Fixture","format":"torogao-expression-map-3d/1",
            "expressions":{"oo":{"morphs":{"Fac_Mth_OoBig":0.5}}}}"#,
    )
    .expect("map parses");
    let options = ExportOptions {
        expressions: Some(&map),
        spring: None,
    };
    let err = export_vrm(&source, Some(&profile()), &options).expect_err("must refuse");
    assert!(matches!(err, VrmError::MorphTargetAbsent { .. }), "{err}");
}

/// Meta tampered after export fails read-back validation,
/// naming the field that disagrees with the profile.
#[test]
fn tampered_meta_fails_readback() {
    let source = fixture_glb(&[], None);
    let out = export_vrm(&source, Some(&profile()), &ExportOptions::default()).expect("export");
    let parsed = parse_glb(&out).expect("parses");
    let mut json = parsed.json.clone();
    json["extensions"]["VRMC_vrm"]["meta"]["allowRedistribution"] = serde_json::json!(true);
    let tampered = write_glb(&json, &parsed.bin).expect("rewrites");
    let err = validate_vrm(&tampered, Some(&profile())).expect_err("must fail");
    match err {
        VrmError::ValidationFailed { problems } => {
            assert!(
                problems
                    .iter()
                    .any(|problem| problem.contains("allowRedistribution")),
                "{problems:?}"
            );
        }
        other => panic!("expected ValidationFailed, got {other}"),
    }
}

/// A file that is not a VRM at all fails validation on the
/// missing extension, not with a panic.
#[test]
fn plain_glb_is_not_a_vrm() {
    let source = fixture_glb(&[], None);
    let err = validate_vrm(&source, None).expect_err("must fail");
    assert!(matches!(err, VrmError::ValidationFailed { .. }), "{err}");
}
