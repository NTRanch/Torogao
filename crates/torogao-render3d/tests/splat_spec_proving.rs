//! Splat spec proving (programme Rung 1): a frozen
//! scene spec naming a splat layer renders through the
//! spec compiler (`torogao_render3d::splat_spec`) —
//! hash-verified asset, axis map, layer transform —
//! and the frame is BIT-IDENTICAL to the direct-packet
//! render of the same scene (the spike's route), and
//! twin-identical through the compiler twice. A spec
//! with no splat layer compiles to `None` and the
//! render is bit-identical to the character-only
//! render: the boundary property, through the compiler.
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
use torogao_render3d::splat::{SplatTransform, pack_splats, room_corner_cloud};
use torogao_render3d::splat_spec::compile_scene_splats;
use torogao_render3d::toon::{ToonData, ToonImage, ToonSettings, ToonTextures};

fn identity() -> Mat4 {
    Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

/// The spike proving test's wide framing, duplicated
/// by design — proving tests do not share helpers.
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

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn spec_driven_splat_layer_renders_like_the_direct_packet() {
    let fixture =
        PathBuf::from(std::env::var("TOROGAO_RENDER3D_TOON_FIXTURE").expect("fixture dir env var"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.join("manifest.json")).unwrap()).unwrap();
    let bytes = std::fs::read(fixture.join("prima_toon.glb")).unwrap();
    let model = load_model(&bytes).unwrap();
    let tex = |name: &str| {
        let entry = &manifest["textures"][name];
        ToonImage::load_rgba(
            &fixture.join(entry["file"].as_str().unwrap()),
            entry["width"].as_u64().unwrap() as u32,
            entry["height"].as_u64().unwrap() as u32,
        )
        .unwrap()
    };
    let f32s = |value: &serde_json::Value| -> [f32; 3] {
        let a = value.as_array().unwrap();
        [
            a[0].as_f64().unwrap() as f32,
            a[1].as_f64().unwrap() as f32,
            a[2].as_f64().unwrap() as f32,
        ]
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
            head_forward_local: f32s(&manifest["head"]["forward_local"]),
            head_right_local: f32s(&manifest["head"]["right_local"]),
            ..ToonSettings::default()
        },
    };
    toon.validate().unwrap();

    let (width, height) = (512_u32, 640_u32);
    let scene = plate_rig_scene(camera_world());
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

    // The frozen spec: the room cloud as a named,
    // hash-pinned asset.
    let asset_dir =
        std::env::temp_dir().join(format!("torogao-splat-spec-proving-{}", std::process::id()));
    std::fs::create_dir_all(&asset_dir).unwrap();
    let cloud = room_corner_cloud();
    let asset_bytes = cloud.write_ply();
    std::fs::write(asset_dir.join("room.ply"), &asset_bytes).unwrap();
    let spec = format!(
        r#"{{ "name": "rung1-proving",
             "layers": [ {{ "kind": "splat",
                 "asset": {{ "path": "room.ply", "sha256": "{}" }},
                 "transform": {{ "translation": [0.0, 0.0, 0.0], "scale": 1.0 }},
                 "axis": "gltf" }} ] }}"#,
        sha256_hex(&asset_bytes)
    );

    // Compiler path, twice (the twin).
    let mut compiled_frames = Vec::new();
    for _ in 0..2 {
        let layer = compile_scene_splats(spec.as_bytes(), &asset_dir, &scene, width, height)
            .expect("spec compiles")
            .expect("spec names a splat layer");
        let mut packed = pack_scene(&model, &scene);
        packed.toon = Some(toon.clone());
        packed.splats = Some(layer);
        compiled_frames.push(render(&packed, &draw, width, height));
    }
    assert_eq!(
        sha256_hex(&compiled_frames[0].rgba),
        sha256_hex(&compiled_frames[1].rgba),
        "compiler path is twin-identical"
    );
    save_evidence(
        "spec-splat-composite.png",
        &compiled_frames[0],
        width,
        height,
    );

    // Direct path (the spike's route): same cloud, same
    // placement, packed by hand.
    let mut packed = pack_scene(&model, &scene);
    packed.toon = Some(toon.clone());
    packed.splats = Some(pack_splats(
        &cloud,
        &SplatTransform::default(),
        &scene,
        width,
        height,
    ));
    let direct = render(&packed, &draw, width, height);
    assert_eq!(
        sha256_hex(&compiled_frames[0].rgba),
        sha256_hex(&direct.rgba),
        "spec-compiled render is bit-identical to the direct-packet render"
    );

    // Boundary through the compiler: a spec with no
    // splat layer compiles to None, and the render is
    // the character-only render, bit for bit.
    let empty_spec = r#"{ "name": "rung1-proving", "layers": [] }"#;
    let none = compile_scene_splats(empty_spec.as_bytes(), &asset_dir, &scene, width, height)
        .expect("empty spec compiles");
    assert!(none.is_none(), "no splat layer compiles to None");
    let mut packed = pack_scene(&model, &scene);
    packed.toon = Some(toon);
    let character_only = render(&packed, &draw, width, height);
    assert_ne!(
        sha256_hex(&character_only.rgba),
        sha256_hex(&direct.rgba),
        "the splat layer is really in the composite (guard against a vacuous pass)"
    );
    println!(
        "rung1 proving: composite sha {}, character-only sha {}",
        sha256_hex(&direct.rgba),
        sha256_hex(&character_only.rgba)
    );
}
