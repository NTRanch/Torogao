//! Family B proving test: a GLB whose material carries a
//! base-colour texture (an 8x8 four-quadrant PNG: TL red,
//! TR green, BL blue, BR white) must load, render with the
//! texture multiplying the albedo, and render
//! deterministically. The quadrant centroids pin the
//! texture orientation empirically: each quadrant must
//! land on the quad corner whose UV names it (this camera
//! views the +Z-facing quad from -Z, so u = 0 appears on
//! the frame's right — the pin is the UV-to-corner
//! correspondence, not a screen corner).
//!
//! Ignored like the other proving tests: it needs a
//! Vulkan device and the fixture directory in
//! TOROGAO_RENDER3D_FAMILYB_FIXTURE (generator:
//! make_fixture.py in that directory).

use std::path::PathBuf;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, RenderBackend, pack_scene};
use torogao_render3d::compare::sha256_hex;
use torogao_render3d::math::Mat4;
use torogao_render3d::model::load_model;
use torogao_render3d::setscene::plate_rig_scene;

fn identity() -> Mat4 {
    Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

/// The toon fixture's bust framing, reused so the quad
/// (centred at (0, 1.18, 0)) fills the frame the same way.
fn camera_world() -> Mat4 {
    let eye = [0.85, 1.42, -1.15];
    let target = [0.0, 1.18, 0.0];
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let norm = |a: [f64; 3]| {
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let forward = norm(sub(target, eye));
    let right = norm(cross(forward, [0.0, 1.0, 0.0]));
    let up = cross(right, forward);
    let back = [-forward[0], -forward[1], -forward[2]];
    Mat4::from_rows([
        [right[0], up[0], back[0], eye[0]],
        [right[1], up[1], back[1], eye[1]],
        [right[2], up[2], back[2], eye[2]],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

const W: u32 = 256;
const H: u32 = 320;

fn render(glb: &std::path::Path) -> (Vec<u8>, torogao_render3d::model::Model) {
    let bytes = std::fs::read(glb).unwrap();
    let model = load_model(&bytes).unwrap();
    let scene = plate_rig_scene(camera_world());
    let packed = pack_scene(&model, &scene);
    let draw = DrawFrame {
        frame: 0,
        joint_matrices: (0..model.skin.joints.len())
            .map(|_| identity().to_f32_cols())
            .collect(),
        mesh_weights: model
            .meshes
            .iter()
            .map(|m| m.default_weights.clone())
            .collect(),
        visible_meshes: vec![true; model.meshes.len()],
        include_set: false,
        toy_model: identity(),
    };
    let mut backend = VulkanBackend::new().unwrap();
    backend.upload_scene(&packed, W, H).unwrap();
    let frame = backend.render_frame(&draw).unwrap();
    (frame.rgba, model)
}

/// Centroid (x, y) of the pixels in `rgba` matching `class`,
/// restricted to the `mask` pixel set.
fn centroid(rgba: &[u8], mask: &[bool], class: impl Fn(u8, u8, u8) -> bool) -> Option<(f64, f64)> {
    let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0usize);
    for (i, px) in rgba.chunks_exact(4).enumerate() {
        if mask[i] && class(px[0], px[1], px[2]) {
            sx += (i as u32 % W) as f64;
            sy += (i as u32 / W) as f64;
            n += 1;
        }
    }
    (n > 20).then(|| (sx / n as f64, sy / n as f64))
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_FAMILYB_FIXTURE"]
fn textured_material_loads_renders_and_is_deterministic() {
    let fixture = PathBuf::from(
        std::env::var("TOROGAO_RENDER3D_FAMILYB_FIXTURE").expect("fixture dir env var"),
    );

    // Loader level: the textured GLB resolves a shared
    // albedo; row 0 of the decode is the PNG's top row
    // (red quadrant first texel). The untextured twin
    // resolves none.
    let (textured, model) = render(&fixture.join("familyb_quad.glb"));
    assert_eq!(model.albedos.len(), 1, "one shared atlas = one albedo slot");
    let albedo = &model.albedos[0];
    assert_eq!((albedo.width, albedo.height), (8, 8));
    assert_eq!(
        &albedo.rgba[0..4],
        &[255, 0, 0, 255],
        "row 0 starts with red"
    );
    let (plain, plain_model) = render(&fixture.join("familyb_quad_untextured.glb"));
    assert!(plain_model.albedos.is_empty());

    // Determinism witness (D-1397).
    let (twin, _) = render(&fixture.join("familyb_quad.glb"));
    assert_eq!(
        sha256_hex(&textured),
        sha256_hex(&twin),
        "twin textured renders must be bit-identical"
    );

    // The texture must actually reach the frame: the quad
    // is exactly the set of pixels that changed vs the
    // untextured render.
    let changed: Vec<bool> = textured
        .chunks_exact(4)
        .zip(plain.chunks_exact(4))
        .map(|(a, b)| a != b)
        .collect();
    let changed_count = changed.iter().filter(|c| **c).count();
    assert!(
        changed_count * 5 > (W * H) as usize,
        "textured render differs from untextured in only {changed_count} pixels"
    );

    // Orientation pin, stated as pairwise orderings so it
    // does not depend on the camera's handedness: this
    // camera's right vector is world -X (it views the
    // +Z-facing quad from -Z), so the quad's u = 0 side
    // (world -X) appears on the frame's right, and v = 0
    // (the quad's top) appears above v = 1. Measured on
    // the first run: red centroid (178.2, 86.9) — right
    // and top, exactly the (0,0) corner's projection.
    let red = centroid(&textured, &changed, |r, g, b| {
        r > g.saturating_add(30) && r > b.saturating_add(30)
    })
    .expect("red quadrant present");
    let green = centroid(&textured, &changed, |r, g, b| {
        g > r.saturating_add(30) && g > b.saturating_add(30)
    })
    .expect("green quadrant present");
    let blue = centroid(&textured, &changed, |r, g, b| {
        b > r.saturating_add(30) && b > g.saturating_add(30)
    })
    .expect("blue quadrant present");
    let white = centroid(&textured, &changed, |r, g, b| {
        r > 110 && g > 110 && b > 110 && r.abs_diff(g) < 45 && g.abs_diff(b) < 45
    })
    .expect("white quadrant present");
    // u axis: red (u<0.5) right of green (u>0.5); blue
    // (u<0.5) right of white (u>0.5).
    assert!(
        red.0 > green.0 && blue.0 > white.0,
        "u order wrong: red {red:?}, green {green:?}, blue {blue:?}, white {white:?}"
    );
    // v axis: red/green (v<0.5) above blue/white (v>0.5).
    assert!(
        red.1 < blue.1 && green.1 < white.1,
        "v order wrong: red {red:?}, green {green:?}, blue {blue:?}, white {white:?}"
    );
}
