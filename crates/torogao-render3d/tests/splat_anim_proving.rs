//! Splat GPU-path + animation proving tests
//! (programme Rung 5, on DESIGN.md §6.1).
//!
//! The contract, proven against the procedural
//! room-corner cloud at the proving framing:
//!
//! 1. Static: the GPU path's frame is bit-identical
//!    to the CPU pack's frame of the same layer, and
//!    its projected records equal the CPU projection's
//!    record-for-record (the §6.1 determinism gate at
//!    packet level).
//! 2. Rotated: the same gates under a non-identity
//!    layer rotation (the Wigner-D SH path).
//! 3. Lit: the same gates with the Rung 4 light
//!    interaction named (centre shadow + grade).
//! 4. Motion: an 8-frame TRS sweep — every frame's
//!    GPU render equals the CPU pack's render at
//!    that frame's evaluated transform, and a second
//!    sweep is twin-identical to the first.
//! 5. Sequence: a 3-frame resident sequence — every
//!    frame's GPU render equals the CPU still of
//!    that frame's cloud.
//!
//! Ignored like the other proving tests: it needs a
//! Vulkan device and the fixture directory in
//! TOROGAO_RENDER3D_TOON_FIXTURE. When
//! TOROGAO_RENDER3D_SPLAT_OUT is set, renders land
//! there as PNG evidence.

use std::path::PathBuf;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, PackedScene, RenderBackend, RenderedFrame, pack_scene};
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::math::Mat4;
use torogao_render3d::model::{Model, load_model};
use torogao_render3d::morph::PackedMorphs;
use torogao_render3d::reference::skin_mesh;
use torogao_render3d::setscene::{ArcScene, plate_rig_scene};
use torogao_render3d::splat::{
    PackedSplats, SplatCloud, SplatTransform, pack_splats, pack_splats_lit, project_splats,
    room_corner_cloud,
};
use torogao_render3d::splat_gpu::{
    LayerMotion, PackedGpuSplats, ProjectedRecord, SplatCamera, TrsKey,
};
use torogao_render3d::splat_light::{
    LightResponse, SHADOW_MAP_SIZE, SplatLight, SplatShadowMap, mesh_triangles,
};
use torogao_render3d::toon::{ToonData, ToonImage, ToonSettings, ToonTextures};

fn identity() -> Mat4 {
    Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

/// The proving framing (the splat proving tests'
/// camera): eye (2.6, 1.9, -3.4) toward (0, 0.9, 0).
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

fn load_toon(fixture: &std::path::Path) -> (Model, ToonData) {
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
    (model, toon)
}

struct Rig {
    model: Model,
    toon: ToonData,
    scene: ArcScene,
    draw: DrawFrame,
    width: u32,
    height: u32,
}

fn rig() -> Rig {
    let fixture =
        PathBuf::from(std::env::var("TOROGAO_RENDER3D_TOON_FIXTURE").expect("fixture dir env var"));
    let (model, toon) = load_toon(&fixture);
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
    Rig {
        model,
        toon,
        scene,
        draw,
        width,
        height,
    }
}

impl Rig {
    fn cpu_scene(&self, layer: torogao_render3d::splat::PackedSplats) -> PackedScene {
        let mut packed = pack_scene(&self.model, &self.scene);
        packed.toon = Some(self.toon.clone());
        packed.splats = Some(layer);
        packed
    }

    fn gpu_scene(&self, packet: PackedGpuSplats) -> PackedScene {
        let mut packed = pack_scene(&self.model, &self.scene);
        packed.toon = Some(self.toon.clone());
        packed.splat_gpu = Some(packet);
        packed
    }

    fn packet(
        &self,
        frames: Vec<SplatCloud>,
        transform: SplatTransform,
        shadow: Option<SplatShadowMap>,
        response: Option<LightResponse>,
    ) -> PackedGpuSplats {
        PackedGpuSplats {
            frames,
            transform,
            camera: SplatCamera::from_scene(&self.scene, self.width, self.height),
            shadow,
            response,
        }
    }
}

fn render(packed: &PackedScene, draw: &DrawFrame, width: u32, height: u32) -> RenderedFrame {
    let mut backend = VulkanBackend::new().expect("backend");
    backend.upload_scene(packed, width, height).expect("upload");
    backend.render_frame(draw).expect("render")
}

fn save_evidence(name: &str, frame: &RenderedFrame) {
    if let Ok(dir) = std::env::var("TOROGAO_RENDER3D_SPLAT_OUT") {
        let image = Image::new(frame.width, frame.height, frame.rgba.clone()).expect("image");
        let path = PathBuf::from(dir).join(name);
        write_png(&path, &image).expect("write evidence png");
        println!("evidence: {}", path.display());
    }
}

/// The packet-level gate: the GPU snapshot's records
/// equal the CPU projection's, exactly and in order.
fn assert_snapshot_equals(
    rig: &Rig,
    backend: &mut VulkanBackend,
    cloud: &SplatCloud,
    transform: &SplatTransform,
    light: Option<SplatLight<'_>>,
    packet: &PackedSplats,
    what: &str,
) {
    let snapshot = backend.splat_gpu_snapshot().expect("snapshot");
    let expected: Vec<ProjectedRecord> =
        project_splats(cloud, transform, &rig.scene, rig.width, rig.height, light)
            .iter()
            .map(ProjectedRecord::from_projection)
            .collect();
    assert_eq!(
        snapshot.survivor_count,
        expected.len(),
        "{what}: survivor count"
    );
    assert_eq!(snapshot.records, expected, "{what}: projected records");
    // The gather's stream and draw count, against
    // the caller's CPU pack of the same projection
    // (the lit caller passes its lit pack).
    assert_eq!(snapshot.stream, packet.vertices, "{what}: quad stream");
    assert_eq!(
        snapshot.draw_index_count as usize,
        packet.indices.len(),
        "{what}: draw index count"
    );
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn gpu_static_layer_matches_the_cpu_pack() {
    let rig = rig();
    let cloud = room_corner_cloud();
    let transform = SplatTransform::default();

    let cpu = render(
        &rig.cpu_scene(pack_splats(
            &cloud, &transform, &rig.scene, rig.width, rig.height,
        )),
        &rig.draw,
        rig.width,
        rig.height,
    );
    let mut backend = VulkanBackend::new().expect("backend");
    backend
        .upload_scene(
            &rig.gpu_scene(rig.packet(vec![cloud.clone()], transform, None, None)),
            rig.width,
            rig.height,
        )
        .expect("upload");
    assert_snapshot_equals(
        &rig,
        &mut backend,
        &cloud,
        &transform,
        None,
        &pack_splats(&cloud, &transform, &rig.scene, rig.width, rig.height),
        "static",
    );
    let gpu = backend.render_frame(&rig.draw).expect("render");
    save_evidence("anim-static-gpu.png", &gpu);
    save_evidence("anim-static-cpu.png", &cpu);
    assert_eq!(
        sha256_hex(&gpu.rgba),
        sha256_hex(&cpu.rgba),
        "GPU frame must equal the CPU pack's frame bit-for-bit"
    );
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn gpu_rotated_layer_matches_the_cpu_pack() {
    let rig = rig();
    let cloud = room_corner_cloud();
    // 30 degrees about Y plus a translation: the
    // Wigner-D rotation path, exercised end to end.
    let half = std::f64::consts::PI / 12.0;
    let transform = SplatTransform {
        translation: [0.2, 0.05, -0.1],
        rotation: [half.cos(), 0.0, half.sin(), 0.0],
        scale: 1.05,
    };
    let cpu = render(
        &rig.cpu_scene(pack_splats(
            &cloud, &transform, &rig.scene, rig.width, rig.height,
        )),
        &rig.draw,
        rig.width,
        rig.height,
    );
    let mut backend = VulkanBackend::new().expect("backend");
    backend
        .upload_scene(
            &rig.gpu_scene(rig.packet(vec![cloud.clone()], transform, None, None)),
            rig.width,
            rig.height,
        )
        .expect("upload");
    assert_snapshot_equals(
        &rig,
        &mut backend,
        &cloud,
        &transform,
        None,
        &pack_splats(&cloud, &transform, &rig.scene, rig.width, rig.height),
        "rotated",
    );
    let gpu = backend.render_frame(&rig.draw).expect("render");
    save_evidence("anim-rotated-gpu.png", &gpu);
    assert_eq!(
        sha256_hex(&gpu.rgba),
        sha256_hex(&cpu.rgba),
        "rotated GPU frame must equal the CPU pack's frame bit-for-bit"
    );
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn gpu_lit_layer_matches_the_cpu_pack() {
    let rig = rig();
    let cloud = room_corner_cloud();
    let transform = SplatTransform::default();
    // The character's own skinned triangles as the
    // casters (the Rung 4 proving construction).
    let joints: Vec<Mat4> = (0..rig.model.skin.joints.len())
        .map(|_| identity())
        .collect();
    let mut triangles = Vec::new();
    for (index, mesh) in rig.model.meshes.iter().enumerate() {
        let morphs = PackedMorphs::pack(mesh);
        let (positions, _normals) =
            skin_mesh(&rig.model, index, &morphs, &joints, &mesh.default_weights);
        triangles.extend(mesh_triangles(&positions, &mesh.indices));
    }
    assert!(!triangles.is_empty(), "the character casts");
    let shadow_map = SplatShadowMap::rasterise(
        &triangles,
        torogao_render3d::backend::light_view_proj(rig.scene.sun.to_light),
        SHADOW_MAP_SIZE,
    );
    let response = LightResponse {
        grade: [[0.9, 0.1, 0.0], [0.0, 1.0, 0.2], [0.3, 0.0, 0.8]],
        world_mix: 0.35,
    };
    let light = SplatLight {
        shadow: Some(&shadow_map),
        response: Some(response),
    };
    let lit_packet = pack_splats_lit(
        &cloud, &transform, &rig.scene, rig.width, rig.height, &light,
    );
    let cpu = render(
        &rig.cpu_scene(lit_packet.clone()),
        &rig.draw,
        rig.width,
        rig.height,
    );
    let mut backend = VulkanBackend::new().expect("backend");
    backend
        .upload_scene(
            &rig.gpu_scene(rig.packet(
                vec![cloud.clone()],
                transform,
                Some(shadow_map.clone()),
                Some(response),
            )),
            rig.width,
            rig.height,
        )
        .expect("upload");
    assert_snapshot_equals(
        &rig,
        &mut backend,
        &cloud,
        &transform,
        Some(light),
        &lit_packet,
        "lit",
    );
    let gpu = backend.render_frame(&rig.draw).expect("render");
    save_evidence("anim-lit-gpu.png", &gpu);
    save_evidence("anim-lit-cpu.png", &cpu);
    assert_eq!(
        sha256_hex(&gpu.rgba),
        sha256_hex(&cpu.rgba),
        "lit GPU frame must equal the CPU pack's frame bit-for-bit"
    );
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn gpu_motion_sweep_matches_cpu_and_is_twin_deterministic() {
    let rig = rig();
    let cloud = room_corner_cloud();
    let half = std::f64::consts::PI / 12.0;
    let motion = LayerMotion::new(vec![
        TrsKey {
            frame: 0,
            translation: [0.0, 0.0, 0.0],
            rotation: [1.0, 0.0, 0.0, 0.0],
            scale: 1.0,
        },
        TrsKey {
            frame: 7,
            translation: [0.25, 0.1, 0.0],
            rotation: [half.cos(), 0.0, half.sin(), 0.0],
            scale: 1.1,
        },
    ])
    .expect("motion");
    let mut backend = VulkanBackend::new().expect("backend");
    backend
        .upload_scene(
            &rig.gpu_scene(rig.packet(vec![cloud.clone()], SplatTransform::default(), None, None)),
            rig.width,
            rig.height,
        )
        .expect("upload");
    let mut first_sweep: Vec<String> = Vec::new();
    for sweep in 0..2 {
        let mut hashes: Vec<String> = Vec::new();
        for frame in 0..8_u32 {
            let transform = motion.transform_at(frame);
            backend
                .set_splat_transform(transform)
                .expect("set transform");
            let gpu = backend.render_frame(&rig.draw).expect("render");
            let gpu_hash = sha256_hex(&gpu.rgba);
            if sweep == 0 {
                // The sequence-path packet equals the
                // still-path packet for the same
                // (cloud, transform, camera): frame and
                // records both.
                assert_snapshot_equals(
                    &rig,
                    &mut backend,
                    &cloud,
                    &transform,
                    None,
                    &pack_splats(&cloud, &transform, &rig.scene, rig.width, rig.height),
                    "motion frame",
                );
                let cpu = render(
                    &rig.cpu_scene(pack_splats(
                        &cloud, &transform, &rig.scene, rig.width, rig.height,
                    )),
                    &rig.draw,
                    rig.width,
                    rig.height,
                );
                assert_eq!(
                    gpu_hash,
                    sha256_hex(&cpu.rgba),
                    "motion frame {frame}: GPU render must equal the CPU pack at the evaluated transform"
                );
                if frame == 3 {
                    save_evidence("anim-motion-frame3-gpu.png", &gpu);
                }
            }
            hashes.push(gpu_hash);
        }
        if sweep == 0 {
            first_sweep = hashes;
        } else {
            assert_eq!(
                hashes, first_sweep,
                "the animated sweep must be twin-identical"
            );
        }
    }
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_TOON_FIXTURE"]
fn gpu_sequence_frames_match_cpu_stills() {
    let rig = rig();
    let base = room_corner_cloud();
    let shifted = |dx: f32, dy: f32| {
        let mut cloud = base.clone();
        for s in &mut cloud.splats {
            s.pos[0] += dx;
            s.pos[1] += dy;
        }
        cloud
    };
    let frames = vec![base.clone(), shifted(0.15, 0.0), shifted(0.0, 0.2)];
    let mut backend = VulkanBackend::new().expect("backend");
    backend
        .upload_scene(
            &rig.gpu_scene(rig.packet(frames.clone(), SplatTransform::default(), None, None)),
            rig.width,
            rig.height,
        )
        .expect("upload");
    for (f, frame_cloud) in frames.iter().enumerate() {
        backend.set_splat_sequence_frame(f).expect("set frame");
        assert_snapshot_equals(
            &rig,
            &mut backend,
            frame_cloud,
            &SplatTransform::default(),
            None,
            &pack_splats(
                frame_cloud,
                &SplatTransform::default(),
                &rig.scene,
                rig.width,
                rig.height,
            ),
            "sequence frame",
        );
        let gpu = backend.render_frame(&rig.draw).expect("render");
        let cpu = render(
            &rig.cpu_scene(pack_splats(
                frame_cloud,
                &SplatTransform::default(),
                &rig.scene,
                rig.width,
                rig.height,
            )),
            &rig.draw,
            rig.width,
            rig.height,
        );
        assert_eq!(
            sha256_hex(&gpu.rgba),
            sha256_hex(&cpu.rgba),
            "sequence frame {f}: GPU render must equal the CPU still"
        );
        if f == 1 {
            save_evidence("anim-sequence-frame1-gpu.png", &gpu);
        }
    }
    // Out-of-range selection is a typed error, never
    // a silent clamp.
    assert!(backend.set_splat_sequence_frame(3).is_err());
}
