//! Splat rotation proving (programme Rung 3, gate 3):
//! a cloud rotated by R and rendered from a camera
//! rotated by the same R matches the unrotated render
//! up to rasterisation resampling — measured and
//! reported, the residual named (a view-equivalence
//! check, not a bit-identity claim). The rotated path
//! also renders twin-identical (the R6 witness on the
//! new path). Two subjects: the procedural room cloud
//! (geometry + covariance rotation) and a synthetic
//! cloud with strong SH bands 1–3 coefficients (the
//! colour path through the real renderer — the room
//! cloud is DC-only).
//!
//! Ignored like the other proving tests: it needs a
//! Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE.

use std::path::PathBuf;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, PackedScene, RenderBackend, RenderedFrame, pack_scene};
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::math::Mat4;
use torogao_render3d::model::load_model;
use torogao_render3d::setscene::plate_rig_scene;
use torogao_render3d::splat::{Splat, SplatCloud, SplatTransform, pack_splats, room_corner_cloud};
use torogao_render3d::splat_rotate::Rotation;

fn identity() -> Mat4 {
    Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

/// The proving tests' wide framing, duplicated by
/// design — proving tests do not share helpers.
fn camera_world() -> Mat4 {
    let eye = [2.6, 1.9, -3.4];
    let target = [0.0, 0.9, 0.0];
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

fn render(packed: &PackedScene, draw: &DrawFrame, width: u32, height: u32) -> RenderedFrame {
    let mut backend = VulkanBackend::new().expect("backend");
    backend.upload_scene(packed, width, height).expect("upload");
    backend.render_frame(draw).expect("render")
}

fn save_evidence(name: &str, frame: &RenderedFrame, width: u32, height: u32) {
    if let Ok(dir) = std::env::var("TOROGAO_RENDER3D_SPLAT_OUT") {
        let image = Image::new(width, height, frame.rgba.clone()).expect("evidence image");
        let path = PathBuf::from(dir).join(name);
        write_png(&path, &image).expect("write evidence png");
        println!("evidence: {}", path.display());
    }
}

/// A small cloud whose colour is dominated by SH
/// bands 1–3 (dc = 0 → the 0.5 base plus the rest
/// field), placed around the proving camera's target.
fn sh_cloud() -> SplatCloud {
    let mut rest_a = [0.0_f32; 45];
    rest_a[0] = 0.9; // band 1, ch R
    rest_a[3 + 1] = -0.8; // band 2, ch R
    rest_a[15 + 2] = 0.85; // band 1, ch G
    rest_a[15 + 8 + 4] = 0.7; // band 3, ch G
    rest_a[30 + 1] = -0.9; // band 1, ch B
    rest_a[30 + 8] = 0.75; // band 3, ch B
    let mut rest_b = [0.0_f32; 45];
    rest_b[2] = -0.85;
    rest_b[15 + 3] = 0.8;
    rest_b[30 + 8 + 6] = -0.7;
    let splat = |pos: [f32; 3], rest: [f32; 45], log_scale: [f32; 3], rot: [f32; 4]| Splat {
        pos,
        dc: [0.0; 3],
        rest,
        opacity_logit: 3.476, // sigmoid ≈ 0.97
        log_scale,
        rot,
    };
    let s = 0.32_f32.ln();
    SplatCloud {
        splats: vec![
            splat([0.0, 0.9, 0.0], rest_a, [s, s, s], [1.0, 0.0, 0.0, 0.0]),
            splat([0.55, 1.25, 0.3], rest_b, [s, s, s], [1.0, 0.0, 0.0, 0.0]),
            splat([-0.55, 0.55, 0.3], rest_a, [s, s, s], [1.0, 0.0, 0.0, 0.0]),
            splat([0.2, 0.9, -0.55], rest_b, [s, s, s], [1.0, 0.0, 0.0, 0.0]),
            // Anisotropic, pre-rotated: covariance
            // rotation is exercised, not just centres.
            splat(
                [-0.2, 1.35, -0.3],
                rest_b,
                [0.45_f32.ln(), 0.12_f32.ln(), 0.3_f32.ln()],
                [
                    std::f32::consts::FRAC_1_SQRT_2,
                    0.0,
                    0.0,
                    std::f32::consts::FRAC_1_SQRT_2,
                ],
            ),
        ],
    }
}

struct Diff {
    any: usize,
    gt1: usize,
    max_delta: u32,
    mean_abs: f64,
}

fn diff(a: &RenderedFrame, b: &RenderedFrame) -> Diff {
    assert_eq!(a.rgba.len(), b.rgba.len());
    let px = a.rgba.len() / 4;
    let mut d = Diff {
        any: 0,
        gt1: 0,
        max_delta: 0,
        mean_abs: 0.0,
    };
    let mut sum = 0_u64;
    for i in 0..px {
        let mut px_max = 0_u32;
        for c in 0..3 {
            let delta = (i32::from(a.rgba[i * 4 + c]) - i32::from(b.rgba[i * 4 + c])).abs();
            px_max = px_max.max(delta as u32);
            sum += delta as u64;
        }
        if px_max > 0 {
            d.any += 1;
        }
        if px_max > 1 {
            d.gt1 += 1;
        }
        d.max_delta = d.max_delta.max(px_max);
    }
    d.mean_abs = sum as f64 / (px * 3) as f64;
    d
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn rotated_cloud_from_rotated_camera_matches_unrotated_render() {
    let fixture =
        PathBuf::from(std::env::var("TOROGAO_RENDER3D_TOON_FIXTURE").expect("fixture dir env var"));
    let bytes = std::fs::read(fixture.join("prima_toon.glb")).unwrap();
    let model = load_model(&bytes).unwrap();

    let (width, height) = (512_u32, 640_u32);

    // R: 37° about (1, 2, 3)/√14 — a general rotation,
    // not an axis special case.
    let axis_len = 14.0_f64.sqrt();
    let axis = [1.0 / axis_len, 2.0 / axis_len, 3.0 / axis_len];
    let half = (37.0_f64.to_radians()) / 2.0;
    let quat = [
        half.cos(),
        axis[0] * half.sin(),
        axis[1] * half.sin(),
        axis[2] * half.sin(),
    ];
    let rotation = Rotation::from_quaternion(quat).expect("unit quaternion");
    let m = rotation.matrix;
    let r4 = Mat4::from_rows([
        [m[0][0], m[0][1], m[0][2], 0.0],
        [m[1][0], m[1][1], m[1][2], 0.0],
        [m[2][0], m[2][1], m[2][2], 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    let cam_a = camera_world();
    let cam_b = r4.multiply(cam_a);
    let scene_a = plate_rig_scene(cam_a);
    let scene_b = plate_rig_scene(cam_b);

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
        visible_meshes: vec![false; model.meshes.len()],
        include_set: false,
        toy_model: identity(),
    };

    for (name, cloud) in [("room", room_corner_cloud()), ("sh", sh_cloud())] {
        let mut packed_a = pack_scene(&model, &scene_a);
        packed_a.splats = Some(pack_splats(
            &cloud,
            &SplatTransform::default(),
            &scene_a,
            width,
            height,
        ));
        let frame_a = render(&packed_a, &draw, width, height);
        save_evidence(
            &format!("rung3-{name}-unrotated.png"),
            &frame_a,
            width,
            height,
        );

        let rotated_transform = SplatTransform {
            translation: [0.0; 3],
            rotation: quat,
            scale: 1.0,
        };
        let mut frames_b = Vec::new();
        for _ in 0..2 {
            let mut packed_b = pack_scene(&model, &scene_b);
            packed_b.splats = Some(pack_splats(
                &cloud,
                &rotated_transform,
                &scene_b,
                width,
                height,
            ));
            frames_b.push(render(&packed_b, &draw, width, height));
        }
        assert_eq!(
            sha256_hex(&frames_b[0].rgba),
            sha256_hex(&frames_b[1].rgba),
            "{name}: rotated path is twin-identical"
        );
        save_evidence(
            &format!("rung3-{name}-rotated.png"),
            &frames_b[0],
            width,
            height,
        );

        let d = diff(&frame_a, &frames_b[0]);
        let px = (width * height) as usize;
        println!(
            "rung3 gate3 [{name}]: {} px; differ-any {} ({:.3}%), differ->1 {} ({:.4}%), \
             max delta {}, mean |delta| {:.4}",
            px,
            d.any,
            100.0 * d.any as f64 / px as f64,
            d.gt1,
            100.0 * d.gt1 as f64 / px as f64,
            d.max_delta,
            d.mean_abs
        );
        assert!(
            (d.gt1 as f64 / px as f64) < 0.005,
            "{name}: >1-LSB residual on {:.4}% of pixels (bound 0.5%) — beyond rasterisation resampling",
            100.0 * d.gt1 as f64 / px as f64
        );
        assert!(
            d.max_delta <= 6,
            "{name}: max channel delta {} exceeds the named residual bound 6",
            d.max_delta
        );
    }
}
