//! Splat bench (programme Rung 2): load one real 3DGS
//! capture, pack it through the spike's architecture
//! (CPU pack, one baked quad draw), and MEASURE the
//! costs the DESIGN's §4 only estimates — pack time,
//! packed stream bytes, upload time, steady-state frame
//! time — at the plate size (512x640) and a judging
//! size (1280x720). The twin witness is re-run at
//! production count: determinism must survive a million
//! splats, not just the spike's 781.
//!
//! Two configurations:
//!
//!   A. The capture in its native (COLMAP) space,
//!      splats only, camera fitted to the cloud's bounds.
//!      Nothing is transformed, so the SH field, the
//!      sort and the culling are exactly the asset's own.
//!   B. The capture axis-mapped (COLMAP -> glTF) and
//!      normalised around the G1 toon character, drawn
//!      as the production composite. The axis map moves
//!      centres and covariances; the SH field is NOT
//!      rotated (that is Rung 3's machinery), so B's
//!      view-dependent colour is approximate by exactly
//!      that named amount. B is an evidence still and a
//!      composite cost, not a colour proof.
//!
//! Usage:
//!   splat-bench --ply PATH --fixture DIR --out DIR
//!               [--frames N]
//!
//! Every number prints on its own labelled line and is
//! transcribed into the rung record; nothing here is a
//! gate by itself — the record is where numbers are
//! judged against §4.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, PackedScene, RenderBackend, pack_scene};
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::math::Mat4;
use torogao_render3d::model::{Model, load_model};
use torogao_render3d::setscene::{ArcScene, plate_rig_scene};
use torogao_render3d::splat::{Splat, SplatCloud, SplatTransform, pack_splats};
use torogao_render3d::toon::{ToonData, ToonImage, ToonSettings, ToonTextures};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn identity() -> Mat4 {
    Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
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

/// Camera world matrix from eye/target/up-hint, the
/// look-at construction the proving tests use (rows:
/// right, up, back; camera looks down its -Z).
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

/// Bounds of a cloud: centre (bounds midpoint) and the
/// bounding-sphere radius around it.
fn cloud_bounds(cloud: &SplatCloud) -> ([f64; 3], f64) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for s in &cloud.splats {
        for i in 0..3 {
            lo[i] = lo[i].min(f64::from(s.pos[i]));
            hi[i] = hi[i].max(f64::from(s.pos[i]));
        }
    }
    let centre = [
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    ];
    let mut radius = 0.0_f64;
    for s in &cloud.splats {
        let d = sub(
            [
                f64::from(s.pos[0]),
                f64::from(s.pos[1]),
                f64::from(s.pos[2]),
            ],
            centre,
        );
        radius = radius.max((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt());
    }
    (centre, radius)
}

/// COLMAP -> glTF axis map applied to the geometry:
/// centres (x, -y, -z); rotations R' = A * R with
/// A = diag(1, -1, -1) (180 degrees about X, quaternion
/// (0, 1, 0, 0)), whose Hamilton product with
/// (w, x, y, z) is (-x, w, -z, y). The SH field is not
/// rotated — see the module docs for what that means.
fn colmap_to_gltf(cloud: &SplatCloud) -> SplatCloud {
    let splats = cloud
        .splats
        .iter()
        .map(|s| Splat {
            pos: [s.pos[0], -s.pos[1], -s.pos[2]],
            rot: [-s.rot[1], s.rot[0], -s.rot[3], s.rot[2]],
            ..*s
        })
        .collect();
    SplatCloud { splats }
}

fn rest_draw(model: &Model, visible: bool) -> DrawFrame {
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
        visible_meshes: vec![visible; model.meshes.len()],
        include_set: false,
        toy_model: identity(),
    }
}

/// One uploaded scene, timed: warm-up frames untimed,
/// then `frames` timed renders; returns
/// (upload_ms, median_frame_ms, min_frame_ms, sha of
/// the last frame).
fn timed_render(
    packed: &PackedScene,
    draw: &DrawFrame,
    width: u32,
    height: u32,
    frames: usize,
) -> (f64, f64, f64, String) {
    let mut backend = VulkanBackend::new().expect("backend");
    let t = Instant::now();
    backend.upload_scene(packed, width, height).expect("upload");
    let upload_ms = t.elapsed().as_secs_f64() * 1e3;
    for _ in 0..3 {
        backend.render_frame(draw).expect("warm-up render");
    }
    let mut times = Vec::with_capacity(frames);
    let mut last_hash = String::new();
    for _ in 0..frames {
        let t = Instant::now();
        let frame = backend.render_frame(draw).expect("timed render");
        times.push(t.elapsed().as_secs_f64() * 1e3);
        last_hash = sha256_hex(&frame.rgba);
    }
    times.sort_by(|a, b| a.total_cmp(b));
    (upload_ms, times[times.len() / 2], times[0], last_hash)
}

/// A single render through a fresh backend (the twin
/// witness's second instance).
fn render_once(
    packed: &PackedScene,
    draw: &DrawFrame,
    width: u32,
    height: u32,
) -> torogao_render3d::backend::RenderedFrame {
    let mut backend = VulkanBackend::new().expect("backend");
    backend.upload_scene(packed, width, height).expect("upload");
    backend.render_frame(draw).expect("render")
}

fn save_png(out: &Path, name: &str, frame: &torogao_render3d::backend::RenderedFrame) {
    let image = Image::new(frame.width, frame.height, frame.rgba.clone()).expect("image");
    write_png(&out.join(name), &image).expect("write png");
    println!("evidence: {}", out.join(name).display());
}

fn load_toon(fixture: &Path) -> (Model, ToonData) {
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
    (model, toon)
}

/// The plate scene at a landscape judging size: same
/// lens (same tan_half_y), horizontal tan widened to
/// the viewport's aspect — the same camera on a wider
/// sensor.
fn landscape(mut scene: ArcScene, width: u32, height: u32) -> ArcScene {
    scene.tan_half_x = scene.tan_half_y * (f64::from(width) / f64::from(height));
    scene
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(ply), Some(fixture), Some(out)) = (
        arg_value(&args, "--ply"),
        arg_value(&args, "--fixture"),
        arg_value(&args, "--out"),
    ) else {
        eprintln!("usage: splat-bench --ply PATH --fixture DIR --out DIR [--frames N]");
        return ExitCode::FAILURE;
    };
    let frames: usize = arg_value(&args, "--frames")
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let fixture = PathBuf::from(fixture);
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("out dir");

    let probe = VulkanBackend::new().expect("backend");
    println!("adapter: {}", probe.describe());
    drop(probe);

    // -- Load (timed) ---------------------------------
    let bytes = std::fs::read(&ply).expect("read ply");
    println!("asset: {} ({} bytes)", ply, bytes.len());
    println!("asset sha256: {}", sha256_hex(&bytes));
    let t = Instant::now();
    let cloud = SplatCloud::load_ply(&bytes).expect("load ply");
    let load_ms = t.elapsed().as_secs_f64() * 1e3;
    println!("cloud: {} splats, load {load_ms:.1} ms", cloud.splats.len());
    let (centre, radius) = cloud_bounds(&cloud);
    println!("bounds: centre {centre:?}, radius {radius:.3} (asset units)");

    let (model, toon) = load_toon(&fixture);
    let draw_char = rest_draw(&model, true);
    let draw_hidden = rest_draw(&model, false);

    // -- Config A: native space, splats only ----------
    // COLMAP space: the scene's up is -Y. A three-
    // quarter view from front-above, fitted so the
    // bounding sphere fills the tighter frustum axis.
    let plate_scene = |cloud: &SplatCloud| -> ArcScene {
        let (c, r) = cloud_bounds(cloud);
        let dir = norm([0.55, -0.45, -0.85]);
        let eye0 = [c[0] + dir[0], c[1] + dir[1], c[2] + dir[2]];
        // Read the plate lens from the rig itself, then
        // fit the distance so the bounding sphere fills
        // the tighter frustum axis.
        let lens = plate_rig_scene(camera_world(eye0, c, [0.0, -1.0, 0.0]));
        let tan_min = lens.tan_half_x.min(lens.tan_half_y);
        let dist = r / tan_min * 1.02;
        let eye = [
            c[0] + dir[0] * dist,
            c[1] + dir[1] * dist,
            c[2] + dir[2] * dist,
        ];
        plate_rig_scene(camera_world(eye, c, [0.0, -1.0, 0.0]))
    };

    for (label, width, height, wide) in [
        ("plate 512x640", 512_u32, 640_u32, false),
        ("judging 1280x720", 1280_u32, 720_u32, true),
    ] {
        let base_scene = plate_scene(&cloud);
        let scene = if wide {
            landscape(base_scene, width, height)
        } else {
            base_scene
        };

        // Baseline: the character alone (no splats),
        // the renderer's pre-splat frame cost.
        let mut packed_base = pack_scene(&model, &scene);
        packed_base.toon = Some(toon.clone());
        let (up, med, min, _) = timed_render(&packed_base, &draw_char, width, height, frames);
        println!(
            "{label} baseline (character only): upload {up:.1} ms, frame median {med:.2} ms, min {min:.2} ms"
        );

        // Splats: pack (timed), then upload + frames.
        let t = Instant::now();
        let layer = pack_splats(&cloud, &SplatTransform::default(), &scene, width, height);
        let pack_ms = t.elapsed().as_secs_f64() * 1e3;
        let stream_bytes = layer.vertices.len()
            * std::mem::size_of::<torogao_render3d::splat::SplatVertex>()
            + layer.indices.len() * 4;
        println!(
            "{label} config A pack: {} of {} splats packed in {pack_ms:.1} ms; stream {} bytes ({:.1} MiB)",
            layer.splat_count,
            cloud.splats.len(),
            stream_bytes,
            stream_bytes as f64 / 1_048_576.0
        );
        let mut packed = packed_base.clone();
        packed.splats = Some(layer);
        let (up, med, min, hash) = timed_render(&packed, &draw_hidden, width, height, frames);
        println!(
            "{label} config A (splats only): upload {up:.1} ms, frame median {med:.2} ms, min {min:.2} ms, frame sha {hash}"
        );
        if !wide {
            // Twin witness at production count: a
            // second, fresh backend renders the same
            // packed scene; the readbacks must match
            // bit-for-bit.
            let twin = render_once(&packed, &draw_hidden, width, height);
            let twin_hash = sha256_hex(&twin.rgba);
            println!(
                "{label} config A twin: {twin_hash} — {}",
                if twin_hash == hash {
                    "IDENTICAL"
                } else {
                    "MISMATCH"
                }
            );
            assert_eq!(twin_hash, hash, "twin witness at production count");
            let frame = render_once(&packed, &draw_hidden, width, height);
            save_png(&out, "bench-config-a-plate.png", &frame);
        }
    }

    // -- Config B: composite around the character -----
    let mapped = colmap_to_gltf(&cloud);
    let (mc, mr) = cloud_bounds(&mapped);
    let scale = 2.0 / mr;
    // Floor of the mapped cloud sits at y = 0 (the
    // character's ground plane).
    let mut floor_y = f64::INFINITY;
    for s in &mapped.splats {
        floor_y = floor_y.min(f64::from(s.pos[1]));
    }
    let transform = SplatTransform {
        translation: [-mc[0] * scale, -floor_y * scale, -mc[2] * scale],
        rotation: torogao_render3d::splat_rotate::IDENTITY_QUAT,
        scale,
    };
    let (width, height) = (512_u32, 640_u32);
    let scene = plate_rig_scene(camera_world(
        [2.6, 1.9, -3.4],
        [0.0, 0.9, 0.0],
        [0.0, 1.0, 0.0],
    ));
    let t = Instant::now();
    let layer = pack_splats(&mapped, &transform, &scene, width, height);
    let pack_ms = t.elapsed().as_secs_f64() * 1e3;
    println!(
        "config B pack: {} of {} splats in {pack_ms:.1} ms (layer scale {scale:.4})",
        layer.splat_count,
        mapped.splats.len()
    );
    let mut packed = pack_scene(&model, &scene);
    packed.toon = Some(toon);
    packed.splats = Some(layer);
    let (up, med, min, hash) = timed_render(&packed, &draw_char, width, height, frames);
    println!(
        "plate config B (composite): upload {up:.1} ms, frame median {med:.2} ms, min {min:.2} ms, frame sha {hash}"
    );
    let frame = render_once(&packed, &draw_char, width, height);
    save_png(&out, "bench-config-b-composite.png", &frame);

    println!("bench complete");
    ExitCode::SUCCESS
}
