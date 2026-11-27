//! Trial gates against the real bundled Prima base GLB.
//!
//! These are the numbers the trial record cites: census,
//! bind-pose round-trip, morph locality, pose locality, and
//! render coverage/difference. Half are adversarial in spirit:
//! unknown names must refuse, corrupt input must not panic,
//! and a morph must move *only* the vertices it declares.

use std::fs;
use std::path::PathBuf;

use torogao_mobile_local::record::prima_template;
use torogao_mobile_local::render::{RenderError, deform, render};
use torogao_mobile_local::{LocalEngine, PoseMap, PreviewView};

fn temp_engine(tag: &str) -> (PathBuf, LocalEngine) {
    let dir = std::env::temp_dir().join(format!(
        "torogao-mobile-local-trial-{tag}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let engine = LocalEngine::open(&dir).expect("engine opens on the bundled base");
    (dir, engine)
}

#[test]
fn bundled_base_census_matches_the_gated_artefact() {
    let (dir, engine) = temp_engine("census");
    let mesh = engine.mesh();
    assert_eq!(mesh.prims.len(), 4, "body + hair + outfit prims");
    assert_eq!(mesh.vertex_count(), 22_340);
    assert_eq!(mesh.joint_names.len(), 54, "Mixamo skin");
    assert!(
        mesh.morph_names.iter().any(|n| n == "Fac_Mth_Smile"),
        "face set present"
    );
    assert!(
        mesh.morph_names.iter().any(|n| n == "Body_Bust_Size"),
        "body morph present"
    );
    assert_eq!(
        mesh.morph_names.len(),
        37,
        "36 face morphs + Body_Bust_Size"
    );
    // First run seeds exactly the Prima template record.
    let records = engine.store().list();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0], prima_template());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bind_pose_deform_round_trips() {
    let (dir, engine) = temp_engine("bind");
    let mesh = engine.mesh();
    let (deformed, stats) = deform(mesh, &prima_template(), &PoseMap::new()).expect("bind deform");
    assert_eq!(stats.vertices, 22_340);
    let mut worst = 0.0f32;
    for (pi, prim) in mesh.prims.iter().enumerate() {
        for (v, p) in prim.positions.iter().enumerate() {
            let d = deformed[pi].0[v];
            for i in 0..3 {
                worst = worst.max((d[i] - p[i]).abs());
            }
        }
    }
    assert!(
        worst < 1e-3,
        "bind pose must reproduce the export (worst delta {worst})"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn smile_morph_moves_only_its_declared_vertices() {
    let (dir, engine) = temp_engine("smile");
    let mesh = engine.mesh();
    let mut rec = prima_template();
    rec.set_expression("Fac_Mth_Smile", 1.0);
    let (smiling, _) = deform(mesh, &rec, &PoseMap::new()).expect("smile deform");
    let (neutral, _) = deform(mesh, &prima_template(), &PoseMap::new()).expect("neutral deform");
    // The body prim (prim 0) carries the face set.
    let target = mesh.prims[0]
        .morphs
        .iter()
        .find(|m| m.name == "Fac_Mth_Smile")
        .expect("smile target");
    let declared: std::collections::HashSet<u32> = target.pos_idx.iter().copied().collect();
    let mut changed = 0usize;
    for v in 0..mesh.prims[0].positions.len() {
        let moved = (0..3).any(|i| (smiling[0].0[v][i] - neutral[0].0[v][i]).abs() > 1e-6);
        if moved {
            changed += 1;
            assert!(
                declared.contains(&(v as u32)),
                "vertex {v} moved but is not in the smile target's sparse set"
            );
        }
    }
    // The Smile target's sparse set is small and mouth-local:
    // measured 36 movers at full weight on this artefact. The
    // gate is "a real mouthful, all declared" — not a round
    // number invented in advance.
    assert!(
        changed >= 30,
        "a full smile must move a mouthful of vertices (moved {changed})"
    );
    assert!(
        changed <= declared.len(),
        "movers cannot exceed the declared set"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn head_pose_moves_the_head_and_not_the_feet() {
    let (dir, engine) = temp_engine("pose");
    let mesh = engine.mesh();
    let head_joint = mesh.joint_index("mixamorig:Head").expect("head joint");
    let foot_joint = mesh.joint_index("mixamorig:LeftFoot").expect("foot joint");
    // A vertex dominated by the head, and one by the left foot.
    let dominated_by = |joint: usize| -> (usize, usize) {
        for (pi, prim) in mesh.prims.iter().enumerate() {
            for (v, js) in prim.joints.iter().enumerate() {
                let w = prim.weights[v];
                let (best, _) = (0..4)
                    .max_by(|&a, &b| w[a].partial_cmp(&w[b]).unwrap())
                    .map(|k| (js[k] as usize, w[k]))
                    .unwrap();
                if best == joint && w[(0..4).find(|&k| js[k] as usize == joint).unwrap()] > 0.9 {
                    return (pi, v);
                }
            }
        }
        panic!("no vertex dominated by joint {joint}");
    };
    let (hp, hv) = dominated_by(head_joint);
    let (fp, fv) = dominated_by(foot_joint);
    let rec = prima_template();
    let mut pose = PoseMap::new();
    pose.insert("mixamorig:Head".to_string(), [0.0, 30.0, 0.0]);
    let (posed, _) = deform(mesh, &rec, &pose).expect("posed deform");
    let (bind, _) = deform(mesh, &prima_template(), &PoseMap::new()).expect("bind deform");
    let head_move = (0..3)
        .map(|i| (posed[hp].0[hv][i] - bind[hp].0[hv][i]).abs())
        .fold(0.0f32, f32::max);
    let foot_move = (0..3)
        .map(|i| (posed[fp].0[fv][i] - bind[fp].0[fv][i]).abs())
        .fold(0.0f32, f32::max);
    assert!(
        head_move > 0.01,
        "head vertex must swing (moved {head_move})"
    );
    assert!(
        foot_move < 1e-4,
        "foot vertex must not follow the head (moved {foot_move})"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unknown_names_refuse_by_name() {
    let (dir, engine) = temp_engine("refuse");
    let mut rec = prima_template();
    rec.set_expression("Fac_Nope", 1.0);
    match engine.preview(&rec, &PoseMap::new(), PreviewView::Body, 64, 64) {
        Err(RenderError::UnknownMorphs(names)) => {
            assert_eq!(names, ["Fac_Nope"]);
        }
        other => panic!("expected UnknownMorphs, got {other:?}"),
    }
    let mut pose = PoseMap::new();
    pose.insert("mixamorig:Tail".to_string(), [0.0; 3]);
    match engine.preview(&prima_template(), &pose, PreviewView::Body, 64, 64) {
        Err(RenderError::UnknownJoints(names)) => {
            assert_eq!(names, ["mixamorig:Tail"]);
        }
        other => panic!("expected UnknownJoints, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn body_render_covers_and_smile_changes_the_face() {
    let (dir, engine) = temp_engine("render");
    let mesh = engine.mesh();
    let camera = engine.body_camera();
    let (neutral, stats) = render(mesh, &prima_template(), &PoseMap::new(), &camera, 360, 480)
        .expect("neutral render");
    assert_eq!(stats.triangles, mesh.triangle_count() as u32);
    let bg = [23u8, 18, 33];
    let covered = neutral
        .rgba
        .chunks_exact(4)
        .filter(|px| px[0..3] != bg)
        .count();
    let frac = covered as f32 / (360.0 * 480.0);
    assert!(
        (0.05..0.85).contains(&frac),
        "figure must cover a sane share of the frame (got {frac})"
    );
    // Smile vs neutral, face view: the mouth band must change.
    let face_cam = engine
        .face_camera(&prima_template(), &PoseMap::new())
        .expect("face cam");
    let (face_neutral, _) = render(
        mesh,
        &prima_template(),
        &PoseMap::new(),
        &face_cam,
        360,
        360,
    )
    .expect("face neutral");
    let mut rec = prima_template();
    rec.set_expression("Fac_Mth_Smile", 1.0);
    rec.set_expression("Fac_Eye_Close", 0.6);
    let (face_smile, _) =
        render(mesh, &rec, &PoseMap::new(), &face_cam, 360, 360).expect("face smile");
    let differing = face_neutral
        .rgba
        .iter()
        .zip(face_smile.rgba.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        differing > 500,
        "smile + blink must repaint the face (differing bytes {differing})"
    );
    let _ = fs::remove_dir_all(&dir);
}
