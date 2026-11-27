//! Alpha-overlay (EyeShadowFilm) proving test.
//!
//! The route's contract, proven on the Rung 8 parity
//! bundle (the canonical base's real GLB export):
//!
//! 1. Input contract: exactly one material carries the
//!    blend class — the EyeShadow, decoded from the
//!    GLB's `alphaMode: "BLEND"` — and the film mesh
//!    wears it.
//! 2. Full scene, three ways: film blended (the route),
//!    film opaque (the pre-route behaviour, by flipping
//!    the packed material's blend class back), film
//!    hidden. Blending guarantees the blended value
//!    lies BETWEEN the opaque and hidden values on
//!    every channel; the test asserts that invariant
//!    on every pixel (1 level of UNORM rounding slack),
//!    plus a byte-identical twin render.
//! 3. Film alone over the clear background: the same
//!    trio reduced to blend vs opaque over a known
//!    backdrop, where the "over" identity
//!    `A = a * B + (1 - a) * C` can be solved per pixel
//!    for the one alpha shared by all channels. The
//!    implied alpha must sit inside the physical range
//!    and actually land (the film's authored band peaks
//!    well above zero).
//!
//! Why the algebra is proven on the film alone: at the
//! bundle-face framing the film's frontmost sliver
//! over the face carries authored alpha <= 0.012
//! (measured on this bundle: the film's alpha band
//! rasterizes across both eyes, but in the full scene
//! the eye structures cover it, and the sliver that
//! stays frontmost is the gradient's transparent
//! tail), so over the face the blend is below UNORM
//! quantization and no per-pixel identity can be
//! checked there. The film-only surface carries the
//! same mesh, the same atlas, and the same pipeline —
//! it is the decisive surface, and the full-scene
//! betweenness invariant covers the compositing.
//!
//! Ignored like the other proving tests: it needs a
//! Vulkan device and the bundle directory in
//! TOROGAO_RENDER3D_FILM_BUNDLE (the R8 bundle-face dir
//! of the Prima canonical-base tree).

use std::path::PathBuf;

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, PackedScene, RenderBackend, RenderedFrame, pack_scene};
use torogao_render3d::compare::sha256_hex;
use torogao_render3d::math::{Mat4, V3};
use torogao_render3d::model::load_model;
use torogao_render3d::setscene::{DirectionalLight, plate_rig_scene};
use torogao_render3d::toon::{ToonData, ToonImage, ToonSettings, ToonTextures};

fn identity() -> Mat4 {
    Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
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
/// camera (glTF space, camera looks along its -Z). The
/// toon-still driver's matrix, duplicated by design —
/// proving tests do not share helpers with binaries.
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

fn render(packed: &PackedScene, draw: &DrawFrame, width: u32, height: u32) -> RenderedFrame {
    let mut backend = VulkanBackend::new().expect("backend");
    backend.upload_scene(packed, width, height).expect("upload");
    backend.render_frame(draw).expect("render")
}

#[test]
#[ignore = "needs a Vulkan device and TOROGAO_RENDER3D_FILM_BUNDLE"]
fn film_overlay_composites_over_the_face() {
    let bundle =
        PathBuf::from(std::env::var("TOROGAO_RENDER3D_FILM_BUNDLE").expect("bundle dir env var"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(bundle.join("manifest.json")).unwrap()).unwrap();
    let glb_path = bundle.join(manifest["glb"].as_str().expect("manifest glb"));
    let bytes = std::fs::read(&glb_path).unwrap();
    let model = load_model(&bytes).unwrap();

    // The route's input contract on the real artefact:
    // exactly one blend-class material, the EyeShadow,
    // and the film mesh wears it.
    let overlay_materials: Vec<usize> = model
        .materials
        .iter()
        .enumerate()
        .filter(|(_, m)| m.alpha_overlay)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(overlay_materials.len(), 1, "one blend-class material");
    assert_eq!(model.materials[overlay_materials[0]].name, "EyeShadow");
    let film_mesh = model
        .meshes
        .iter()
        .position(|m| m.name.contains("EyeShadowFilm"))
        .expect("film mesh present");
    assert_eq!(model.meshes[film_mesh].material, overlay_materials[0]);

    let tex = |name: &str| {
        let entry = &manifest["textures"][name];
        ToonImage::load_rgba(
            &bundle.join(entry["file"].as_str().unwrap()),
            entry["width"].as_u64().unwrap() as u32,
            entry["height"].as_u64().unwrap() as u32,
        )
        .unwrap()
    };
    let head = &manifest["head"];
    let toon = ToonData {
        textures: ToonTextures {
            packed: tex("packed"),
            ramp: tex("ramp"),
            face_sdf: tex("face_sdf"),
            sphere: tex("sphere"),
            params: None,
        },
        settings: ToonSettings {
            head_joint: head["joint_index"].as_i64().unwrap() as i32,
            head_forward_local: vec3f(&head["forward_local"]),
            head_right_local: vec3f(&head["right_local"]),
            ..ToonSettings::default()
        },
    };
    toon.validate().unwrap();

    let camera = &manifest["camera_gltf"];
    let view = view_matrix(
        vec3(&camera["eye"]),
        vec3(&camera["target"]),
        vec3(&camera["up"]),
    );
    let mut scene = plate_rig_scene(view.inverse());
    let lights = &manifest["lights"];
    let sun = vec3(&lights["sun_to_light_gltf"]);
    let fill = vec3(&lights["fill_to_light_gltf"]);
    let world = vec3(&lights["world"]);
    scene.sun = DirectionalLight {
        to_light: V3::new(sun[0], sun[1], sun[2]),
        energy: lights["sun_energy"].as_f64().unwrap() as f32,
    };
    scene.fill = DirectionalLight {
        to_light: V3::new(fill[0], fill[1], fill[2]),
        energy: lights["fill_energy"].as_f64().unwrap() as f32,
    };
    scene.world = [world[0] as f32, world[1] as f32, world[2] as f32];

    let width = manifest["render"]["width"].as_u64().unwrap() as u32;
    let height = manifest["render"]["height"].as_u64().unwrap() as u32;

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

    // -- Full scene -----------------------------------
    // A: the route. A2: its twin (determinism witness).
    let blended = render(&packed, &draw, width, height);
    let blended_twin = render(&packed, &draw, width, height);
    assert_eq!(
        sha256_hex(&blended.rgba),
        sha256_hex(&blended_twin.rgba),
        "route render must be deterministic"
    );

    // B: the pre-route behaviour — the same scene with
    // the film's material flipped back to opaque.
    let mut opaque_packed = packed.clone();
    opaque_packed.materials[overlay_materials[0]].alpha_overlay = false;
    let opaque = render(&opaque_packed, &draw, width, height);

    // C: the film hidden — what the film composites over.
    let mut hidden_draw = draw.clone();
    hidden_draw.visible_meshes[film_mesh] = false;
    let hidden = render(&packed, &hidden_draw, width, height);

    assert_eq!(blended.rgba.len(), opaque.rgba.len());
    assert_eq!(blended.rgba.len(), hidden.rgba.len());

    // Betweenness: with alpha in [0, 1] the blend lies
    // between the opaque and hidden values per channel.
    // 1 level of slack for UNORM rounding.
    let pixels = blended.rgba.len() / 4;
    let mut footprint = 0usize;
    let mut between_violations = 0usize;
    for px in 0..pixels {
        let base = px * 4;
        let mut differs = false;
        for ch in 0..3 {
            let a = i32::from(blended.rgba[base + ch]);
            let b = i32::from(opaque.rgba[base + ch]);
            let c = i32::from(hidden.rgba[base + ch]);
            if a != c {
                differs = true;
            }
            let (lo, hi) = if b <= c { (b, c) } else { (c, b) };
            if a < lo - 1 || a > hi + 1 {
                between_violations += 1;
            }
        }
        if differs {
            footprint += 1;
        }
    }
    println!(
        "full scene: film footprint {footprint} px, betweenness violations {between_violations}"
    );
    assert_eq!(
        between_violations, 0,
        "every blended channel must lie between its opaque and hidden values"
    );

    // -- Film alone over the clear background ---------
    let mut draw_film = draw.clone();
    draw_film.visible_meshes = vec![false; model.meshes.len()];
    draw_film.visible_meshes[film_mesh] = true;
    let film_blend = render(&packed, &draw_film, width, height);
    let film_opaque = render(&opaque_packed, &draw_film, width, height);

    // The backdrop both film renders composite over:
    // the cleared corner pixel, identical in both.
    let bg: [i32; 3] = [
        i32::from(film_blend.rgba[0]),
        i32::from(film_blend.rgba[1]),
        i32::from(film_blend.rgba[2]),
    ];
    assert_eq!(
        bg,
        [
            i32::from(film_opaque.rgba[0]),
            i32::from(film_opaque.rgba[1]),
            i32::from(film_opaque.rgba[2]),
        ],
        "both film renders share the cleared backdrop"
    );

    let mut film_footprint = 0usize;
    let mut checked = 0usize;
    let mut violations = 0usize;
    let mut implied: Vec<f32> = Vec::new();
    for px in 0..pixels {
        let base = px * 4;
        let mut covered = false;
        let mut alphas: Vec<f32> = Vec::new();
        for ch in 0..3 {
            let o = i32::from(film_opaque.rgba[base + ch]);
            let b = i32::from(film_blend.rgba[base + ch]);
            if o != bg[ch] {
                covered = true;
            }
            let span = o - bg[ch];
            if span.abs() >= 24 {
                alphas.push((b - bg[ch]) as f32 / span as f32);
            }
        }
        if covered {
            film_footprint += 1;
        }
        if alphas.len() >= 2 {
            checked += 1;
            let lo = alphas.iter().copied().fold(f32::INFINITY, f32::min);
            let hi = alphas.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let mid = alphas.iter().sum::<f32>() / alphas.len() as f32;
            implied.push(mid);
            if !(-0.03..=1.02).contains(&mid) || hi - lo > 0.08 {
                violations += 1;
            }
        }
    }
    implied.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let median = implied[implied.len() / 2];
    let max_implied = *implied.last().unwrap();
    println!(
        "film alone: footprint {film_footprint} px; algebra-checked {checked} px; \
         violations {violations}; implied alpha median {median:.3}, max {max_implied:.3}"
    );
    assert!(film_footprint > 4_000, "the film must cover a real band");
    assert!(checked > 500, "enough high-signal pixels to prove on");
    assert!(
        violations * 100 <= checked,
        "at most 1% of checked pixels may break the over identity"
    );
    assert!(
        (0.01..=0.60).contains(&median),
        "median implied alpha {median} in the authored range"
    );
    assert!(
        max_implied >= 0.30,
        "the film's peak alpha must actually land (max {max_implied})"
    );
}
