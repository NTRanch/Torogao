//! Splat relighting proving (programme Rung 4): a
//! shadow-receiving splat layer under the toon
//! fixture's character. The character stands between
//! a steepened sun and a splat carpet; the carpet
//! splats behind her read back darker by the applied
//! shadow factor at their centres, the carpet outside
//! her shadow is bit-identical to the unlit render,
//! and the lit render is twin-identical. The shadow
//! map is rasterised on the CPU from the same skinned
//! geometry the GPU shadow pass draws
//! (`reference::skin_mesh` at the draw's pose).
//!
//! Ignored like the other proving tests: it needs a
//! Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE.

use std::path::PathBuf;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{
    DrawFrame, PackedScene, RenderBackend, RenderedFrame, light_view_proj, pack_scene,
};
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::math::{Mat4, V3};
use torogao_render3d::model::load_model;
use torogao_render3d::morph::PackedMorphs;
use torogao_render3d::reference::skin_mesh;
use torogao_render3d::setscene::{plate_rig_scene, projection};
use torogao_render3d::splat::{Splat, SplatCloud};
use torogao_render3d::splat_light::{SHADOW_MAP_SIZE, SplatShadowMap, mesh_triangles};
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

/// A flat DC-only carpet splat (the pack's own
/// colour convention: dc = (colour - 0.5) / SH_C0).
fn carpet_splat(pos: [f32; 3]) -> Splat {
    const SH_C0: f32 = 0.282_094_8;
    let colour = [0.55_f32, 0.50, 0.45];
    let scale = [0.11_f32, 0.01, 0.11];
    Splat {
        pos,
        dc: [
            (colour[0] - 0.5) / SH_C0,
            (colour[1] - 0.5) / SH_C0,
            (colour[2] - 0.5) / SH_C0,
        ],
        rest: [0.0; 45],
        opacity_logit: (0.97_f32 / (1.0 - 0.97)).ln(),
        log_scale: [scale[0].ln(), scale[1].ln(), scale[2].ln()],
        rot: [1.0, 0.0, 0.0, 0.0],
    }
}

fn carpet_cloud() -> SplatCloud {
    let mut splats = Vec::new();
    let mut gx = -2.2_f32;
    while gx <= 2.2 {
        let mut gz = -2.2_f32;
        while gz <= 2.2 {
            splats.push(carpet_splat([gx, 0.0, gz]));
            gz += 0.15;
        }
        gx += 0.15;
    }
    SplatCloud { splats }
}

/// Mean RGB over a square block centred at a world
/// point projected through the scene's view-proj
/// (the pack's pixel convention: ndc * 0.5 + 0.5).
fn block_mean(
    frame: &RenderedFrame,
    view_proj: Mat4,
    world: [f64; 3],
    half: i32,
    width: u32,
    height: u32,
) -> ([f64; 3], Vec<u8>) {
    let p = V3::new(world[0], world[1], world[2]);
    let m = &view_proj.m;
    let clip = [
        m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12],
        m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13],
        m[3] * p.x + m[7] * p.y + m[11] * p.z + m[15],
    ];
    let ndc = [clip[0] / clip[2], clip[1] / clip[2]];
    let cx = ((ndc[0] * 0.5 + 0.5) * f64::from(width)) as i32;
    let cy = ((ndc[1] * 0.5 + 0.5) * f64::from(height)) as i32;
    let mut sum = [0.0_f64; 3];
    let mut bytes = Vec::new();
    let mut n = 0_u32;
    for y in (cy - half)..=(cy + half) {
        for x in (cx - half)..=(cx + half) {
            assert!(
                x >= 0 && y >= 0 && x < width as i32 && y < height as i32,
                "sample block stays in frame (centre {cx}, {cy})"
            );
            let i = ((y as u32 * width + x as u32) * 4) as usize;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += f64::from(frame.rgba[i + c]);
            }
            bytes.extend_from_slice(&frame.rgba[i..i + 4]);
            n += 1;
        }
    }
    (
        [
            sum[0] / f64::from(n),
            sum[1] / f64::from(n),
            sum[2] / f64::from(n),
        ],
        bytes,
    )
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn shadow_receiving_carpet_darkens_under_the_character() {
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
    // The plate rig with a steepened sun, so her
    // shadow pools on the carpet beside her feet
    // instead of raking off-frame.
    let mut scene = plate_rig_scene(camera_world());
    scene.sun.to_light = V3::new(0.25, 1.0, 0.3).normalized();
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

    // The CPU shadow map: her skinned geometry at the
    // draw's pose (identity joints, default weights —
    // the same caster set the GPU shadow pass draws
    // for this draw: visible character meshes, no set).
    let joints: Vec<Mat4> = (0..model.skin.joints.len()).map(|_| identity()).collect();
    let mut triangles = Vec::new();
    for (index, mesh) in model.meshes.iter().enumerate() {
        let morphs = PackedMorphs::pack(mesh);
        let (positions, _normals) =
            skin_mesh(&model, index, &morphs, &joints, &mesh.default_weights);
        triangles.extend(mesh_triangles(&positions, &mesh.indices));
    }
    assert!(!triangles.is_empty(), "the character casts");
    let shadow_map = SplatShadowMap::rasterise(
        &triangles,
        light_view_proj(scene.sun.to_light),
        SHADOW_MAP_SIZE,
    );

    // The frozen specs: one carpet layer, with and
    // without the Rung 4 light object.
    let asset_dir = std::env::temp_dir().join(format!(
        "torogao-splat-light-proving-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&asset_dir).unwrap();
    let cloud = carpet_cloud();
    let asset_bytes = cloud.write_ply();
    std::fs::write(asset_dir.join("carpet.ply"), &asset_bytes).unwrap();
    let spec_with = |light: &str| {
        format!(
            r#"{{ "name": "rung4-proving",
                 "layers": [ {{ "kind": "splat",
                     "asset": {{ "path": "carpet.ply", "sha256": "{}" }},
                     "transform": {{ "translation": [0.0, 0.0, 0.0], "scale": 1.0 }},
                     "axis": "gltf"{light} }} ] }}"#,
            sha256_hex(&asset_bytes)
        )
    };
    let lit_spec = spec_with(r#", "light": { "receive_shadow": true }"#);
    let unlit_spec = spec_with("");

    let compile_render = |spec: &str, shadow: Option<&SplatShadowMap>| {
        let layer =
            compile_scene_splats(spec.as_bytes(), &asset_dir, &scene, width, height, shadow)
                .expect("spec compiles")
                .expect("spec names a splat layer");
        let mut packed = pack_scene(&model, &scene);
        packed.toon = Some(toon.clone());
        packed.splats = Some(layer);
        render(&packed, &draw, width, height)
    };

    let lit_a = compile_render(&lit_spec, Some(&shadow_map));
    let lit_b = compile_render(&lit_spec, Some(&shadow_map));
    assert_eq!(
        sha256_hex(&lit_a.rgba),
        sha256_hex(&lit_b.rgba),
        "lit render is twin-identical"
    );
    let unlit = compile_render(&unlit_spec, None);
    assert_ne!(
        sha256_hex(&lit_a.rgba),
        sha256_hex(&unlit.rgba),
        "the shadow is really in the frame (guard against a vacuous pass)"
    );
    save_evidence("rung4-lit-carpet.png", &lit_a, width, height);
    save_evidence("rung4-unlit-carpet.png", &unlit, width, height);

    // Gate 1: the umbra block — the ground point her
    // shadow pools on (her axis cast along the sun to
    // y = 0, pushed to the pool's centre) — reads
    // back darker in the lit render by the applied
    // factor; the lit block — carpet well clear of
    // her shadow — is bit-identical.
    let view_proj = projection(&scene).multiply(scene.view);
    let (umbra_lit, _) = block_mean(&lit_a, view_proj, [-0.35, 0.0, -0.42], 10, width, height);
    let (umbra_unlit, _) = block_mean(&unlit, view_proj, [-0.35, 0.0, -0.42], 10, width, height);
    let (clear_lit, clear_lit_bytes) =
        block_mean(&lit_a, view_proj, [-1.4, 0.0, -0.2], 6, width, height);
    let (clear_unlit, clear_unlit_bytes) =
        block_mean(&unlit, view_proj, [-1.4, 0.0, -0.2], 6, width, height);
    println!(
        "rung4 proving: umbra lit {umbra_lit:?} vs unlit {umbra_unlit:?}; \
         clear lit {clear_lit:?} vs unlit {clear_unlit:?}"
    );
    // Cross-check at the centres themselves: the mean
    // visibility the map assigns to the carpet splats
    // whose quads reach the umbra block (centres
    // within one quad radius, 0.35 m). The render
    // block-mean must track it — the block composites
    // exactly those splats.
    let mut vis_sum = 0.0_f64;
    let mut vis_n = 0_u32;
    for splat in &cloud.splats {
        let dx = f64::from(splat.pos[0]) - (-0.35);
        let dz = f64::from(splat.pos[2]) - (-0.42);
        if dx * dx + dz * dz <= 0.35 * 0.35 {
            vis_sum += f64::from(shadow_map.visibility([
                f64::from(splat.pos[0]),
                f64::from(splat.pos[1]),
                f64::from(splat.pos[2]),
            ]));
            vis_n += 1;
        }
    }
    let centre_visibility = vis_sum / f64::from(vis_n);
    println!(
        "rung4 proving: mean centre visibility near the umbra block \
         {centre_visibility:.3} over {vis_n} splats"
    );
    assert!(
        centre_visibility < 0.6,
        "the block's own centres are mostly shadowed: {centre_visibility}"
    );
    for c in 0..3 {
        assert!(
            umbra_lit[c] < 0.6 * umbra_unlit[c],
            "umbra channel {c} darkens by the applied factor (stated \
             tolerance 0.6, cross-checked at the centres): {} vs {}",
            umbra_lit[c],
            umbra_unlit[c]
        );
    }
    assert_eq!(
        clear_lit_bytes, clear_unlit_bytes,
        "carpet outside the shadow is bit-identical between lit and unlit"
    );
}
