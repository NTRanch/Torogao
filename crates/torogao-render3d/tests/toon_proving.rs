//! G1 toon-model proving test: renders the parity
//! fixture (Prima retrofit + contract channels) with the
//! toon model on and off and checks the two frames differ
//! materially, then renders the toon frame a second time
//! and checks the twin-render determinism witness
//! (D-1397: identical inputs, bit-identical frame).
//!
//! Ignored like the other proving tests: it needs a
//! Vulkan device and the fixture directory in
//! TOROGAO_RENDER3D_TOON_FIXTURE.

use std::path::PathBuf;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, RenderBackend, pack_scene};
use torogao_render3d::compare::sha256_hex;
use torogao_render3d::math::Mat4;
use torogao_render3d::model::load_model;
use torogao_render3d::setscene::plate_rig_scene;
use torogao_render3d::toon::{ToonData, ToonImage, ToonSettings, ToonTextures};

fn identity() -> Mat4 {
    Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

/// Camera world matrix for the fixture's bust framing
/// (eye (0.85, 1.42, -1.15) toward (0, 1.18, 0)).
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
    // Camera world: basis columns + eye as translation.
    Mat4::from_rows([
        [right[0], up[0], back[0], eye[0]],
        [right[1], up[1], back[1], eye[1]],
        [right[2], up[2], back[2], eye[2]],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn toon_model_renders_and_is_deterministic() {
    let fixture =
        PathBuf::from(std::env::var("TOROGAO_RENDER3D_TOON_FIXTURE").expect("fixture dir env var"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.join("manifest.json")).unwrap()).unwrap();
    let bytes = std::fs::read(fixture.join("prima_toon.glb")).unwrap();
    let model = load_model(&bytes).unwrap();
    // The fixture GLB must carry the contract channels.
    assert!(
        model.meshes.iter().all(|m| m.has_texcoord1),
        "every fixture mesh carries TEXCOORD_1"
    );
    let tex = |name: &str| {
        let entry = &manifest["textures"][name];
        ToonImage::load_rgba(
            &fixture.join(entry["file"].as_str().unwrap()),
            entry["width"].as_u64().unwrap() as u32,
            entry["height"].as_u64().unwrap() as u32,
        )
        .unwrap()
    };
    let toon = ToonData {
        textures: ToonTextures {
            packed: tex("packed"),
            ramp: tex("ramp"),
            face_sdf: tex("face_sdf"),
            sphere: tex("sphere"),
        },
        settings: ToonSettings {
            head_joint: manifest["head"]["joint_index"].as_i64().unwrap() as i32,
            head_forward_local: manifest["head"]["forward_local"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            head_right_local: manifest["head"]["right_local"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            ..ToonSettings::default()
        },
    };
    toon.validate().unwrap();

    let scene = plate_rig_scene(camera_world());
    let mut packed = pack_scene(&model, &scene);
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
    backend.upload_scene(&packed, 256, 320).unwrap();
    let legacy = backend.render_frame(&draw).unwrap();

    packed.toon = Some(toon);
    let mut backend = VulkanBackend::new().unwrap();
    backend.upload_scene(&packed, 256, 320).unwrap();
    let toon_a = backend.render_frame(&draw).unwrap();
    let toon_b = backend.render_frame(&draw).unwrap();

    // Determinism witness (D-1397).
    assert_eq!(
        sha256_hex(&toon_a.rgba),
        sha256_hex(&toon_b.rgba),
        "twin toon renders must be bit-identical"
    );
    // The toon model must actually change the image:
    // at least 5% of pixels differ from the legacy path.
    let differing = legacy
        .rgba
        .chunks_exact(4)
        .zip(toon_a.rgba.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    let total = (256 * 320) as usize;
    assert!(
        differing * 20 > total,
        "toon frame differs from legacy in only {differing}/{total} pixels"
    );
}
