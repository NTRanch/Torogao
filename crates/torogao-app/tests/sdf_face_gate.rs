//! Gate tests for the SDF face material (T-11 Torogao half):
//! calibration discipline, the threshold shading reference, the
//! range-debug colours, and the shader interface contract.

use torogao_app::sdf_face::{
    LIGHT_HEIGHT, SDF_FACE_SHADER_WGSL, SdfFaceCalibration, SdfFaceUniforms, debug_colour,
    light_side, mirrored_u, sdf_lit_factor, shade_face,
};

const EPSILON: f32 = 1e-6;

#[test]
fn calibration_accepts_ordered_triples_and_refuses_the_rest() {
    let calibration = SdfFaceCalibration::new(0.2, 0.5, 0.8).expect("ordered triple");
    assert_eq!(calibration.min, 0.2);
    assert_eq!(calibration.mid, 0.5);
    assert_eq!(calibration.max, 0.8);
    // Unordered, out of range, and non-finite all refuse.
    assert!(SdfFaceCalibration::new(0.6, 0.5, 0.8).is_err());
    assert!(SdfFaceCalibration::new(0.2, 0.5, 0.4).is_err());
    assert!(SdfFaceCalibration::new(-0.1, 0.5, 0.8).is_err());
    assert!(SdfFaceCalibration::new(0.2, f32::NAN, 0.8).is_err());
}

#[test]
fn calibration_normalizes_the_painted_range() {
    let calibration = SdfFaceCalibration::new(0.2, 0.5, 0.8).expect("ordered triple");
    assert!((calibration.normalize(0.2) - 0.0).abs() < EPSILON);
    assert!((calibration.normalize(0.8) - 1.0).abs() < EPSILON);
    assert!((calibration.normalize(0.5) - 0.5).abs() < EPSILON);
    // Outside the calibrated range clamps, never extrapolates.
    assert!((calibration.normalize(0.0) - 0.0).abs() < EPSILON);
    assert!((calibration.normalize(1.0) - 1.0).abs() < EPSILON);
    // A degenerate range maps to the mid position.
    let degenerate = SdfFaceCalibration::new(0.5, 0.5, 0.5).expect("degenerate triple");
    assert!((degenerate.normalize(0.9) - 0.5).abs() < EPSILON);
}

#[test]
fn light_side_is_signed_and_sinusoidal() {
    // Frontal light: no sweep, either side.
    assert!((light_side(0.0, 0.0) - 0.0).abs() < EPSILON);
    // Full side light, each sign.
    assert!((light_side(std::f32::consts::FRAC_PI_2, 0.0) - 1.0).abs() < EPSILON);
    assert!((light_side(-std::f32::consts::FRAC_PI_2, 0.0) + 1.0).abs() < EPSILON);
    // Only the difference between azimuth and facing reads.
    assert!((light_side(1.0, 1.0) - 0.0).abs() < EPSILON);
    // Mirroring follows the sign: negative side flips the sample.
    assert_eq!(mirrored_u(0.25, 0.5), 0.25);
    assert_eq!(mirrored_u(0.25, -0.5), 0.75);
    assert_eq!(mirrored_u(0.25, 0.0), 0.25);
}

#[test]
fn lit_factor_ramps_across_the_painted_threshold() {
    let calibration = SdfFaceCalibration::identity();
    // Painted threshold 0.5: frontal light lights it, a sweep of
    // exactly 0.5 halves it, a full sweep shadows it.
    assert_eq!(sdf_lit_factor(0.5, 0.0, calibration), 1.0);
    assert!((sdf_lit_factor(0.5, 0.5, calibration) - 0.5).abs() < EPSILON);
    assert_eq!(sdf_lit_factor(0.5, 1.0, calibration), 0.0);
    // The sweep magnitude is what reads: the sign only mirrors.
    assert_eq!(sdf_lit_factor(0.5, -1.0, calibration), 0.0);
    assert_eq!(
        sdf_lit_factor(0.5, -0.5, calibration),
        sdf_lit_factor(0.5, 0.5, calibration)
    );
    // A texel painted at the range floor sits exactly at its
    // threshold under frontal light.
    assert_eq!(sdf_lit_factor(0.0, 0.0, calibration), 0.5);
}

#[test]
fn shade_face_is_a_two_band_cel() {
    let calibration = SdfFaceCalibration::identity();
    let base = [0.8, 0.6, 0.4];
    // Painted high (holds light) under a small sweep: lit.
    let lit = shade_face(base, 0.9, 0.1, calibration, false);
    assert_eq!(lit, base);
    // Painted low (shadows early) under a deep sweep: shadow band.
    let shadow = shade_face(base, 0.3, 0.9, calibration, false);
    assert!((shadow[0] - base[0] * 0.55).abs() < EPSILON);
    assert!((shadow[1] - base[1] * 0.55).abs() < EPSILON);
    assert!((shadow[2] - base[2] * 0.55).abs() < EPSILON);
}

#[test]
fn debug_colours_follow_the_three_number_workflow() {
    let calibration = SdfFaceCalibration::new(0.2, 0.5, 0.8).expect("ordered triple");
    assert_eq!(debug_colour(0.1, calibration), [1.0, 0.0, 0.0]); // below min: red
    assert_eq!(debug_colour(0.9, calibration), [0.0, 0.0, 1.0]); // above max: blue
    assert_eq!(debug_colour(0.5, calibration), [0.0, 1.0, 0.0]); // at mid: green
    assert_eq!(debug_colour(0.35, calibration), [0.35, 0.35, 0.35]); // between: grey
    // Debug mode in shade_face ignores base and light entirely.
    assert_eq!(
        shade_face([0.9, 0.9, 0.9], 0.1, 1.0, calibration, true),
        [1.0, 0.0, 0.0]
    );
}

#[test]
fn uniforms_pack_calibration_light_and_constants() {
    let calibration = SdfFaceCalibration::new(0.2, 0.5, 0.8).expect("ordered triple");
    let uniforms = SdfFaceUniforms::new(calibration, 0.75, true);
    assert_eq!(uniforms.calibration, [0.2, 0.5, 0.8, 1.0]);
    assert_eq!(uniforms.light[0], 0.75);
    assert_eq!(uniforms.light[1], LIGHT_HEIGHT);
    assert_eq!(uniforms.light[2], 0.55);
    assert_eq!(uniforms.light[3], 0.04);
    // The one light height is a material constant, pinned here.
    assert_eq!(LIGHT_HEIGHT, 0.35);
}

#[test]
fn shader_carries_the_material_interface() {
    for needle in [
        "SdfFaceUniforms",
        "sdf_texture",
        "base_texture",
        "vs_main",
        "fs_main",
        "calibration",
    ] {
        assert!(
            SDF_FACE_SHADER_WGSL.contains(needle),
            "shader missing {needle}"
        );
    }
}
