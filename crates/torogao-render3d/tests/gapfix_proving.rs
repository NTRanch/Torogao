//! Gap-fix proving tests (2026-11-26): the StellarToon /
//! LSCherry grammar gap list (GAP-1..GAP-10) against the
//! toon model.
//!
//! Two fixtures:
//! - the programme's standard character fixture (Prima
//!   retrofit + contract channels, env
//!   TOROGAO_RENDER3D_TOON_FIXTURE): the parameter-table
//!   terms (GAP-1 SSS, GAP-2 rim colour, GAP-3 via the
//!   table), the shadow tint (GAP-6) and the calibration
//!   block (GAP-7) must move the frame materially, and
//!   the modified frame must stay twin-deterministic
//!   (D-1397).
//! - a synthetic quad authored in code with TEXCOORD_2
//!   (planar, GAP-4) and TEXCOORD_3 (shading normal,
//!   GAP-8): each channel must change the hair-ID frame
//!   when present, and a reversed-winding render must
//!   read as the per-ID back colour (GAP-5).
//!
//! Ignored like the other proving tests: they need a
//! Vulkan device.

use std::path::{Path, PathBuf};

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, RenderBackend, pack_scene};
use torogao_render3d::compare::sha256_hex;
use torogao_render3d::math::Mat4;
use torogao_render3d::model::{Material, Mesh, Model, Node, Skin, load_model};
use torogao_render3d::setscene::plate_rig_scene;
use torogao_render3d::toon::{
    ToonData, ToonImage, ToonMaterialParams, ToonParams, ToonSettings, ToonTextures,
};

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
    Mat4::from_rows([
        [right[0], up[0], back[0], eye[0]],
        [right[1], up[1], back[1], eye[1]],
        [right[2], up[2], back[2], eye[2]],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

fn draw_for(model: &Model) -> DrawFrame {
    DrawFrame {
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
    }
}

fn render(model: &Model, toon: ToonData, w: u32, h: u32) -> (Vec<u8>, Vec<u8>) {
    let scene = plate_rig_scene(camera_world());
    let mut packed = pack_scene(model, &scene);
    packed.toon = Some(toon);
    let draw = draw_for(model);
    let mut backend = VulkanBackend::new().unwrap();
    backend.upload_scene(&packed, w, h).unwrap();
    let a = backend.render_frame(&draw).unwrap();
    let b = backend.render_frame(&draw).unwrap();
    (a.rgba, b.rgba)
}

fn differing(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(x, y)| x != y)
        .count()
}

fn fixture_toon(manifest: &serde_json::Value, dir: &Path) -> ToonData {
    let tex = |name: &str| {
        let entry = &manifest["textures"][name];
        ToonImage::load_rgba(
            &dir.join(entry["file"].as_str().unwrap()),
            entry["width"].as_u64().unwrap() as u32,
            entry["height"].as_u64().unwrap() as u32,
        )
        .unwrap()
    };
    ToonData {
        textures: ToonTextures {
            packed: tex("packed"),
            ramp: tex("ramp"),
            face_sdf: tex("face_sdf"),
            sphere: tex("sphere"),
            params: None,
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
    }
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn gap_terms_move_the_fixture_frame_and_stay_deterministic() {
    let fixture =
        PathBuf::from(std::env::var("TOROGAO_RENDER3D_TOON_FIXTURE").expect("fixture dir env var"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.join("manifest.json")).unwrap()).unwrap();
    let bytes = std::fs::read(fixture.join("prima_toon.glb")).unwrap();
    let model = load_model(&bytes).unwrap();

    let base = fixture_toon(&manifest, &fixture);
    base.validate().unwrap();
    let (base_a, base_b) = render(&model, base.clone(), 256, 320);
    assert_eq!(sha256_hex(&base_a), sha256_hex(&base_b), "twin (defaults)");

    // Every gap term on at once: SSS + rim colour from
    // the table (GAP-1/2), shadow tint (GAP-6),
    // calibration (GAP-7).
    let mut params = ToonParams::default();
    for e in params.entries.iter_mut() {
        *e = ToonMaterialParams {
            sss_color: [0.85, 0.30, 0.10],
            sss_width: 0.35,
            rim_color: [0.35, 0.55, 1.0],
            ..ToonMaterialParams::default()
        };
    }
    let mut hot = base.clone();
    hot.textures.params = Some(params.to_image().unwrap());
    hot.settings.shadow_tint = [0.20, 0.10, 0.50];
    hot.settings.shadow_tint_factor = 0.8;
    hot.settings.exposure = 1.3;
    hot.validate().unwrap();
    let (hot_a, hot_b) = render(&model, hot, 256, 320);
    assert_eq!(sha256_hex(&hot_a), sha256_hex(&hot_b), "twin (gap terms)");
    let total = (256 * 320) as usize;
    let diff = differing(&base_a, &hot_a);
    assert!(
        diff * 20 > total,
        "gap terms changed only {diff}/{total} pixels"
    );
}

/// The synthetic hair quad: ID 2 everywhere (packed
/// alpha 34/255), planar along-length running 0 at the
/// top edge to 1 at the bottom.
fn quad_model(planar: bool, toon_normal: bool, reversed: bool) -> Model {
    // Facing the bust camera: plane z = 0 centred on the
    // framing target, normal toward -z (the camera side).
    let positions = vec![
        [-0.3, 1.48, 0.0],
        [0.3, 1.48, 0.0],
        [0.3, 0.88, 0.0],
        [-0.3, 0.88, 0.0],
    ];
    let normals = vec![[0.0, 0.0, -1.0]; 4];
    let texcoord2 = if planar {
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
    } else {
        vec![[0.0, 0.0]; 4]
    };
    let tn = if toon_normal {
        // Shading normal swung 60 degrees toward +x: the
        // ramp/spec/rim terms must follow it, not the
        // geometric (0,0,-1).
        vec![[0.866, 0.0, -0.5, 1.0]; 4]
    } else {
        vec![[0.0, 0.0, 0.0, 0.0]; 4]
    };
    let indices = if reversed {
        vec![0, 2, 1, 0, 3, 2]
    } else {
        vec![0, 1, 2, 0, 2, 3]
    };
    let mesh = Mesh {
        name: "SyntheticHair".to_string(),
        node: 0,
        material: 0,
        positions,
        normals: normals.clone(),
        color0: vec![[0.75, 0.55, 0.35, 1.0]; 4],
        color1: vec![[0.0; 4]; 4],
        has_color1: false,
        texcoord0: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        texcoord1: vec![[0.5, 0.5]; 4],
        has_texcoord1: true,
        texcoord2,
        has_texcoord2: planar,
        toon_normal: tn,
        has_toon_normal: toon_normal,
        outline_normal: normals,
        joints: vec![[0, 0, 0, 0]; 4],
        weights: vec![[1.0, 0.0, 0.0, 0.0]; 4],
        indices,
        targets: Vec::new(),
        target_names: Vec::new(),
        default_weights: Vec::new(),
    };
    Model {
        meshes: vec![mesh],
        nodes: vec![Node {
            name: "root".to_string(),
            parent: None,
            children: Vec::new(),
            local: identity(),
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            mesh: Some(0),
        }],
        skin: Skin {
            name: String::new(),
            joints: vec![0],
            inverse_bind: vec![identity()],
        },
        materials: vec![Material {
            name: "hair".to_string(),
            base_color: [1.0; 4],
            metallic: 0.0,
            roughness: 0.8,
            double_sided: true,
            alpha_overlay: false,
            base_color_image: None,
            albedo_slot: None,
        }],
        albedos: Vec::new(),
    }
}

fn synthetic_toon() -> ToonData {
    // Ramp: a smooth dark-to-light gradient in every
    // row so the quad reads its lighting term.
    let mut ramp = vec![0u8; 256 * 32 * 4];
    for y in 0..32usize {
        for x in 0..256usize {
            let i = (y * 256 + x) * 4;
            ramp[i] = (x / 2) as u8;
            ramp[i + 1] = (x / 2) as u8;
            ramp[i + 2] = (x / 2) as u8;
            ramp[i + 3] = 255;
        }
    }
    ToonData {
        textures: ToonTextures {
            // AO 1, spec mask 1, metal 0, ID 2 (hair).
            packed: ToonImage::solid(4, 4, [255, 255, 0, 34]).unwrap(),
            ramp: ToonImage::new(256, 32, ramp).unwrap(),
            face_sdf: ToonImage::solid(4, 4, [128, 0, 0, 0]).unwrap(),
            sphere: ToonImage::solid(4, 4, [90, 90, 90, 255]).unwrap(),
            params: None,
        },
        settings: ToonSettings {
            // No inverted hull on the synthetic quad: the
            // hull pass follows winding, and the GAP-5
            // case below is a winding case.
            outline_width_m: 0.0,
            ..ToonSettings::default()
        },
    }
}

/// Mean RGB over the pixels where two frames differ
/// (the quad region; the shared background cancels).
fn mean_rgb_over_diff(a: &[u8], b: &[u8], which: &[u8]) -> [f64; 3] {
    let mut acc = [0f64; 3];
    let mut n = 0f64;
    for (x, y) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        if x != y {
            let px = if std::ptr::eq(which, a) { x } else { y };
            acc[0] += f64::from(px[0]);
            acc[1] += f64::from(px[1]);
            acc[2] += f64::from(px[2]);
            n += 1.0;
        }
    }
    assert!(n > 100.0, "quad region too small: {n} pixels");
    [acc[0] / n, acc[1] / n, acc[2] / n]
}

#[test]
#[ignore = "needs a Vulkan device"]
fn planar_bands_toon_normal_and_back_face_move_the_synthetic_frame() {
    let toon = synthetic_toon();
    toon.validate().unwrap();

    let plain = quad_model(false, false, false);
    let (plain_a, plain_b) = render(&plain, toon.clone(), 256, 320);
    assert_eq!(sha256_hex(&plain_a), sha256_hex(&plain_b), "twin (plain)");

    // GAP-4: authoring TEXCOORD_2 must switch the band
    // term on.
    let banded = quad_model(true, false, false);
    let (banded_a, _) = render(&banded, toon.clone(), 256, 320);
    let total = (256 * 320) as usize;
    let diff = differing(&plain_a, &banded_a);
    assert!(
        diff * 50 > total,
        "planar bands changed only {diff}/{total} pixels"
    );

    // GAP-8: the authored shading normal must move the
    // shading terms off the geometric normal's answer.
    let swung = quad_model(false, true, false);
    let (swung_a, _) = render(&swung, toon.clone(), 256, 320);
    let diff = differing(&plain_a, &swung_a);
    assert!(
        diff * 50 > total,
        "toon normal changed only {diff}/{total} pixels"
    );

    // GAP-5: reversed winding renders the same quad as
    // back faces; its colour must move toward the
    // table's back colour (deep blue) relative to the
    // front render.
    let back = quad_model(false, false, true);
    let (back_a, _) = render(&back, toon, 256, 320);
    let front_mean = mean_rgb_over_diff(&plain_a, &back_a, &plain_a);
    let back_mean = mean_rgb_over_diff(&plain_a, &back_a, &back_a);
    assert!(
        front_mean[0] > front_mean[2],
        "front quad should read warm, got {front_mean:?}"
    );
    assert!(
        back_mean[2] > back_mean[0],
        "back-face quad mean {back_mean:?} is not blue-dominant (front {front_mean:?})"
    );
}
