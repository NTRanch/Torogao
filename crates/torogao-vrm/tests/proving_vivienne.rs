//! The Vivienne proving case (D-654 pilot model), gated on
//! environment variables so the suite stays hermetic:
//!
//! - `TOROGAO_VRM_TEST_GLB`: path to the pilot GLB.
//! - `TOROGAO_VRM_TEST_EXPRESSIONS`: path to her governed
//!   expression map (optional).
//!
//! With the GLB unset the test reports itself skipped on
//! stderr and passes; the gate runs set the variables so the
//! proving counts.

use torogao_vrm::{ExportOptions, ExpressionMap, VrmProfile, export_vrm, validate_vrm};

/// The deliberate test profile shipped with the crate.
const TEST_PROFILE: &str = include_str!("../data/vivienne-test.vrm-profile.ron");

#[test]
fn vivienne_export_proving() {
    let Ok(glb_path) = std::env::var("TOROGAO_VRM_TEST_GLB") else {
        eprintln!("vivienne proving skipped: TOROGAO_VRM_TEST_GLB is unset");
        return;
    };
    let bytes = std::fs::read(&glb_path).expect("proving GLB reads");
    let profile = VrmProfile::load(TEST_PROFILE).expect("test profile checks");
    let map = std::env::var("TOROGAO_VRM_TEST_EXPRESSIONS")
        .ok()
        .map(|path| {
            let text = std::fs::read_to_string(path).expect("expression map reads");
            ExpressionMap::parse(&text).expect("expression map parses")
        });
    let options = ExportOptions {
        expressions: map.as_ref(),
        spring: None,
    };
    let out = export_vrm(&bytes, Some(&profile), &options).expect("export succeeds");
    let report = validate_vrm(&out, Some(&profile)).expect("read-back validates");

    // The pilot skeleton: 54 joints, every one resolving.
    assert_eq!(report.humanoid_bones.len(), 54);
    assert!(report.humanoid_bones.contains(&"hips".to_string()));
    assert!(
        report
            .humanoid_bones
            .contains(&"leftThumbMetacarpal".to_string())
    );
    // The pilot carries no spring data: the block is absent.
    assert_eq!(report.spring_chains, 0);
    // Meta is the test profile's, field for field (compare ran
    // inside validate); the name is the visible witness.
    assert_eq!(report.meta_name, "Vivienne (D-654 pilot) — TEST PROFILE");
    if map.is_some() {
        for preset in ["aa", "blink", "ee", "ih", "neutral", "oh", "ou"] {
            assert!(
                report.preset_expressions.contains(&preset.to_string()),
                "preset {preset} missing: {:?}",
                report.preset_expressions
            );
        }
        for custom in ["arousal", "joy", "smile", "trance"] {
            assert!(
                report.custom_expressions.contains(&custom.to_string()),
                "custom {custom} missing: {:?}",
                report.custom_expressions
            );
        }
    }
}
