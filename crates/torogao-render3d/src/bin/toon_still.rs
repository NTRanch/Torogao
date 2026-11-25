//! Render one rest-pose still of the G1 toon parity
//! fixture through the Vulkan backend: the fixture GLB
//! (Prima retrofit + contract channels) with the toon
//! model on, under the fixture's declared light rig.
//!
//! Usage:
//!   toon-still --fixture DIR --out DIR [--sun right|left]
//!
//! The frame renders TWICE and both SHA-256 hashes print:
//! the D-1397 determinism witness (identical inputs must
//! produce a bit-identical frame). The PNG lands in the
//! out dir as `engine_key_<sun>.png`.

use std::path::PathBuf;
use std::process::ExitCode;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, RenderBackend, pack_scene};
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::math::{Mat4, V3};
use torogao_render3d::model::load_model;
use torogao_render3d::setscene::{DirectionalLight, plate_rig_scene};
use torogao_render3d::toon::{ToonData, ToonImage, ToonSettings, ToonTextures};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn vec3(value: &serde_json::Value) -> [f64; 3] {
    let a = value.as_array().expect("manifest vector");
    [
        a[0].as_f64().expect("manifest vector x"),
        a[1].as_f64().expect("manifest vector y"),
        a[2].as_f64().expect("manifest vector z"),
    ]
}

fn vec3f(value: &serde_json::Value) -> [f32; 3] {
    let v = vec3(value);
    [v[0] as f32, v[1] as f32, v[2] as f32]
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

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalized(a: [f64; 3]) -> [f64; 3] {
    let len = dot(a, a).sqrt();
    [a[0] / len, a[1] / len, a[2] / len]
}

/// World -> camera view matrix for an eye/target/up
/// camera (glTF space, camera looks along its -Z).
fn view_matrix(eye: [f64; 3], target: [f64; 3], up: [f64; 3]) -> Mat4 {
    let forward = normalized(sub(target, eye));
    let right = normalized(cross(forward, up));
    let true_up = cross(right, forward);
    Mat4::from_rows([
        [right[0], right[1], right[2], -dot(right, eye)],
        [true_up[0], true_up[1], true_up[2], -dot(true_up, eye)],
        [-forward[0], -forward[1], -forward[2], dot(forward, eye)],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let fixture = PathBuf::from(
        arg_value(&args, "--fixture").ok_or("usage: toon-still --fixture DIR --out DIR")?,
    );
    let out = PathBuf::from(arg_value(&args, "--out").ok_or("--out DIR is required")?);
    let sun_side = arg_value(&args, "--sun").unwrap_or_else(|| "right".to_string());
    if sun_side != "right" && sun_side != "left" && sun_side != "profile" {
        return Err(format!("--sun wants right|left|profile, got {sun_side:?}"));
    }
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fixture.join("manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let glb_path = fixture.join(manifest["glb"].as_str().ok_or("manifest glb")?);
    let bytes = std::fs::read(&glb_path).map_err(|e| e.to_string())?;
    let model = load_model(&bytes).map_err(|e| e.to_string())?;

    let tex = |name: &str| -> Result<ToonImage, String> {
        let entry = &manifest["textures"][name];
        let file = entry["file"].as_str().ok_or("texture file")?;
        let width = entry["width"].as_u64().ok_or("texture width")? as u32;
        let height = entry["height"].as_u64().ok_or("texture height")? as u32;
        ToonImage::load_rgba(&fixture.join(file), width, height).map_err(|e| e.to_string())
    };
    let head = &manifest["head"];
    let toon = ToonData {
        textures: ToonTextures {
            packed: tex("packed")?,
            ramp: tex("ramp")?,
            face_sdf: tex("face_sdf")?,
            sphere: tex("sphere")?,
        },
        settings: ToonSettings {
            head_joint: head["joint_index"].as_i64().ok_or("head joint")? as i32,
            head_forward_local: vec3f(&head["forward_local"]),
            head_right_local: vec3f(&head["right_local"]),
            ..ToonSettings::default()
        },
    };
    toon.validate().map_err(|e| e.to_string())?;

    let camera = &manifest["camera_gltf"];
    let view = view_matrix(
        vec3(&camera["eye"]),
        vec3(&camera["target"]),
        vec3(&camera["up"]),
    );
    let mut scene = plate_rig_scene(view.inverse());
    let lights = &manifest["lights"];
    let sun_key = match sun_side.as_str() {
        "right" => "sun_to_light_gltf",
        "left" => "sun_left_to_light_gltf",
        _ => "sun_profile_to_light_gltf",
    };
    let sun = vec3(&lights[sun_key]);
    let fill = vec3(&lights["fill_to_light_gltf"]);
    let world = vec3(&lights["world"]);
    scene.sun = DirectionalLight {
        to_light: V3::new(sun[0], sun[1], sun[2]),
        energy: lights["sun_energy"].as_f64().ok_or("sun energy")? as f32,
    };
    scene.fill = DirectionalLight {
        to_light: V3::new(fill[0], fill[1], fill[2]),
        energy: lights["fill_energy"].as_f64().ok_or("fill energy")? as f32,
    };
    scene.world = [world[0] as f32, world[1] as f32, world[2] as f32];

    let mut packed = pack_scene(&model, &scene);
    packed.toon = Some(toon);
    let width = manifest["render"]["width"].as_u64().ok_or("render width")? as u32;
    let height = manifest["render"]["height"]
        .as_u64()
        .ok_or("render height")? as u32;

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

    let mut backend = VulkanBackend::new().map_err(|e| e.to_string())?;
    println!("backend: {}", backend.describe());
    backend
        .upload_scene(&packed, width, height)
        .map_err(|e| e.to_string())?;
    let first = backend.render_frame(&draw).map_err(|e| e.to_string())?;
    let second = backend.render_frame(&draw).map_err(|e| e.to_string())?;
    let hash_a = sha256_hex(&first.rgba);
    let hash_b = sha256_hex(&second.rgba);
    println!("render 1 sha256: {hash_a}");
    println!("render 2 sha256: {hash_b}");
    if hash_a != hash_b {
        return Err("twin-render hashes differ: determinism witness FAILED".to_string());
    }
    println!("twin-render determinism: IDENTICAL");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let png_path = out.join(format!("engine_key_{sun_side}.png"));
    let image = Image::new(first.width, first.height, first.rgba).map_err(|e| e.to_string())?;
    write_png(&png_path, &image).map_err(|e| e.to_string())?;
    println!("wrote {}", png_path.display());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(detail) => {
            eprintln!("toon-still: {detail}");
            ExitCode::FAILURE
        }
    }
}
