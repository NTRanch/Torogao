//! Splat pass proving test.
//!
//! The spike's contract, proven against the procedural
//! room-corner cloud (known ground truth by
//! construction — `splat::room_corner_cloud`) composited
//! with the G1 toon fixture character:
//!
//! 1. Splat-only render: the floor, walls and the warm
//!    sphere prop read back at their authored colours
//!    (through the renderer's display transform, the
//!    mirror of which lives in setscene).
//! 2. Composite render: character + splats differ from
//!    both parents materially; inside the character's
//!    silhouette the composite matches the
//!    character-only render on the overwhelming
//!    majority of pixels (the room stands behind her
//!    in this framing and is depth-occluded there).
//! 3. Twin render of the composite is bit-identical
//!    (the estate's determinism witness).
//!
//! Ignored like the other proving tests: it needs a
//! Vulkan device and the fixture directory in
//! TOROGAO_RENDER3D_TOON_FIXTURE. When
//! TOROGAO_RENDER3D_SPLAT_OUT is set, the renders land
//! there as PNG evidence.

use std::path::PathBuf;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, PackedScene, RenderBackend, RenderedFrame, pack_scene};
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::math::Mat4;
use torogao_render3d::model::load_model;
use torogao_render3d::setscene::{display_transform_mirror, plate_rig_scene};
use torogao_render3d::splat::{SplatTransform, pack_splats, room_corner_cloud};
use torogao_render3d::toon::{ToonData, ToonImage, ToonSettings, ToonTextures};

fn identity() -> Mat4 {
    Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

/// Camera world matrix, wide framing: eye
/// (2.6, 1.9, -3.4) toward (0, 0.9, 0) — the whole
/// room corner and the standing character in frame.
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

/// Project a world point to pixel coordinates with
/// the scene's own matrices (the same projection the
/// packer used).
fn project_px(
    scene: &torogao_render3d::setscene::ArcScene,
    p: [f64; 3],
    w: u32,
    h: u32,
) -> (i32, i32) {
    let vp = torogao_render3d::setscene::projection(scene).multiply(scene.view);
    let v = [p[0], p[1], p[2], 1.0];
    let mut clip = [0.0; 4];
    for i in 0..4 {
        clip[i] = vp.m[i] * v[0] + vp.m[4 + i] * v[1] + vp.m[8 + i] * v[2] + vp.m[12 + i] * v[3];
    }
    let ndc = [clip[0] / clip[3], clip[1] / clip[3]];
    (
        ((ndc[0] * 0.5 + 0.5) * f64::from(w)) as i32,
        ((ndc[1] * 0.5 + 0.5) * f64::from(h)) as i32,
    )
}

/// Mean RGB over a square block around (cx, cy).
fn block_mean(frame: &RenderedFrame, w: u32, cx: i32, cy: i32, r: i32) -> [f64; 3] {
    let mut sum = [0.0; 3];
    let mut n = 0.0;
    for y in (cy - r)..=(cy + r) {
        for x in (cx - r)..=(cx + r) {
            if x < 0 || y < 0 || x >= w as i32 {
                continue;
            }
            let at = ((y as u32 * w + x as u32) * 4) as usize;
            if at + 2 >= frame.rgba.len() {
                continue;
            }
            for ch in 0..3 {
                sum[ch] += f64::from(frame.rgba[at + ch]);
            }
            n += 1.0;
        }
    }
    [sum[0] / n, sum[1] / n, sum[2] / n]
}

/// Assert a block mean sits within `tol` per channel
/// of the display-transformed authored colour.
fn assert_block_near(
    frame: &RenderedFrame,
    scene: &torogao_render3d::setscene::ArcScene,
    point: [f64; 3],
    linear: [f32; 3],
    tol: f64,
    what: &str,
    viewport: (u32, u32),
) {
    let (width, height) = viewport;
    let (cx, cy) = project_px(scene, point, width, height);
    let mean = block_mean(frame, width, cx, cy, 6);
    let d = display_transform_mirror(linear);
    let want = [
        f64::from(d[0]) * 255.0,
        f64::from(d[1]) * 255.0,
        f64::from(d[2]) * 255.0,
    ];
    println!("{what} at ({cx},{cy}): mean {mean:?}, want {want:?}");
    for ch in 0..3 {
        assert!(
            (mean[ch] - want[ch]).abs() <= tol,
            "{what} channel {ch}: mean {} vs want {} (tol {tol})",
            mean[ch],
            want[ch]
        );
    }
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn splats_composite_with_the_toon_character() {
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
            params: None,
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
    let mut packed = pack_scene(&model, &scene);
    packed.toon = Some(toon);
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

    // Parents: character only, splats only.
    let character_only = render(&packed, &draw, width, height);
    let cloud = room_corner_cloud();
    let splat_layer = pack_splats(&cloud, &SplatTransform::default(), &scene, width, height);
    println!(
        "room cloud: {} splats in, {} packed",
        cloud.splats.len(),
        splat_layer.splat_count
    );
    {
        let mut max_off = 0.0_f32;
        let mut max_clip = 0.0_f32;
        for v in &splat_layer.vertices {
            max_off = max_off.max(v.offset_px[0].abs()).max(v.offset_px[1].abs());
            max_clip = max_clip.max(v.clip[0].abs()).max(v.clip[1].abs());
        }
        let first = &splat_layer.vertices[0];
        println!(
            "packed stream: max |offset| {max_off:.1} px, max |clip| {max_clip:.2}, \
             first vertex clip {:?} conic {:?} colour {:?}",
            first.clip, first.conic, first.colour
        );
    }
    assert!(splat_layer.splat_count > 500, "the room must pack");
    packed.splats = Some(splat_layer);
    let mut draw_hidden = draw.clone();
    draw_hidden.visible_meshes = vec![false; model.meshes.len()];
    let splats_only = render(&packed, &draw_hidden, width, height);
    save_evidence("splat-room-only.png", &splats_only, width, height);
    {
        let px = |x: u32, y: u32| {
            let at = ((y * width + x) * 4) as usize;
            [
                splats_only.rgba[at],
                splats_only.rgba[at + 1],
                splats_only.rgba[at + 2],
            ]
        };
        println!(
            "splat-only samples: TL {:?} TR {:?} C {:?} BL {:?} BR {:?}",
            px(8, 8),
            px(width - 8, 8),
            px(width / 2, height / 2),
            px(8, height - 8),
            px(width - 8, height - 8)
        );
    }

    // -- 1. Ground truth reads back -------------------
    // Block means at the projected feature locations,
    // compared in display space (the splat field is
    // soft, so the check is a local mean, not a pixel
    // census).
    assert_block_near(
        &splats_only,
        &scene,
        [0.6, 0.0, 0.6],
        [0.32, 0.34, 0.40],
        30.0,
        "floor",
        (width, height),
    );
    assert_block_near(
        &splats_only,
        &scene,
        [-0.6, 1.5, 2.4],
        [0.45, 0.40, 0.36],
        30.0,
        "back wall",
        (width, height),
    );
    assert_block_near(
        &splats_only,
        &scene,
        [-0.8, 0.55, 1.0],
        [0.85, 0.45, 0.20],
        34.0,
        "sphere",
        (width, height),
    );

    // -- 2. Composite ---------------------------------
    let composite = render(&packed, &draw, width, height);
    let composite_twin = render(&packed, &draw, width, height);
    assert_eq!(
        sha256_hex(&composite.rgba),
        sha256_hex(&composite_twin.rgba),
        "splat composite must be deterministic"
    );
    let pixels = (width * height) as usize;
    let diff_vs_character = composite
        .rgba
        .chunks_exact(4)
        .zip(character_only.rgba.chunks_exact(4))
        .filter(|(a, b)| a[..3] != b[..3])
        .count();
    let diff_vs_splats = composite
        .rgba
        .chunks_exact(4)
        .zip(splats_only.rgba.chunks_exact(4))
        .filter(|(a, b)| a[..3] != b[..3])
        .count();
    println!(
        "composite differs from character-only on {diff_vs_character} px, \
         from splats-only on {diff_vs_splats} px (of {pixels})"
    );
    assert!(
        diff_vs_character > pixels / 10,
        "the environment must be present in the composite"
    );
    assert!(
        diff_vs_splats > pixels / 20,
        "the character must be present in the composite"
    );

    // Occlusion: the empty render (all hidden, no
    // splats) is the clear backdrop; the character
    // silhouette is where character-only differs from
    // it. Inside the silhouette, the composite must
    // equal character-only almost everywhere — in
    // this framing the whole room stands behind or
    // around her, so the environment there is
    // depth-occluded; the slack covers edge pixels
    // where a splat quad straddles the silhouette
    // (the centre-depth limit, DESIGN.md L2).
    let mut bare = packed.clone();
    bare.splats = None;
    let backdrop = render(&bare, &draw_hidden, width, height);
    let mut silhouette = 0usize;
    let mut occluded_same = 0usize;
    for i in 0..pixels {
        let base = i * 4;
        let in_silhouette = character_only.rgba[base..base + 3] != backdrop.rgba[base..base + 3];
        if in_silhouette {
            silhouette += 1;
            if composite.rgba[base..base + 3] == character_only.rgba[base..base + 3] {
                occluded_same += 1;
            }
        }
    }
    println!("silhouette {silhouette} px, occluded-identical {occluded_same} px");
    assert!(silhouette > 5_000, "the character must be in frame");
    assert!(
        occluded_same * 100 >= silhouette * 85,
        "splats behind the character must be depth-occluded \
         ({occluded_same}/{silhouette})"
    );

    save_evidence("splat-composite.png", &composite, width, height);
    save_evidence("splat-room-only.png", &splats_only, width, height);
    save_evidence("splat-character-only.png", &character_only, width, height);
}
