//! Splat animation bench (programme Rung 5, on the
//! §6.1 GPU path): the determinism + performance
//! instrument for the on-device projection and sort.
//!
//! What it measures, at the production capture
//! (1,157,141 splats) and the plate size (512x640),
//! with the judging size (1280x720) for the frame
//! rate:
//!
//!   1. The witness: config A (native space, splats
//!      only) rendered on the GPU path must reproduce
//!      the CPU path's frame — the Rung 2/4 twin
//!      witness dd2ec4b9... — bit-for-bit, and the
//!      snapshot's projected records must equal the
//!      CPU projection's records record-for-record.
//!   2. The rotated layer: the same gates under a
//!      non-identity layer rotation (the Wigner-D
//!      path), which config A's identity transform
//!      never exercises.
//!   3. Animation: a 30-frame TRS motion sweep,
//!      rendered twice; every frame's hash must be
//!      twin-identical, and one mid-sweep frame's
//!      snapshot must equal the CPU projection at
//!      that frame's transform.
//!   4. Sequence: a two-frame resident sequence,
//!      each frame gated against the CPU pack of
//!      that frame's cloud.
//!   5. Frame times: median full-frame (projection +
//!      sort + draw + readback) on the GPU path,
//!      against the 24 fps premise (41.7 ms).
//!
//! Usage:
//!   splat-anim-bench --ply PATH --fixture DIR --out DIR
//!
//! Every number prints on its own labelled line and
//! is transcribed into the rung record.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, RenderBackend, pack_scene};
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::math::Mat4;
use torogao_render3d::model::{Model, load_model};
use torogao_render3d::setscene::{ArcScene, plate_rig_scene};
use torogao_render3d::splat::{SplatCloud, SplatTransform, pack_splats, project_splats};
use torogao_render3d::splat_gpu::{
    LayerMotion, PackedGpuSplats, ProjectedRecord, SplatCamera, TrsKey,
};
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

fn plate_scene(cloud: &SplatCloud) -> ArcScene {
    let (c, r) = cloud_bounds(cloud);
    let dir = norm([0.55, -0.45, -0.85]);
    let eye0 = [c[0] + dir[0], c[1] + dir[1], c[2] + dir[2]];
    let lens = plate_rig_scene(camera_world(eye0, c, [0.0, -1.0, 0.0]));
    let tan_min = lens.tan_half_x.min(lens.tan_half_y);
    let dist = r / tan_min * 1.02;
    let eye = [
        c[0] + dir[0] * dist,
        c[1] + dir[1] * dist,
        c[2] + dir[2] * dist,
    ];
    plate_rig_scene(camera_world(eye, c, [0.0, -1.0, 0.0]))
}

fn landscape(mut scene: ArcScene, width: u32, height: u32) -> ArcScene {
    scene.tan_half_x = scene.tan_half_y * (f64::from(width) / f64::from(height));
    scene
}

/// Field-by-field census of two projected-record
/// lists (GPU snapshot vs CPU projection): how many
/// records differ at all, and the worst f64 delta.
fn record_census(gpu: &[ProjectedRecord], cpu: &[ProjectedRecord]) -> (usize, f64) {
    assert_eq!(gpu.len(), cpu.len(), "record counts differ");
    let mut differing = 0_usize;
    let mut worst = 0.0_f64;
    for (g, c) in gpu.iter().zip(cpu.iter()) {
        if g == c {
            continue;
        }
        differing += 1;
        for (a, b) in g
            .centre_px
            .iter()
            .chain(g.radii.iter())
            .chain(g.axis1.iter())
            .zip(
                c.centre_px
                    .iter()
                    .chain(c.radii.iter())
                    .chain(c.axis1.iter()),
            )
        {
            worst = worst.max((a - b).abs());
        }
    }
    (differing, worst)
}

fn cpu_records(
    cloud: &SplatCloud,
    transform: &SplatTransform,
    scene: &ArcScene,
    width: u32,
    height: u32,
) -> Vec<ProjectedRecord> {
    project_splats(cloud, transform, scene, width, height, None)
        .iter()
        .map(ProjectedRecord::from_projection)
        .collect()
}

fn median(times: &mut [f64]) -> f64 {
    times.sort_by(|a, b| a.total_cmp(b));
    times[times.len() / 2]
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(ply), Some(fixture), Some(out)) = (
        arg_value(&args, "--ply"),
        arg_value(&args, "--fixture"),
        arg_value(&args, "--out"),
    ) else {
        eprintln!("usage: splat-anim-bench --ply PATH --fixture DIR --out DIR");
        return ExitCode::FAILURE;
    };
    let fixture = PathBuf::from(fixture);
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).expect("out dir");

    let probe = VulkanBackend::new().expect("backend");
    println!("adapter: {}", probe.describe());
    drop(probe);

    let bytes = std::fs::read(&ply).expect("read ply");
    println!("asset: {} ({} bytes)", ply, bytes.len());
    println!("asset sha256: {}", sha256_hex(&bytes));
    let t = Instant::now();
    let cloud = SplatCloud::load_ply(&bytes).expect("load ply");
    println!(
        "cloud: {} splats, load {:.1} ms",
        cloud.splats.len(),
        t.elapsed().as_secs_f64() * 1e3
    );
    let (model, toon) = load_toon(&fixture);
    let draw_hidden = rest_draw(&model, false);

    let (width, height) = (512_u32, 640_u32);
    let scene = plate_scene(&cloud);

    // -- CPU reference (config A) ---------------------
    let t = Instant::now();
    let cpu_layer = pack_splats(&cloud, &SplatTransform::default(), &scene, width, height);
    let cpu_pack_ms = t.elapsed().as_secs_f64() * 1e3;
    let mut cpu_scene = pack_scene(&model, &scene);
    cpu_scene.toon = Some(toon.clone());
    cpu_scene.splats = Some(cpu_layer);
    let cpu_hash = {
        let mut backend = VulkanBackend::new().expect("backend");
        backend
            .upload_scene(&cpu_scene, width, height)
            .expect("upload");
        let frame = backend.render_frame(&draw_hidden).expect("render");
        let image = Image::new(frame.width, frame.height, frame.rgba.clone()).expect("image");
        write_png(&out.join("anim-config-a-cpu.png"), &image).expect("write png");
        sha256_hex(&frame.rgba)
    };
    println!("config A CPU pack: {cpu_pack_ms:.1} ms");
    println!("config A CPU frame sha: {cpu_hash}");

    // -- GPU path, static (the witness) ---------------
    let packet = PackedGpuSplats {
        frames: vec![cloud.clone()],
        transform: SplatTransform::default(),
        camera: SplatCamera::from_scene(&scene, width, height),
        shadow: None,
        response: None,
    };
    let mut gpu_scene = pack_scene(&model, &scene);
    gpu_scene.toon = Some(toon.clone());
    gpu_scene.splat_gpu = Some(packet);
    let mut backend = VulkanBackend::new().expect("backend");
    let t = Instant::now();
    backend
        .upload_scene(&gpu_scene, width, height)
        .expect("upload");
    println!(
        "config A GPU upload: {:.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );

    let snapshot = backend.splat_gpu_snapshot().expect("snapshot");
    let expected = cpu_records(&cloud, &SplatTransform::default(), &scene, width, height);
    let (differing, worst) = record_census(&snapshot.records, &expected);
    println!(
        "config A snapshot: {} survivors (CPU {}), {} records differ, worst f64 delta {worst:e}",
        snapshot.survivor_count,
        expected.len(),
        differing
    );

    for _ in 0..3 {
        backend.render_frame(&draw_hidden).expect("warm-up");
    }
    let mut times = Vec::new();
    let mut gpu_hash = String::new();
    for _ in 0..30 {
        let t = Instant::now();
        let frame = backend.render_frame(&draw_hidden).expect("render");
        times.push(t.elapsed().as_secs_f64() * 1e3);
        gpu_hash = sha256_hex(&frame.rgba);
        if times.len() == 1 {
            let image = Image::new(frame.width, frame.height, frame.rgba.clone()).expect("image");
            write_png(&out.join("anim-config-a-gpu.png"), &image).expect("write png");
        }
    }
    println!(
        "config A GPU frame: median {:.2} ms over 30, sha {gpu_hash}",
        median(&mut times)
    );
    println!(
        "config A witness: {}",
        if gpu_hash == cpu_hash {
            "IDENTICAL"
        } else {
            "MISMATCH"
        }
    );

    // -- Rotated layer (the Wigner-D path) ------------
    let rotated = SplatTransform {
        translation: [0.0, 0.0, 0.0],
        rotation: {
            let half = std::f64::consts::FRAC_PI_8; // 22.5 degrees, half of 45
            [half.cos(), half.sin(), 0.0, 0.0] // about X, (w, x, y, z)
        },
        scale: 1.0,
    };
    backend.set_splat_transform(rotated).expect("set transform");
    let snapshot = backend.splat_gpu_snapshot().expect("snapshot");
    let expected = cpu_records(&cloud, &rotated, &scene, width, height);
    let (differing, worst) = record_census(&snapshot.records, &expected);
    println!(
        "rotated snapshot: {} survivors (CPU {}), {} records differ, worst f64 delta {worst:e}",
        snapshot.survivor_count,
        expected.len(),
        differing
    );
    let gpu_rot_hash = sha256_hex(&backend.render_frame(&draw_hidden).expect("render").rgba);
    let cpu_rot_hash = {
        let mut scene2 = pack_scene(&model, &scene);
        scene2.toon = Some(toon.clone());
        scene2.splats = Some(pack_splats(&cloud, &rotated, &scene, width, height));
        let mut b2 = VulkanBackend::new().expect("backend");
        b2.upload_scene(&scene2, width, height).expect("upload");
        sha256_hex(&b2.render_frame(&draw_hidden).expect("render").rgba)
    };
    println!("rotated GPU frame sha: {gpu_rot_hash}");
    println!("rotated CPU frame sha: {cpu_rot_hash}");
    println!(
        "rotated witness: {}",
        if gpu_rot_hash == cpu_rot_hash {
            "IDENTICAL"
        } else {
            "MISMATCH"
        }
    );
    backend
        .set_splat_transform(SplatTransform::default())
        .expect("reset transform");

    // -- Animation sweep (TRS motion, twin) -----------
    let motion = LayerMotion::new(vec![
        TrsKey {
            frame: 0,
            translation: [0.0, 0.0, 0.0],
            rotation: [1.0, 0.0, 0.0, 0.0],
            scale: 1.0,
        },
        TrsKey {
            frame: 29,
            translation: [0.3, 0.1, 0.0],
            rotation: {
                let half = std::f64::consts::PI / 12.0; // 15 degrees, half of 30
                [half.cos(), 0.0, half.sin(), 0.0] // about Y
            },
            scale: 1.1,
        },
    ])
    .expect("motion");
    let sweep = |backend: &mut VulkanBackend| -> (Vec<String>, Vec<f64>) {
        let mut hashes = Vec::new();
        let mut times = Vec::new();
        for f in 0..30_u32 {
            backend
                .set_splat_transform(motion.transform_at(f))
                .expect("set transform");
            let t = Instant::now();
            let frame = backend.render_frame(&draw_hidden).expect("render");
            times.push(t.elapsed().as_secs_f64() * 1e3);
            hashes.push(sha256_hex(&frame.rgba));
        }
        (hashes, times)
    };
    let (hashes1, mut times1) = sweep(&mut backend);
    // Mid-sweep packet gate at frame 10.
    backend
        .set_splat_transform(motion.transform_at(10))
        .expect("set transform");
    let snapshot = backend.splat_gpu_snapshot().expect("snapshot");
    let expected = cpu_records(&cloud, &motion.transform_at(10), &scene, width, height);
    let (differing, worst) = record_census(&snapshot.records, &expected);
    println!(
        "anim frame 10 snapshot: {} survivors (CPU {}), {} records differ, worst f64 delta {worst:e}",
        snapshot.survivor_count,
        expected.len(),
        differing
    );
    let (hashes2, _) = sweep(&mut backend);
    let twin_ok = hashes1 == hashes2;
    println!(
        "anim sweep: median frame {:.2} ms over 30, twin census {}",
        median(&mut times1),
        if twin_ok {
            "IDENTICAL (30/30)"
        } else {
            "MISMATCH"
        }
    );
    println!("anim frame 0 sha: {}", hashes1[0]);
    println!("anim frame 29 sha: {}", hashes1[29]);
    backend
        .set_splat_transform(SplatTransform::default())
        .expect("reset transform");
    drop(backend);

    // -- Resident sequence (two frames) ---------------
    let shifted = {
        let mut c = cloud.clone();
        for s in &mut c.splats {
            s.pos[0] += 0.05;
        }
        c
    };
    let packet = PackedGpuSplats {
        frames: vec![cloud.clone(), shifted.clone()],
        transform: SplatTransform::default(),
        camera: SplatCamera::from_scene(&scene, width, height),
        shadow: None,
        response: None,
    };
    let mut seq_scene = pack_scene(&model, &scene);
    seq_scene.toon = Some(toon.clone());
    seq_scene.splat_gpu = Some(packet);
    let mut backend = VulkanBackend::new().expect("backend");
    backend
        .upload_scene(&seq_scene, width, height)
        .expect("upload");
    for (f, frame_cloud) in [&cloud, &shifted].iter().enumerate() {
        backend.set_splat_sequence_frame(f).expect("set frame");
        let gpu_hash = sha256_hex(&backend.render_frame(&draw_hidden).expect("render").rgba);
        let mut scene2 = pack_scene(&model, &scene);
        scene2.toon = Some(toon.clone());
        scene2.splats = Some(pack_splats(
            frame_cloud,
            &SplatTransform::default(),
            &scene,
            width,
            height,
        ));
        let mut b2 = VulkanBackend::new().expect("backend");
        b2.upload_scene(&scene2, width, height).expect("upload");
        let cpu_hash = sha256_hex(&b2.render_frame(&draw_hidden).expect("render").rgba);
        println!(
            "sequence frame {f}: GPU {gpu_hash} vs CPU {cpu_hash} — {}",
            if gpu_hash == cpu_hash {
                "IDENTICAL"
            } else {
                "MISMATCH"
            }
        );
    }
    drop(backend);

    // -- Judging size frame rate -----------------------
    let (jwidth, jheight) = (1280_u32, 720_u32);
    let jscene = landscape(scene, jwidth, jheight);
    let packet = PackedGpuSplats {
        frames: vec![cloud.clone()],
        transform: SplatTransform::default(),
        camera: SplatCamera::from_scene(&jscene, jwidth, jheight),
        shadow: None,
        response: None,
    };
    let mut jgpu_scene = pack_scene(&model, &jscene);
    jgpu_scene.toon = Some(toon.clone());
    jgpu_scene.splat_gpu = Some(packet);
    let mut backend = VulkanBackend::new().expect("backend");
    backend
        .upload_scene(&jgpu_scene, jwidth, jheight)
        .expect("upload");
    for _ in 0..3 {
        backend.render_frame(&draw_hidden).expect("warm-up");
    }
    let mut times = Vec::new();
    let mut jhash = String::new();
    for _ in 0..30 {
        let t = Instant::now();
        let frame = backend.render_frame(&draw_hidden).expect("render");
        times.push(t.elapsed().as_secs_f64() * 1e3);
        jhash = sha256_hex(&frame.rgba);
    }
    println!(
        "judging 1280x720 GPU frame: median {:.2} ms over 30, sha {jhash}",
        median(&mut times)
    );
    let jcpu_hash = {
        let mut scene2 = pack_scene(&model, &jscene);
        scene2.toon = Some(toon);
        scene2.splats = Some(pack_splats(
            &cloud,
            &SplatTransform::default(),
            &jscene,
            jwidth,
            jheight,
        ));
        let mut b2 = VulkanBackend::new().expect("backend");
        b2.upload_scene(&scene2, jwidth, jheight).expect("upload");
        sha256_hex(&b2.render_frame(&draw_hidden).expect("render").rgba)
    };
    println!("judging CPU frame sha: {jcpu_hash}");
    println!(
        "judging witness: {}",
        if jhash == jcpu_hash {
            "IDENTICAL"
        } else {
            "MISMATCH"
        }
    );

    println!("anim bench complete");
    ExitCode::SUCCESS
}
