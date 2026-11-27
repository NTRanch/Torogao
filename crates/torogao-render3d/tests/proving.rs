//! Proving tests over the staged Prima inputs. These are
//! `#[ignore]`d because they need the asset directory:
//! run them with
//!   TOROGAO_RENDER3D_TEST_INPUTS=<inputs dir> \
//!     cargo test -p torogao-render3d --test proving -- --ignored
//! The pure unit tests (math, CSR packing, scene
//! generators, garbage-input handling) live in the modules
//! and always run.

use std::path::PathBuf;

use torogao_render3d::arc::{PLATE_BUST_WEIGHT, load_arc_inputs};
use torogao_render3d::math::V3;
use torogao_render3d::morph::PackedMorphs;
use torogao_render3d::reference::skin_vertex;
use torogao_render3d::setscene::{OutfitSelection, PLATE_FRAMES, outfit_for_frame};

fn inputs() -> PathBuf {
    let dir = std::env::var("TOROGAO_RENDER3D_TEST_INPUTS").expect(
        "set TOROGAO_RENDER3D_TEST_INPUTS to the staged inputs dir (prima.glb, clip, def, channels, scene-probe.json)",
    );
    PathBuf::from(dir)
}

/// The gap analysis census, as decoded by this crate.
#[test]
#[ignore = "needs staged Prima inputs"]
fn prima_census_matches_gap_analysis() {
    let arc = load_arc_inputs(&inputs()).expect("inputs load");
    let model = &arc.model;
    assert_eq!(model.meshes.len(), 4);
    let total_verts: usize = model.meshes.iter().map(|m| m.vertex_count()).sum();
    assert_eq!(total_verts, 22_340);
    assert_eq!(model.skin.joints.len(), 54);
    let body = model.mesh_named("_Body").expect("body");
    assert_eq!(body.vertex_count(), 13_920);
    assert_eq!(body.targets.len(), 37);
    assert_eq!(model.mesh_named("_Hair").unwrap().targets.len(), 0);
    assert_eq!(model.mesh_named("_Outfit").unwrap().targets.len(), 1);
    assert_eq!(model.mesh_named("_OutfitEnd").unwrap().targets.len(), 1);
    assert!(body.has_color1);
    // Sparse stored-entry census per body target (the
    // gate's PARITY numbers; Fac_Eye_Close = 232 is the
    // recorded witness).
    let census: [usize; 37] = [
        232, 116, 116, 116, 116, 232, 152, 152, 792, 792, 80, 100, 100, 100, 100, 64, 55, 55, 55,
        55, 55, 55, 55, 55, 55, 36, 55, 18, 18, 18, 18, 18, 18, 18, 18, 58, 1480,
    ];
    for (target, expected) in body.targets.iter().zip(census.iter()) {
        assert_eq!(target.stored_position_entries, *expected);
    }
    let total_stored: usize = census.iter().sum();
    assert_eq!(total_stored, 5_628);
    // Materials: ExportVC (roughness 0.5) + Hair (0.85),
    // metallic 0, double-sided.
    assert_eq!(model.materials.len(), 2);
    assert_eq!(model.materials[0].roughness, 0.5);
    assert_eq!(model.materials[1].roughness, 0.85);
    assert!(
        model
            .materials
            .iter()
            .all(|m| m.metallic == 0.0 && m.double_sided)
    );
}

/// GLB joint-node rest TRS == skeleton-def rest TRS, all
/// 54 joints (the binding the pose layer relies on).
#[test]
#[ignore = "needs staged Prima inputs"]
fn def_rest_matches_glb_nodes() {
    use torogao_motion_clip::SkeletonDef;
    let arc = load_arc_inputs(&inputs()).expect("inputs load");
    for (bone, glb_name) in arc
        .skeleton
        .bones()
        .iter()
        .zip(arc.skeleton.glb_names.iter())
    {
        let node = &arc.model.nodes[arc.model.node_named(glb_name).expect("node")];
        for i in 0..3 {
            assert!(
                (node.translation[i] - bone.rest_translation[i]).abs() < 1e-9,
                "{} translation",
                bone.name
            );
            assert!(
                (node.scale[i] - bone.rest_scale[i]).abs() < 1e-9,
                "{} scale",
                bone.name
            );
        }
        for i in 0..4 {
            assert!(
                (node.rotation[i] - bone.rest_rotation[i]).abs() < 1e-9,
                "{} rotation",
                bone.name
            );
        }
    }
}

/// The clip replays to the recorded hash — the whole
/// binding + declaration stack (channel ranges included)
/// is arbitrated by this one number.
#[test]
#[ignore = "needs staged Prima inputs"]
fn replay_hash_matches_record() {
    let arc = load_arc_inputs(&inputs()).expect("inputs load");
    let bound = arc.bind().expect("bind");
    assert_eq!(arc.clip.frames, 900);
    assert_eq!(
        bound.replay_hash(),
        "1bd9dd534a0ea37f8e827e5d9e7fcdcac3d8cebfd53401c3cd05b9be46e986eb"
    );
}

/// The CPU reference against Blender's own evaluated
/// vertices (probe4 strain witnesses, full precision).
/// Frames chosen at exact clip keys (plate k with 2.5k an
/// integer). Tolerance 2e-3 m: Blender evaluates with its
/// cage Mesh Deform at rest and its own armature code;
/// structural errors (wrong joint/weight/delta) are
/// centimetre-scale, so 2 mm still discriminates.
///
/// INDEX NOTE (measured rung 1): the witnesses are
/// indices into the blend's SPARSE body mesh (3,084
/// verts); the GLB body is the dense export (13,920).
/// The sparse indices do NOT carry over — sparse vert 11
/// is the mirror twin of GLB vert 11. The witnesses were
/// re-keyed to GLB indices by exact base-position match:
/// sparse 11 -> GLB 72, sparse 35 -> GLB 229 (base
/// positions agree to < 5e-7 m).
#[test]
#[ignore = "needs staged Prima inputs"]
fn cpu_reference_matches_blender_witnesses() {
    let arc = load_arc_inputs(&inputs()).expect("inputs load");
    let bound = arc.bind().expect("bind");
    let body_index = arc
        .model
        .meshes
        .iter()
        .position(|m| m.name == "Prima_Body")
        .unwrap();
    let packed = PackedMorphs::pack(&arc.model.meshes[body_index]);
    // (frame, vert11 glTF, vert35 glTF) — Blender (x,y,z)
    // converted by (x, z, -y).
    let witnesses: [(u32, [f64; 3], [f64; 3]); 4] = [
        (
            0,
            [-0.050228, 1.024414, 0.074556],
            [-0.065683, 1.013136, 0.070493],
        ),
        (
            24,
            [-0.050228, 1.023586, 0.081658],
            [-0.065683, 1.012396, 0.077360],
        ),
        (
            60,
            [-0.050228, 1.024910, 0.069524],
            [-0.065683, 1.013573, 0.065628],
        ),
        (
            84,
            [-0.050228, 1.024910, 0.069524],
            [-0.065683, 1.013573, 0.065628],
        ),
    ];
    for (frame, want11, want35) in witnesses {
        let evaluated = arc.pose.evaluate_plate_frame(&bound, frame).expect("eval");
        let joints = arc.pose.joint_matrices(&arc.model, &evaluated);
        let weights = arc.frame_weights(&bound, frame).expect("weights")[body_index].clone();
        for (vertex, want) in [(72_usize, want11), (229, want35)] {
            let (pos, _) = skin_vertex(&arc.model, body_index, &packed, &joints, &weights, vertex);
            let delta = pos.minus(V3::new(want[0], want[1], want[2])).length();
            println!("frame {frame} vert {vertex}: delta {delta:.2e} m");
            assert!(
                delta < 2e-3,
                "frame {frame} vert {vertex}: delta {delta} m vs Blender witness"
            );
        }
    }
}

/// The bust override: GLB saves 1.0; the plates were
/// rendered at 0.0; the frame weights must carry 0.0.
#[test]
#[ignore = "needs staged Prima inputs"]
fn bust_override_applies() {
    let arc = load_arc_inputs(&inputs()).expect("inputs load");
    let bound = arc.bind().expect("bind");
    let body = arc.model.mesh_named("_Body").unwrap();
    assert_eq!(body.default_weights[36], 1.0);
    let outfit = arc.model.mesh_named("_Outfit").unwrap();
    assert_eq!(outfit.default_weights[0], 1.0);
    for frame in [0_u32, 100, 200, 359] {
        let weights = arc.frame_weights(&bound, frame).expect("weights");
        let body_w = &weights[arc
            .model
            .meshes
            .iter()
            .position(|m| m.name == "Prima_Body")
            .unwrap()];
        assert_eq!(body_w[36], PLATE_BUST_WEIGHT);
    }
}

/// The transcribed visibility schedule vs the probe's
/// evaluated per-frame visibility, all 360 frames.
#[test]
#[ignore = "needs staged Prima inputs"]
fn schedule_matches_probe_visibility() {
    let arc = load_arc_inputs(&inputs()).expect("inputs load");
    let probe_vis = &arc.probe.visibility;
    assert_eq!(probe_vis.len(), PLATE_FRAMES as usize);
    let piece = |name: &str| arc.scene.pieces.iter().find(|p| p.name == name).unwrap();
    for frame in 0..PLATE_FRAMES {
        let vis = &probe_vis[frame as usize];
        assert_eq!(
            piece("HypnoScreen").visible(frame),
            vis["HypnoScreen"],
            "screen frame {frame}"
        );
        assert_eq!(
            piece("SpreaderBar").visible(frame),
            vis["SpreaderBar"],
            "bar frame {frame}"
        );
        assert_eq!(
            outfit_for_frame(frame) == OutfitSelection::Outfit,
            vis["Prima_Outfit"],
            "outfit frame {frame}"
        );
        assert_eq!(
            outfit_for_frame(frame) == OutfitSelection::OutfitEnd,
            vis["Prima_OutfitEnd"],
            "outfitend frame {frame}"
        );
    }
}

/// COLOR_1 evidence: what the second colour set actually
/// holds, decoded through the strided accessor path. The
/// rung's carried REVIEW resolves here — COLOR_1 is the
/// paint set (see the test body for the pinned evidence).
#[test]
#[ignore = "needs staged Prima inputs"]
fn color1_evidence() {
    // RESOLVED (rung 1, measured): COLOR_1 is the paint
    // set; COLOR_0 is constant white. Evidence pinned by
    // this test: (1) body COLOR_0 is (1,1,1,1) everywhere;
    // (2) hair COLOR_1's mean equals the Hair material's
    // baseColorFactor (0.797, 0.768, 0.869) — the builder
    // baked material colours into COLOR_1; (3) outfit
    // COLOR_1 reaches near-black (the skirt) while body
    // COLOR_1 sits at warm skin values. The renderer's
    // packing therefore feeds COLOR_1 as the albedo paint
    // (material factor not re-applied).
    let arc = load_arc_inputs(&inputs()).expect("inputs load");
    let body = arc.model.mesh_named("_Body").unwrap();
    for c0 in &body.color0 {
        assert_eq!(*c0, [1.0, 1.0, 1.0, 1.0], "body COLOR_0 must be white");
    }
    let mut min = [f32::MAX; 4];
    let mut max = [f32::MIN; 4];
    for c1 in &body.color1 {
        for i in 0..4 {
            assert!(c1[i].is_finite(), "COLOR_1 not finite");
            min[i] = min[i].min(c1[i]);
            max[i] = max[i].max(c1[i]);
        }
    }
    println!("body COLOR_1 ranges {min:?} .. {max:?}");
    let hair = arc.model.mesh_named("_Hair").unwrap();
    let n = hair.vertex_count() as f64;
    let mean: [f64; 3] = [
        hair.color1.iter().map(|c| f64::from(c[0])).sum::<f64>() / n,
        hair.color1.iter().map(|c| f64::from(c[1])).sum::<f64>() / n,
        hair.color1.iter().map(|c| f64::from(c[2])).sum::<f64>() / n,
    ];
    let factor = [0.797_f64, 0.768, 0.869];
    for i in 0..3 {
        assert!(
            (mean[i] - factor[i]).abs() < 2e-3,
            "hair COLOR_1 mean {mean:?} vs material factor {factor:?}"
        );
    }
    let outfit = arc.model.mesh_named("_Outfit").unwrap();
    let darkest = outfit
        .color1
        .iter()
        .map(|c| c[0] + c[1] + c[2])
        .fold(f32::MAX, f32::min);
    assert!(
        darkest < 0.15,
        "outfit COLOR_1 has a near-black region (skirt)"
    );
}

/// Sparse-decode spot fixtures (independent python GLB
/// read): the max-delta vertices of two targets.
#[test]
#[ignore = "needs staged Prima inputs"]
fn morph_spot_fixtures() {
    let arc = load_arc_inputs(&inputs()).expect("inputs load");
    let body = arc.model.mesh_named("_Body").unwrap();
    let eye_close = &body.targets[body.target_index("Fac_Eye_Close").unwrap()];
    let entry = eye_close
        .entries
        .iter()
        .find(|(v, _, _)| *v == 4525)
        .expect("vert 4525 in Fac_Eye_Close");
    let want = [0.0, -0.023936, 0.002298];
    for (got, want) in entry.1.iter().zip(want) {
        assert!((got - want).abs() < 1e-6, "eye close delta");
    }
    let bust = &body.targets[body.target_index("Body_Bust_Size").unwrap()];
    let entry = bust
        .entries
        .iter()
        .find(|(v, _, _)| *v == 992)
        .expect("vert 992 in Body_Bust_Size");
    assert!((entry.1[2] - 0.015).abs() < 1e-6, "bust delta");
}

/// GPU smoke: two renders of frame 0 are bit-identical,
/// and the GPU skin readback agrees with the CPU
/// reference inside the cross-check bound.
#[test]
#[ignore = "needs staged Prima inputs and a Vulkan device"]
fn gpu_smoke_determinism_and_skin() {
    use torogao_render3d::backend::RenderBackend;
    use torogao_render3d::backend::vulkan::VulkanBackend;
    use torogao_render3d::compare::sha256_hex;
    use torogao_render3d::setscene::{PLATE_HEIGHT, PLATE_WIDTH};

    let arc = load_arc_inputs(&inputs()).expect("inputs load");
    let bound = arc.bind().expect("bind");
    let packed = arc.packed_scene();
    let mut backend = VulkanBackend::new().expect("backend");
    println!("device: {}", backend.describe());
    backend
        .upload_scene(&packed, PLATE_WIDTH, PLATE_HEIGHT)
        .expect("upload");
    let draw = arc.draw_frame(&bound, 0, true).expect("draw");
    let a = backend.render_frame(&draw).expect("render a");
    let b = backend.render_frame(&draw).expect("render b");
    assert_eq!(
        sha256_hex(&a.rgba),
        sha256_hex(&b.rgba),
        "frame 0 twin hashes"
    );

    let body_index = arc
        .model
        .meshes
        .iter()
        .position(|m| m.name == "Prima_Body")
        .unwrap();
    let joints = arc.joint_matrices_f32(&bound, 0).expect("joints");
    let weights = arc.frame_weights(&bound, 0).expect("weights")[body_index].clone();
    let (gpu_pos, _gpu_nrm) = backend
        .skin_positions(body_index, &joints, &weights)
        .expect("skin readback");
    let evaluated = arc.pose.evaluate_plate_frame(&bound, 0).expect("eval");
    let joint64 = arc.pose.joint_matrices(&arc.model, &evaluated);
    let packed_morphs = PackedMorphs::pack(&arc.model.meshes[body_index]);
    let (cpu_pos, _cpu_nrm) = torogao_render3d::reference::skin_mesh(
        &arc.model,
        body_index,
        &packed_morphs,
        &joint64,
        &weights,
    );
    let mut max_delta = 0.0_f64;
    for v in 0..cpu_pos.len() {
        let d = V3::from_f32(gpu_pos[v]).minus(V3::from_f32(cpu_pos[v]));
        max_delta = max_delta.max(d.x.abs().max(d.y.abs()).max(d.z.abs()));
    }
    println!("gpu vs cpu max position delta: {max_delta:.3e} m");
    assert!(max_delta <= 1e-4, "cross-check bound 1e-4 m");
}

// ---------------------------------------------------------
// Rung 2, M1: cast-generic proving tests. They need the
// staged per-girl inputs:
//   TOROGAO_RENDER3D_CAST_INPUTS=<dir holding <girl>/inputs/>
// Half validation, half adversarial.
// ---------------------------------------------------------

use torogao_render3d::RenderError;
use torogao_render3d::cast::{CastInputs, CastOptions, OutfitPlan, load_cast_inputs};

/// Per girl: slug, her show's outfit plan, Body vertex
/// count (the fleet census), and the replay hash of her
/// D-1223 clip of record.
const CAST: [(&str, &str, usize, &str); 5] = [
    (
        "alma",
        "0:1",
        13_931,
        "263d6ced318a6319e80dafb8df4ae809f7a1ae34c39a63609dec2fe1afb54bc7",
    ),
    (
        "lux",
        "0:1,90:2",
        13_904,
        "ce0d20ed5f66672dd830fa62835de73f9d290022ddaf8a92d51ee0128949d3aa",
    ),
    (
        "piper",
        "0:2",
        14_192,
        "add3a87df5404180fbbc881345947294c2032539a6637180fdea3abdcd154e22",
    ),
    (
        "sloane",
        "0:2",
        13_921,
        "df8cc71c5610fab7bf694693e1ab1caa3b319dc93d429e5123605b82e6e113a1",
    ),
    (
        "yui",
        "0:2",
        13_894,
        "e3ff8bbe9b163a5d22f01a16f4c1ed1977b37af7b831de8a88a9e62b107c9886",
    ),
];

fn cast_dir(slug: &str) -> PathBuf {
    let root = std::env::var("TOROGAO_RENDER3D_CAST_INPUTS")
        .expect("set TOROGAO_RENDER3D_CAST_INPUTS to the dir holding <girl>/inputs/");
    PathBuf::from(root).join(slug).join("inputs")
}

fn cast_options(slug: &str, plan: &str) -> CastOptions {
    CastOptions {
        slug: slug.to_string(),
        outfit_plan: OutfitPlan::parse(plan).expect("plan"),
        bust_weight: None,
    }
}

fn load_cast(slug: &str, plan: &str) -> CastInputs {
    load_cast_inputs(&cast_dir(slug), &cast_options(slug, plan)).expect("cast inputs load")
}

/// A scratch copy of one girl's inputs under another slug,
/// with an optional edit to her def bytes. Removed on drop.
struct ScratchInputs {
    dir: PathBuf,
}

impl ScratchInputs {
    fn copy_of(source_slug: &str, as_slug: &str, tag: &str, def_suffix: &[u8]) -> ScratchInputs {
        let dir = std::env::temp_dir().join(format!(
            "torogao-render3d-cast-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let source = cast_dir(source_slug);
        for (from, to) in [
            (format!("{source_slug}.glb"), format!("{as_slug}.glb")),
            (
                format!("{source_slug}.clip.json"),
                format!("{as_slug}.clip.json"),
            ),
            (
                format!("{source_slug}-channels.json"),
                format!("{as_slug}-channels.json"),
            ),
            (
                "plate-camera.json".to_string(),
                "plate-camera.json".to_string(),
            ),
        ] {
            std::fs::copy(source.join(from), dir.join(to)).expect("scratch copy");
        }
        let mut def =
            std::fs::read(source.join(format!("{source_slug}-skeleton.json"))).expect("def read");
        def.extend_from_slice(def_suffix);
        std::fs::write(dir.join(format!("{as_slug}-skeleton.json")), def).expect("def write");
        ScratchInputs { dir }
    }
}

impl Drop for ScratchInputs {
    fn drop(&mut self) {
        // Best-effort cleanup of this test's own scratch dir.
        if let Err(error) = std::fs::remove_dir_all(&self.dir) {
            eprintln!("scratch cleanup {}: {error}", self.dir.display());
        }
    }
}

/// Validation: the loader's census equals the fleet census
/// for every girl (54 joints, 59 nodes, Body 37 targets, one
/// bust target per outfit mesh, Body vertex count).
#[test]
#[ignore = "needs staged cast inputs"]
fn cast_census_matches_fleet() {
    for (slug, plan, body_vertices, _) in CAST {
        let cast = load_cast(slug, plan);
        let census = cast.census();
        assert_eq!(census.joints, 54, "{slug} joints");
        assert_eq!(census.nodes, 59, "{slug} nodes");
        let body = cast.model.mesh_named("_Body").expect("body");
        assert_eq!(body.vertex_count(), body_vertices, "{slug} body verts");
        assert_eq!(body.targets.len(), 37, "{slug} body targets");
        assert_eq!(cast.model.mesh_named("_Outfit").unwrap().targets.len(), 1);
        assert_eq!(
            cast.model.mesh_named("_OutfitEnd").unwrap().targets.len(),
            1
        );
        assert!(
            body.default_weights.iter().all(|w| *w == 0.0),
            "{slug} saved"
        );
    }
}

/// Validation: GLB joint-node rest TRS == def rest TRS for
/// every def bone of every girl.
#[test]
#[ignore = "needs staged cast inputs"]
fn cast_def_rest_matches_glb_nodes() {
    use torogao_motion_clip::SkeletonDef;
    for (slug, plan, _, _) in CAST {
        let cast = load_cast(slug, plan);
        assert_eq!(cast.skeleton.bones().len(), 54, "{slug} def bones");
        for (bone, glb_name) in cast.skeleton.bones().iter().zip(&cast.skeleton.glb_names) {
            let node = &cast.model.nodes[cast.model.node_named(glb_name).expect("node")];
            for i in 0..3 {
                assert!((node.translation[i] - bone.rest_translation[i]).abs() < 1e-9);
                assert!((node.scale[i] - bone.rest_scale[i]).abs() < 1e-9);
            }
            for i in 0..4 {
                assert!((node.rotation[i] - bone.rest_rotation[i]).abs() < 1e-9);
            }
        }
    }
}

/// Validation: each girl's clip of record replays to her
/// recorded hash.
#[test]
#[ignore = "needs staged cast inputs"]
fn cast_replay_hashes_match_record() {
    for (slug, plan, _, replay_hash) in CAST {
        let cast = load_cast(slug, plan);
        let bound = cast.bind().expect("bind");
        assert_eq!(cast.clip.frames, 900, "{slug} clip frames");
        assert_eq!(bound.replay_hash(), replay_hash, "{slug} replay hash");
    }
}

/// Validation (GPU): frame-0 twin hashes identical and the
/// GPU skin readback inside the cross-check bound, per girl.
#[test]
#[ignore = "needs staged cast inputs and a Vulkan device"]
fn cast_gpu_smoke_determinism_and_skin() {
    use torogao_render3d::backend::RenderBackend;
    use torogao_render3d::backend::vulkan::VulkanBackend;
    use torogao_render3d::compare::sha256_hex;
    use torogao_render3d::setscene::{PLATE_HEIGHT, PLATE_WIDTH};

    for (slug, plan, _, _) in CAST {
        let cast = load_cast(slug, plan);
        let bound = cast.bind().expect("bind");
        let mut backend = VulkanBackend::new().expect("backend");
        backend
            .upload_scene(&cast.packed_scene(), PLATE_WIDTH, PLATE_HEIGHT)
            .expect("upload");
        let draw = cast.draw_frame(&bound, 0).expect("draw");
        let a = backend.render_frame(&draw).expect("render a");
        let b = backend.render_frame(&draw).expect("render b");
        assert_eq!(sha256_hex(&a.rgba), sha256_hex(&b.rgba), "{slug} twins");

        let body_index = cast.body_index().expect("body");
        let weights = cast.frame_weights(&bound, 0).expect("weights")[body_index].clone();
        let evaluated = cast.pose.evaluate_plate_frame(&bound, 0).expect("eval");
        let joint64 = cast.pose.joint_matrices(&cast.model, &evaluated);
        let joint32: Vec<[f32; 16]> = joint64.iter().map(|m| m.to_f32_cols()).collect();
        let (gpu_pos, _) = backend
            .skin_positions(body_index, &joint32, &weights)
            .expect("skin readback");
        let packed = PackedMorphs::pack(&cast.model.meshes[body_index]);
        let (cpu_pos, _) = torogao_render3d::reference::skin_mesh(
            &cast.model,
            body_index,
            &packed,
            &joint64,
            &weights,
        );
        let mut max_delta = 0.0_f64;
        for v in 0..cpu_pos.len() {
            let d = V3::from_f32(gpu_pos[v]).minus(V3::from_f32(cpu_pos[v]));
            max_delta = max_delta.max(d.x.abs().max(d.y.abs()).max(d.z.abs()));
        }
        assert!(max_delta <= 1e-4, "{slug} cross-check bound 1e-4 m");
    }
}

/// Adversarial: a slug with no files in the directory is a
/// typed I/O failure naming the path, never a panic.
#[test]
#[ignore = "needs staged cast inputs"]
fn cast_missing_files_fail_typed() {
    let result = load_cast_inputs(&cast_dir("alma"), &cast_options("lux", "0:1"));
    match result {
        Err(RenderError::Io { path, .. }) => assert!(path.ends_with("lux.glb"), "{path}"),
        Err(other) => panic!("expected Io, got {other}"),
        Ok(_) => panic!("loaded a girl whose files are absent"),
    }
}

/// Adversarial: one girl's files renamed to another slug
/// are refused at load — the def token names the rig.
#[test]
#[ignore = "needs staged cast inputs"]
fn cast_slug_def_mismatch_is_refused() {
    let scratch = ScratchInputs::copy_of("alma", "lux", "slug", b"");
    let result = load_cast_inputs(&scratch.dir, &cast_options("lux", "0:1"));
    match result {
        Err(RenderError::Document(detail)) => assert!(detail.contains("lux-rig/1"), "{detail}"),
        Err(other) => panic!("expected Document, got {other}"),
        Ok(_) => panic!("accepted alma's rig under the slug lux"),
    }
}

/// Adversarial: a def whose bytes differ from the ones the
/// clip was exported against (same token, different
/// SHA-256) loads but refuses to bind.
#[test]
#[ignore = "needs staged cast inputs"]
fn cast_altered_def_refuses_to_bind() {
    let scratch = ScratchInputs::copy_of("alma", "alma", "sha", b" ");
    let cast = load_cast_inputs(&scratch.dir, &cast_options("alma", "0:1")).expect("loads");
    assert!(
        cast.bind().is_err(),
        "bound a clip to a def it was not made for"
    );
}

/// Adversarial: frames outside the arc are typed errors on
/// every per-frame entry point.
#[test]
#[ignore = "needs staged cast inputs"]
fn cast_frames_outside_the_arc_are_refused() {
    let cast = load_cast("alma", "0:1");
    let bound = cast.bind().expect("bind");
    for frame in [PLATE_FRAMES, PLATE_FRAMES + 1, u32::MAX] {
        assert!(matches!(
            cast.draw_frame(&bound, frame),
            Err(RenderError::FrameOutOfRange { .. })
        ));
        assert!(cast.frame_weights(&bound, frame).is_err());
    }
}
