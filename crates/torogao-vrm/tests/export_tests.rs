//! Validation half: exports that must succeed, and the proof
//! surfaces that must hold on their outputs.

mod common;

use torogao_vrm::glb::{census, parse_glb};
use torogao_vrm::{
    ExportOptions, ExpressionMap, VrmProfile, export_vrm, prepare_spring, validate_vrm,
};

use common::{COMPLETE_PROFILE, fixture_glb};

fn profile() -> VrmProfile {
    VrmProfile::load(COMPLETE_PROFILE).expect("fixture profile checks")
}

/// The baseline export: validates against its own profile, all
/// required bones land, and the census is untouched.
#[test]
fn export_validates_and_preserves_census() {
    let source = fixture_glb(&[], None);
    let out = export_vrm(&source, Some(&profile()), &ExportOptions::default()).expect("export");
    let report = validate_vrm(&out, Some(&profile())).expect("read-back validates");
    assert_eq!(report.meta_name, "Fixture Character");
    assert_eq!(report.humanoid_bones.len(), 54);
    assert!(
        report
            .humanoid_bones
            .contains(&"leftThumbMetacarpal".to_string())
    );
    assert!(report.humanoid_bones.contains(&"upperChest".to_string()));
    assert!(report.humanoid_bones.contains(&"leftEye".to_string()));
    assert_eq!(report.spring_chains, 0);
    let source_census = census(&parse_glb(&source).expect("source parses"));
    assert_eq!(report.census, source_census);
}

/// No spring record → no spring extension, cleanly.
#[test]
fn no_spring_record_means_no_spring_extension() {
    let source = fixture_glb(&[], None);
    let out = export_vrm(&source, Some(&profile()), &ExportOptions::default()).expect("export");
    let parsed = parse_glb(&out).expect("parses");
    assert!(parsed.json["extensions"].get("VRMC_springBone").is_none());
    let used = parsed.json["extensionsUsed"]
        .as_array()
        .expect("extensionsUsed");
    assert!(
        !used
            .iter()
            .any(|entry| entry.as_str() == Some("VRMC_springBone"))
    );
}

/// Governed expressions split preset/custom; synthesis fills
/// the viseme and blink presets the map does not define.
#[test]
fn governed_expressions_and_synthesis_compose() {
    let source = fixture_glb(&[], None);
    let map = ExpressionMap::parse(
        r#"{"character":"Fixture","format":"torogao-expression-map-3d/1",
            "expressions":{
              "joy":{"morphs":{"Fac_Mth_Smile":0.85,"Fac_Ebr_Sad":0.2}},
              "neutral":{"morphs":{"Fac_Mth_Smile":0.0}}
            }}"#,
    )
    .expect("map parses");
    let options = ExportOptions {
        expressions: Some(&map),
        spring: None,
    };
    let out = export_vrm(&source, Some(&profile()), &options).expect("export");
    let report = validate_vrm(&out, Some(&profile())).expect("validates");
    assert_eq!(report.custom_expressions, vec!["joy".to_string()]);
    for preset in [
        "aa",
        "blink",
        "blinkLeft",
        "blinkRight",
        "ee",
        "ih",
        "neutral",
        "oh",
        "ou",
    ] {
        assert!(
            report.preset_expressions.contains(&preset.to_string()),
            "preset {preset} missing: {:?}",
            report.preset_expressions
        );
    }
    let parsed = parse_glb(&out).expect("parses");
    let joy = &parsed.json["extensions"]["VRMC_vrm"]["expressions"]["custom"]["joy"];
    assert_eq!(joy["morphTargetBinds"][0]["weight"], 0.2);
    assert_eq!(joy["morphTargetBinds"][1]["weight"], 0.85);
}

/// A spring record emits a spring block that read-back accepts,
/// colliders and groups included.
#[test]
fn spring_record_emits_and_validates() {
    let source = fixture_glb(&[], None);
    let spring_text = r#"(
        id: "torogao-vrm-spring/1",
        colliders: [(
            node: "Head",
            shape: Sphere(offset: (0.0, 0.1, 0.0), radius: 0.12),
        )],
        collider_groups: [(name: "head guard", colliders: [0])],
        chains: [(
            name: "spine sway",
            joints: [
                (node: "Spine", hit_radius: 0.05, stiffness: 1.0, gravity_power: 0.5, gravity_dir: (0.0, -1.0, 0.0), drag_force: 0.5),
                (node: "Head", hit_radius: 0.05, stiffness: 0.8, gravity_power: 0.5, gravity_dir: (0.0, -1.0, 0.0), drag_force: 0.4),
            ],
            collider_groups: [0],
        )],
    )"#;
    let spring = prepare_spring(spring_text, &source).expect("spring resolves");
    let options = ExportOptions {
        expressions: None,
        spring: Some(&spring),
    };
    let out = export_vrm(&source, Some(&profile()), &options).expect("export");
    let report = validate_vrm(&out, Some(&profile())).expect("validates");
    assert_eq!(report.spring_chains, 1);
    let parsed = parse_glb(&out).expect("parses");
    let block = &parsed.json["extensions"]["VRMC_springBone"];
    assert_eq!(block["specVersion"], "1.0");
    assert_eq!(block["springs"][0]["joints"][0]["stiffness"], 1.0);
    let radius = block["colliders"][0]["shape"]["sphere"]["radius"]
        .as_f64()
        .expect("radius is a number");
    assert!((radius - 0.12).abs() < 1e-6, "radius {radius}");
}

/// Meta in the file equals the profile that wrote it, checked
/// by a reader that was not given the profile's cooperation:
/// validate without a profile still passes the spec-required
/// fields.
#[test]
fn export_validates_without_profile_too() {
    let source = fixture_glb(&[], None);
    let out = export_vrm(&source, Some(&profile()), &ExportOptions::default()).expect("export");
    let report = validate_vrm(&out, None).expect("validates");
    assert_eq!(report.meta_name, "Fixture Character");
}
