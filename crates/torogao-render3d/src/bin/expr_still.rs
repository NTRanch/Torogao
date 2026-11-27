//! Render one still of a toon fixture with an
//! expression/arousal vector applied (toon-layer rung 1,
//! brief sections 3-4): the vector resolves through
//! [`torogao_render3d::expr`] into per-mesh morph
//! weights and toon settings, the frame renders TWICE,
//! and both SHA-256 hashes print — the D-1397
//! determinism witness, same contract as
//! `render3d-toon-still`.
//!
//! Usage:
//!   render3d-expr-still --fixture DIR --out DIR
//!     [--sun right|left|profile]
//!     [--stage S0..S4 | --pos 0..4] [--girl NAME]
//!     [--vector '{"pupil_shape": 0.5}']
//!     [--probe PARAMETER]
//!     [--hide-hair] [--eye x,y,z] [--target x,y,z]
//!     [--label NAME]
//!
//! Every render writes a JSON sidecar beside the PNG:
//! the vector used, the morph weights the resolution
//! actually set, the uniform values applied, and the
//! twin hashes. The pack is built from these sidecars,
//! not from memory.

use std::path::PathBuf;
use std::process::ExitCode;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, RenderBackend, pack_scene};
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::expr::{
    ExprVector, PARAM_NAMES, StageTable, resolve, support,
};
use torogao_render3d::math::{Mat4, V3};
use torogao_render3d::model::{Model, load_model};
use torogao_render3d::setscene::{DirectionalLight, plate_rig_scene};
use torogao_render3d::toon::{ToonData, ToonImage, ToonSettings, ToonTextures};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
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

fn parse_csv3(text: &str) -> Result<[f64; 3], String> {
    let parts: Vec<f64> = text
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("vector {text:?}: {e}"))?;
    if parts.len() != 3 {
        return Err(format!("vector {text:?} wants 3 components"));
    }
    Ok([parts[0], parts[1], parts[2]])
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

struct Fixture {
    model: Model,
    toon: ToonData,
    view: Mat4,
    scene_sun: DirectionalLight,
    scene_fill: DirectionalLight,
    scene_world: [f32; 3],
    width: u32,
    height: u32,
}

fn load_fixture(
    fixture: &std::path::Path,
    sun_side: &str,
    eye_override: Option<[f64; 3]>,
    target_override: Option<[f64; 3]>,
) -> Result<Fixture, String> {
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
            params: None,
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
    let eye = eye_override.unwrap_or_else(|| vec3(&camera["eye"]));
    let target = target_override.unwrap_or_else(|| vec3(&camera["target"]));
    let view = view_matrix(eye, target, vec3(&camera["up"]));
    let lights = &manifest["lights"];
    let sun_key = match sun_side {
        "right" => "sun_to_light_gltf",
        "left" => "sun_left_to_light_gltf",
        _ => "sun_profile_to_light_gltf",
    };
    let sun = vec3(&lights[sun_key]);
    let fill = vec3(&lights["fill_to_light_gltf"]);
    let world = vec3(&lights["world"]);
    Ok(Fixture {
        model,
        toon,
        view,
        scene_sun: DirectionalLight {
            to_light: V3::new(sun[0], sun[1], sun[2]),
            energy: lights["sun_energy"].as_f64().ok_or("sun energy")? as f32,
        },
        scene_fill: DirectionalLight {
            to_light: V3::new(fill[0], fill[1], fill[2]),
            energy: lights["fill_energy"].as_f64().ok_or("fill energy")? as f32,
        },
        scene_world: [world[0] as f32, world[1] as f32, world[2] as f32],
        width: manifest["render"]["width"].as_u64().ok_or("render width")? as u32,
        height: manifest["render"]["height"]
            .as_u64()
            .ok_or("render height")? as u32,
    })
}

fn vector_json(v: &ExprVector) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for name in PARAM_NAMES {
        map.insert(
            name.to_string(),
            serde_json::json!(v.get(name).expect("declared parameter")),
        );
    }
    serde_json::Value::Object(map)
}

fn render_one(
    fixture: &Fixture,
    backend: &mut VulkanBackend,
    vector: &ExprVector,
    hide_hair: bool,
    hide_names: &[String],
    out: &std::path::Path,
    label: &str,
) -> Result<String, String> {
    let resolved = resolve(&fixture.model, &fixture.toon.settings, vector)
        .map_err(|e| e.to_string())?;
    let mut toon = fixture.toon.clone();
    toon.settings = resolved.settings;
    let mut scene = plate_rig_scene(fixture.view.inverse());
    scene.sun = fixture.scene_sun;
    scene.fill = fixture.scene_fill;
    scene.world = fixture.scene_world;
    let mut packed = pack_scene(&fixture.model, &scene);
    packed.toon = Some(toon);
    backend
        .upload_scene(&packed, fixture.width, fixture.height)
        .map_err(|e| e.to_string())?;
    let identity = Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let is_hidden = |name: &str| {
        (hide_hair && name.contains("Hair"))
            || hide_names.iter().any(|h| name.contains(h.as_str()))
    };
    let hidden: Vec<String> = fixture
        .model
        .meshes
        .iter()
        .filter(|m| is_hidden(&m.name))
        .map(|m| m.name.clone())
        .collect();
    let draw = DrawFrame {
        frame: 0,
        joint_matrices: (0..fixture.model.skin.joints.len())
            .map(|_| identity.to_f32_cols())
            .collect(),
        mesh_weights: resolved.mesh_weights.clone(),
        visible_meshes: fixture
            .model
            .meshes
            .iter()
            .map(|m| !is_hidden(&m.name))
            .collect(),
        include_set: false,
        toy_model: identity,
    };
    let first = backend.render_frame(&draw).map_err(|e| e.to_string())?;
    let second = backend.render_frame(&draw).map_err(|e| e.to_string())?;
    let hash_a = sha256_hex(&first.rgba);
    let hash_b = sha256_hex(&second.rgba);
    println!("[{label}] render 1 sha256: {hash_a}");
    println!("[{label}] render 2 sha256: {hash_b}");
    if hash_a != hash_b {
        return Err(format!(
            "[{label}] twin-render hashes differ: determinism witness FAILED"
        ));
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let png_path = out.join(format!("{label}.png"));
    let image = Image::new(first.width, first.height, first.rgba).map_err(|e| e.to_string())?;
    write_png(&png_path, &image).map_err(|e| e.to_string())?;
    let applied: Vec<serde_json::Value> = resolved
        .applied
        .iter()
        .map(|(mesh, target, w)| {
            serde_json::json!({"mesh": mesh, "target": target, "weight": w})
        })
        .collect();
    let s = &resolved.settings;
    let sidecar = serde_json::json!({
        "label": label,
        "vector": vector_json(vector),
        "applied_morph_weights": applied,
        "settings_applied": {
            "blush_strength": s.blush_strength,
            "blush_spread": s.blush_spread,
            "eye_spec_scale": s.eye_spec_scale,
            "tear_sheen": s.tear_sheen,
            "skin_flush": s.skin_flush,
            "skin_spec_scale": s.skin_spec_scale,
            "flush_color": s.flush_color,
        },
        "hidden_meshes": hidden,
        "sha256_rgba": hash_a,
        "twin_identical": true,
        "width": first.width,
        "height": first.height,
    });
    std::fs::write(
        out.join(format!("{label}.json")),
        serde_json::to_string_pretty(&sidecar).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("[{label}] wrote {}", png_path.display());
    Ok(hash_a)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let fixture_dir = PathBuf::from(
        arg_value(&args, "--fixture").ok_or("usage: render3d-expr-still --fixture DIR --out DIR")?,
    );
    let out = PathBuf::from(arg_value(&args, "--out").ok_or("--out DIR is required")?);
    let sun_side = arg_value(&args, "--sun").unwrap_or_else(|| "right".to_string());
    if sun_side != "right" && sun_side != "left" && sun_side != "profile" {
        return Err(format!("--sun wants right|left|profile, got {sun_side:?}"));
    }
    let hide_hair = has_flag(&args, "--hide-hair");
    let hide_names: Vec<String> = match arg_value(&args, "--hide") {
        Some(text) => text.split(',').map(|s| s.trim().to_string()).collect(),
        None => Vec::new(),
    };
    let eye = match arg_value(&args, "--eye") {
        Some(text) => Some(parse_csv3(&text)?),
        None => None,
    };
    let target = match arg_value(&args, "--target") {
        Some(text) => Some(parse_csv3(&text)?),
        None => None,
    };
    let fixture = load_fixture(&fixture_dir, &sun_side, eye, target)?;

    for (name, sup) in support(&fixture.model) {
        println!("support {name}: {sup:?}");
    }

    let mut backend = VulkanBackend::new().map_err(|e| e.to_string())?;
    println!("backend: {}", backend.describe());

    if let Some(param) = arg_value(&args, "--probe") {
        if !PARAM_NAMES.contains(&param.as_str()) {
            return Err(format!("--probe: unknown parameter {param:?}"));
        }
        // The probe pair: the parameter at its two
        // extremes over the NEUTRAL base, everything
        // else held. pupil_dilate also renders its
        // neutral midpoint, since only one half of its
        // range carries a mechanism on this rig.
        let mut values = vec![0.0_f32, 1.0_f32];
        if param == "pupil_dilate" {
            values.push(0.5);
        }
        for value in values {
            let mut v = ExprVector::NEUTRAL;
            v.set(&param, value).map_err(|e| e.to_string())?;
            let tag = if value == 0.5 {
                "mid".to_string()
            } else {
                format!("{value:.0}")
            };
            let label = format!("probe_{param}_{tag}");
            render_one(&fixture, &mut backend, &v, hide_hair, &hide_names, &out, &label)?;
        }
        return Ok(());
    }

    let (vector, default_label) = if let Some(text) = arg_value(&args, "--vector") {
        let doc: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("--vector: {e}"))?;
        let mut v = ExprVector::NEUTRAL;
        let obj = doc
            .as_object()
            .ok_or("--vector wants a JSON object of parameter values")?;
        for (key, val) in obj {
            let f = val
                .as_f64()
                .ok_or_else(|| format!("--vector: {key} is not a number"))?
                as f32;
            v.set(key, f).map_err(|e| e.to_string())?;
        }
        v.validate().map_err(|e| e.to_string())?;
        (v, "vector".to_string())
    } else {
        let table = StageTable::embedded().map_err(|e| e.to_string())?;
        let girl = arg_value(&args, "--girl");
        let girl_ref = girl.as_deref();
        if let Some(stage) = arg_value(&args, "--stage") {
            let idx = torogao_render3d::expr::STAGE_NAMES
                .iter()
                .position(|n| *n == stage)
                .ok_or_else(|| format!("--stage wants S0..S4, got {stage:?}"))?;
            let v = table.vector(idx, girl_ref).map_err(|e| e.to_string())?;
            (v, format!("{stage}_{}", girl.as_deref().unwrap_or("shared")))
        } else {
            let pos: f32 = arg_value(&args, "--pos")
                .unwrap_or_else(|| "0".to_string())
                .parse()
                .map_err(|e| format!("--pos: {e}"))?;
            let v = table.vector_at(pos, girl_ref).map_err(|e| e.to_string())?;
            (v, format!("pos{pos}_{}", girl.as_deref().unwrap_or("shared")))
        }
    };
    let label = arg_value(&args, "--label").unwrap_or(default_label);
    render_one(&fixture, &mut backend, &vector, hide_hair, &hide_names, &out, &label)?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(detail) => {
            eprintln!("expr-still: {detail}");
            ExitCode::FAILURE
        }
    }
}
