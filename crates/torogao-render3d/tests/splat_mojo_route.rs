//! Mojo desktop route gates (adoption, 2026-11-27):
//! the integrated route in `splat_mojo` against the
//! CPU reference pack.
//!
//! All tests are ignored: they need the compiled
//! kernel binary (`mojo/build.sh`) and a GPU. Run
//! them explicitly:
//!
//!   TOROGAO_MOJO_PROJECT_BIN=.../mojo/build/splat_project_sm86 \
//!   TOROGAO_MOJO_DEVICE_ID=1 \
//!   cargo test -p torogao-render3d --test splat_mojo_route \
//!       -- --ignored
//!
//! The twin-render test additionally needs a Vulkan
//! device and the toon fixture in
//! TOROGAO_RENDER3D_TOON_FIXTURE; the full-witness
//! test needs the bonsai ply in
//! TOROGAO_MOJO_WITNESS_PLY.

use std::path::PathBuf;

use torogao_render3d::math::Mat4;
use torogao_render3d::setscene::{ArcScene, plate_rig_scene};
use torogao_render3d::splat::{
    SplatCloud, SplatTransform, pack_splats, project_splats, room_corner_cloud,
};
use torogao_render3d::splat_light::SplatLight;
use torogao_render3d::splat_mojo::{MojoRoute, pack_splats_mojo, project_splats_mojo};

fn route() -> MojoRoute {
    MojoRoute {
        project_bin: PathBuf::from(
            std::env::var("TOROGAO_MOJO_PROJECT_BIN").expect("kernel binary env var"),
        ),
        device_id: std::env::var("TOROGAO_MOJO_DEVICE_ID")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        work_dir: std::env::temp_dir().join("torogao-mojo-route-gate"),
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}
fn camera_world(eye: [f64; 3], target: [f64; 3], up_hint: [f64; 3]) -> Mat4 {
    let forward = norm(sub(target, eye));
    let right = norm(cross(forward, up_hint));
    let up = cross(right, forward);
    let back = [-forward[0], -forward[1], -forward[2]];
    Mat4::from_rows([
        [right[0], up[0], back[0], eye[0]],
        [right[1], up[1], back[1], eye[1]],
        [right[2], up[2], back[2], eye[2]],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

/// The proving framing (eye (2.6, 1.9, -3.4) toward
/// (0, 0.9, 0)), framing the room-corner cloud.
fn proving_scene() -> ArcScene {
    plate_rig_scene(camera_world(
        [2.6, 1.9, -3.4],
        [0.0, 0.9, 0.0],
        [0.0, 1.0, 0.0],
    ))
}

/// A 40-degree rotation about a tilted axis, as a
/// storage-order quaternion (w, x, y, z).
fn tilted_rotation() -> [f64; 4] {
    let axis = norm([0.3, 1.0, 0.2]);
    let half = (40.0_f64).to_radians() / 2.0;
    [
        half.cos(),
        axis[0] * half.sin(),
        axis[1] * half.sin(),
        axis[2] * half.sin(),
    ]
}

/// The route's projection equals the CPU reference
/// record-for-record (PartialEq on SplatProjection is
/// exact value equality; the bench gate proves the
/// bitwise claim at full scale), identity and
/// rotated, and the packets built on them are equal.
#[test]
#[ignore = "needs the Mojo kernel binary + GPU; see file docs"]
fn route_matches_cpu_on_room_corner() {
    let route = route();
    let cloud = room_corner_cloud();
    let scene = proving_scene();
    for transform in [
        SplatTransform::default(),
        SplatTransform {
            translation: [0.25, -0.1, 0.4],
            rotation: tilted_rotation(),
            scale: 1.35,
        },
    ] {
        let via_route = project_splats_mojo(&cloud, &transform, &scene, 512, 640, &route)
            .expect("kernel route must produce a projection");
        let cpu = project_splats(&cloud, &transform, &scene, 512, 640, None);
        assert_eq!(via_route.len(), cpu.len(), "survivor count");
        for (a, b) in via_route.iter().zip(cpu.iter()) {
            assert_eq!(a, b, "record at index {}", a.index);
        }
        let light = SplatLight::default();
        let packed_route =
            pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &light, Some(&route));
        let packed_cpu = pack_splats(&cloud, &transform, &scene, 512, 640);
        assert_eq!(packed_route, packed_cpu, "packet equality");
    }
}

/// The full 1,157,141-splat bonsai witness through
/// the integrated route: the packet equals the CPU
/// pack's packet, byte for byte (PartialEq over the
/// vertex/index streams).
#[test]
#[ignore = "needs the Mojo kernel binary + GPU + the bonsai witness ply"]
fn route_matches_cpu_on_full_witness() {
    let route = route();
    let ply = std::env::var("TOROGAO_MOJO_WITNESS_PLY").expect("witness ply env var");
    let bytes = std::fs::read(&ply).expect("read witness ply");
    let cloud = SplatCloud::load_ply(&bytes).expect("load witness ply");
    assert_eq!(cloud.splats.len(), 1_157_141, "the gated witness size");

    // The bench's plate config A scene for this cloud.
    let (centre, radius) = {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for s in &cloud.splats {
            for i in 0..3 {
                lo[i] = lo[i].min(f64::from(s.pos[i]));
                hi[i] = hi[i].max(f64::from(s.pos[i]));
            }
        }
        let c = [
            (lo[0] + hi[0]) / 2.0,
            (lo[1] + hi[1]) / 2.0,
            (lo[2] + hi[2]) / 2.0,
        ];
        let mut r = 0.0_f64;
        for s in &cloud.splats {
            let d = sub(
                [
                    f64::from(s.pos[0]),
                    f64::from(s.pos[1]),
                    f64::from(s.pos[2]),
                ],
                c,
            );
            r = r.max((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt());
        }
        (c, r)
    };
    let dir = norm([0.55, -0.45, -0.85]);
    let eye0 = [centre[0] + dir[0], centre[1] + dir[1], centre[2] + dir[2]];
    let lens = plate_rig_scene(camera_world(eye0, centre, [0.0, -1.0, 0.0]));
    let dist = radius / lens.tan_half_x.min(lens.tan_half_y) * 1.02;
    let eye = [
        centre[0] + dir[0] * dist,
        centre[1] + dir[1] * dist,
        centre[2] + dir[2] * dist,
    ];
    let scene = plate_rig_scene(camera_world(eye, centre, [0.0, -1.0, 0.0]));

    let transform = SplatTransform::default();
    let light = SplatLight::default();
    let via_route = pack_splats_mojo(&cloud, &transform, &scene, 512, 640, &light, Some(&route));
    let cpu = pack_splats(&cloud, &transform, &scene, 512, 640);
    assert_eq!(via_route.splat_count, 1_157_094, "witness survivor count");
    assert_eq!(via_route, cpu, "full-witness packet equality");
}

/// Twin-render determinism on a scene packed through
/// the Mojo route: two renders of the route's packet
/// are pixel-identical, and the CPU pack's render of
/// the same layer is pixel-identical to them (the
/// route cannot change a frame).
#[test]
#[ignore = "needs the Mojo kernel binary + GPU + Vulkan + the toon fixture"]
fn mojo_packed_scene_renders_twin_identical() {
    use torogao_render3d::backend::vulkan::VulkanBackend;
    use torogao_render3d::backend::{DrawFrame, PackedScene, RenderBackend, pack_scene};
    use torogao_render3d::model::{Model, load_model};
    use torogao_render3d::toon::{ToonData, ToonImage, ToonSettings, ToonTextures};

    let route = route();
    let fixture =
        PathBuf::from(std::env::var("TOROGAO_RENDER3D_TOON_FIXTURE").expect("fixture env var"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.join("manifest.json")).unwrap()).unwrap();
    let model: Model = load_model(&std::fs::read(fixture.join("prima_toon.glb")).unwrap()).unwrap();
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
    let scene = proving_scene();
    let identity = Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let draw = DrawFrame {
        frame: 0,
        joint_matrices: (0..model.skin.joints.len())
            .map(|_| identity.to_f32_cols())
            .collect(),
        mesh_weights: model
            .meshes
            .iter()
            .map(|m| m.default_weights.clone())
            .collect(),
        visible_meshes: vec![true; model.meshes.len()],
        include_set: false,
        toy_model: identity,
    };
    let cloud = room_corner_cloud();
    let transform = SplatTransform::default();
    let light = SplatLight::default();
    let packet_route = pack_splats_mojo(
        &cloud,
        &transform,
        &scene,
        width,
        height,
        &light,
        Some(&route),
    );
    let packet_cpu = pack_splats(&cloud, &transform, &scene, width, height);
    assert_eq!(packet_route, packet_cpu);

    let render = |packet: torogao_render3d::splat::PackedSplats| {
        let mut packed: PackedScene = pack_scene(&model, &scene);
        packed.toon = Some(toon.clone());
        packed.splats = Some(packet);
        let mut backend = VulkanBackend::new().expect("backend");
        backend
            .upload_scene(&packed, width, height)
            .expect("upload");
        backend.render_frame(&draw).expect("render")
    };
    let first = render(packet_route.clone());
    let second = render(packet_route);
    assert_eq!(
        first.rgba, second.rgba,
        "twin renders of the Mojo packet differ"
    );
    let cpu_frame = render(packet_cpu);
    assert_eq!(
        first.rgba, cpu_frame.rgba,
        "Mojo-packed frame differs from the CPU-packed frame"
    );
}
