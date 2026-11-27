//! The on-device preview: morph application, linear-blend
//! skinning, and a small f32 software rasteriser.
//!
//! Tier honesty: this is the estate's **Tier-2** numeric path —
//! f32 throughout, affine (not perspective-correct) attribute
//! interpolation, one wrap-lit directional light, vertex colours
//! only. It exists so a player can *see* the record they are
//! editing, on the phone, with no seat and no network. It is
//! not the Tier-1 render and the UI labels it as such.

use std::collections::BTreeMap;
use std::time::Instant;

use crate::glb::BaseMesh;
use crate::math::{
    Mat4, add, cross, dot, look_at, mul_s, norm, perspective, quat_from_euler_deg, quat_mul, sub,
};
use crate::record::CharacterRecord;

/// One primitive after deformation: `(positions, normals)` in
/// world space.
pub type DeformedPrim = (Vec<[f32; 3]>, Vec<[f32; 3]>);

/// A camera for the preview.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    /// Eye position, world space.
    pub eye: [f32; 3],
    /// Look-at target, world space.
    pub target: [f32; 3],
    /// Up vector.
    pub up: [f32; 3],
    /// Vertical field of view, degrees.
    pub fov_y_deg: f32,
}

/// A rendered preview frame (RGBA8, row-major).
#[derive(Debug, Clone)]
pub struct PreviewImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA pixels, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// Measured stage timings for one preview — the numbers the
/// trial reports instead of impressions.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreviewStats {
    /// Morph-application time, milliseconds.
    pub morph_ms: f32,
    /// Skinning time, milliseconds.
    pub skin_ms: f32,
    /// Rasterisation time, milliseconds.
    pub raster_ms: f32,
    /// Triangles submitted.
    pub triangles: u32,
    /// Vertices deformed.
    pub vertices: u32,
}

/// Preview failures: the record names something the bundled
/// base does not carry. Typed refusals, surfaced in the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// Expression/body morph names absent from the base.
    UnknownMorphs(Vec<String>),
    /// Pose joint names absent from the base's skin.
    UnknownJoints(Vec<String>),
    /// A pose override targets a matrix-form node, whose bind
    /// rotation this trial cannot decompose.
    MatrixJoint(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::UnknownMorphs(names) => {
                write!(
                    f,
                    "record names morphs the base lacks: {}",
                    names.join(", ")
                )
            }
            RenderError::UnknownJoints(names) => {
                write!(
                    f,
                    "record poses joints the base lacks: {}",
                    names.join(", ")
                )
            }
            RenderError::MatrixJoint(name) => {
                write!(
                    f,
                    "joint {name} uses a matrix-form node; pose override refused"
                )
            }
        }
    }
}

impl std::error::Error for RenderError {}

/// A pose override set: joint name -> Euler degrees
/// `[x, y, z]` in the joint's local space. Pose is preview
/// state, not record data — the schema keeps pose in clips,
/// not in the character record — so the shell holds it beside
/// the record and hands it to the preview.
pub type PoseMap = BTreeMap<String, [f32; 3]>;

/// Validate a record (and a preview pose) against the base:
/// every morph it weights and every joint posed must exist.
pub fn validate_record(
    mesh: &BaseMesh,
    record: &CharacterRecord,
    pose: &PoseMap,
) -> Result<(), RenderError> {
    let weights = record.morph_weights();
    let unknown: Vec<String> = weights
        .keys()
        .filter(|name| !mesh.morph_names.iter().any(|m| m == *name))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(RenderError::UnknownMorphs(unknown));
    }
    let unknown_j: Vec<String> = pose
        .keys()
        .filter(|name| mesh.joint_index(name).is_none())
        .cloned()
        .collect();
    if !unknown_j.is_empty() {
        return Err(RenderError::UnknownJoints(unknown_j));
    }
    Ok(())
}

/// World matrix per node with the pose overrides and the
/// record's z-scale applied.
fn node_worlds(
    mesh: &BaseMesh,
    record: &CharacterRecord,
    pose: &PoseMap,
) -> Result<Vec<Mat4>, RenderError> {
    let n = mesh.node_local.len();
    let mut worlds: Vec<Option<Mat4>> = vec![None; n];
    fn resolve(
        mesh: &BaseMesh,
        pose: &PoseMap,
        node: usize,
        worlds: &mut Vec<Option<Mat4>>,
        depth: usize,
    ) -> Result<Mat4, RenderError> {
        if let Some(w) = worlds[node] {
            return Ok(w);
        }
        if depth > 128 {
            return Err(RenderError::UnknownJoints(vec![
                "node hierarchy cycle".to_string(),
            ]));
        }
        let mut local = mesh.node_local[node];
        // Pose override: compose the record's Euler rotation
        // onto the joint's bind rotation, in local space.
        let joint = mesh.joint_of_node[node];
        if joint >= 0 {
            if let Some(euler) = pose.get(&mesh.joint_names[joint as usize]) {
                match mesh.node_trs[node] {
                    Some((t, r, s)) => {
                        let q = quat_mul(quat_from_euler_deg(*euler), r);
                        local = Mat4::from_trs(t, q, s);
                    }
                    None => {
                        return Err(RenderError::MatrixJoint(
                            mesh.joint_names[joint as usize].clone(),
                        ));
                    }
                }
            }
        }
        let parent = mesh.node_parents[node];
        let world = if parent < 0 {
            local
        } else {
            let pw = resolve(mesh, pose, parent as usize, worlds, depth + 1)?;
            pw.mul(&local)
        };
        worlds[node] = Some(world);
        Ok(world)
    }
    for i in 0..n {
        resolve(mesh, pose, i, &mut worlds, 0)?;
    }
    let scale = Mat4::scale(record.z_scale());
    Ok(worlds
        .into_iter()
        .map(|w| scale.mul(&w.expect("every node resolved")))
        .collect())
}

/// Skin matrix per joint: `world * inverseBind`.
fn skin_matrices(
    mesh: &BaseMesh,
    record: &CharacterRecord,
    pose: &PoseMap,
) -> Result<Vec<Mat4>, RenderError> {
    let worlds = node_worlds(mesh, record, pose)?;
    Ok(mesh
        .joint_nodes
        .iter()
        .enumerate()
        .map(|(j, &node)| worlds[node].mul(&mesh.inverse_bind[j]))
        .collect())
}

/// The world position of one joint under the record's pose
/// (used to frame the face camera on the head).
pub fn joint_world_position(
    mesh: &BaseMesh,
    record: &CharacterRecord,
    pose: &PoseMap,
    joint_name: &str,
) -> Result<Option<[f32; 3]>, RenderError> {
    let Some(j) = mesh.joint_index(joint_name) else {
        return Ok(None);
    };
    let worlds = node_worlds(mesh, record, pose)?;
    let w = worlds[mesh.joint_nodes[j]];
    Ok(Some([w.0[12], w.0[13], w.0[14]]))
}

/// Deform only (morph + skin), returning skinned positions and
/// normals per primitive. The host tests gate on this; the
/// rasteriser consumes it.
pub fn deform(
    mesh: &BaseMesh,
    record: &CharacterRecord,
    pose: &PoseMap,
) -> Result<(Vec<DeformedPrim>, PreviewStats), RenderError> {
    validate_record(mesh, record, pose)?;
    let weights = record.morph_weights();
    let t0 = Instant::now();
    // Stage 1: morphs.
    let mut staged: Vec<DeformedPrim> = Vec::with_capacity(mesh.prims.len());
    for prim in &mesh.prims {
        let mut pos = prim.positions.clone();
        let mut nrm = prim.normals.clone();
        for target in &prim.morphs {
            let Some(&w) = weights.get(&target.name) else {
                continue;
            };
            if w.abs() < 1e-4 {
                continue;
            }
            for (k, &vi) in target.pos_idx.iter().enumerate() {
                let d = target.pos_delta[k];
                let p = &mut pos[vi as usize];
                p[0] += w * d[0];
                p[1] += w * d[1];
                p[2] += w * d[2];
            }
            for (k, &vi) in target.nrm_idx.iter().enumerate() {
                let d = target.nrm_delta[k];
                let v = &mut nrm[vi as usize];
                v[0] += w * d[0];
                v[1] += w * d[1];
                v[2] += w * d[2];
            }
        }
        staged.push((pos, nrm));
    }
    let morph_ms = t0.elapsed().as_secs_f32() * 1000.0;
    // Stage 2: skinning.
    let t1 = Instant::now();
    let skins = skin_matrices(mesh, record, pose)?;
    let root_scale = Mat4::scale(record.z_scale());
    let mut stats = PreviewStats {
        morph_ms,
        vertices: mesh.vertex_count() as u32,
        triangles: mesh.triangle_count() as u32,
        ..Default::default()
    };
    for (pi, prim) in mesh.prims.iter().enumerate() {
        let (pos, nrm) = &mut staged[pi];
        if prim.joints.is_empty() {
            for p in pos.iter_mut() {
                *p = root_scale.point(*p);
            }
            continue;
        }
        for v in 0..pos.len() {
            let js = prim.joints[v];
            let ws = prim.weights[v];
            let mut acc_p = [0.0f32; 3];
            let mut acc_n = [0.0f32; 3];
            for k in 0..4 {
                let w = ws[k];
                if w == 0.0 {
                    continue;
                }
                let m = &skins[js[k] as usize];
                let tp = m.point(pos[v]);
                acc_p = add(acc_p, mul_s(tp, w));
                let tn = m.dir(nrm[v]);
                acc_n = add(acc_n, mul_s(tn, w));
            }
            pos[v] = acc_p;
            nrm[v] = norm(acc_n);
        }
    }
    stats.skin_ms = t1.elapsed().as_secs_f32() * 1000.0;
    Ok((staged, stats))
}

/// Render a preview frame of the record from `camera`.
pub fn render(
    mesh: &BaseMesh,
    record: &CharacterRecord,
    pose: &PoseMap,
    camera: &Camera,
    width: u32,
    height: u32,
) -> Result<(PreviewImage, PreviewStats), RenderError> {
    let (deformed, mut stats) = deform(mesh, record, pose)?;
    let t2 = Instant::now();
    let (w, h) = (width.max(1) as usize, height.max(1) as usize);
    let mut rgba = vec![0u8; w * h * 4];
    // Studio backdrop: deep plum, opaque.
    for px in rgba.chunks_exact_mut(4) {
        px[0] = 23;
        px[1] = 18;
        px[2] = 33;
        px[3] = 255;
    }
    let mut depth = vec![f32::INFINITY; w * h];
    let view = look_at(camera.eye, camera.target, camera.up);
    let proj = perspective(camera.fov_y_deg, width as f32 / height as f32, 0.05, 50.0);
    let mvp = proj.mul(&view);
    let light = norm([0.45, 0.8, 0.6]);
    for (pi, prim) in mesh.prims.iter().enumerate() {
        let (pos, nrm) = &deformed[pi];
        let clip: Vec<[f32; 4]> = pos.iter().map(|p| mvp.clip(*p)).collect();
        for tri in prim.indices.chunks_exact(3) {
            let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            let (ca, cb, cc) = (clip[a], clip[b], clip[c]);
            if ca[3] <= 1e-6 || cb[3] <= 1e-6 || cc[3] <= 1e-6 {
                continue;
            }
            let ndc = |v: [f32; 4]| [v[0] / v[3], v[1] / v[3], v[2] / v[3]];
            let (na, nb, nc) = (ndc(ca), ndc(cb), ndc(cc));
            // Backface cull in world space: estate exports are
            // consistently wound; a face pointing away from the
            // camera is not drawn.
            let face_n = cross(sub(pos[b], pos[a]), sub(pos[c], pos[a]));
            if dot(face_n, sub(camera.eye, pos[a])) <= 0.0 {
                continue;
            }
            let scr = |v: [f32; 3]| {
                [
                    (v[0] + 1.0) * 0.5 * w as f32,
                    (1.0 - (v[1] + 1.0) * 0.5) * h as f32,
                    v[2],
                ]
            };
            let (sa, sb, sc) = (scr(na), scr(nb), scr(nc));
            let min_x = sa[0].min(sb[0]).min(sc[0]).floor().max(0.0) as usize;
            let max_x = sa[0].max(sb[0]).max(sc[0]).ceil().min(w as f32 - 1.0);
            let min_y = sa[1].min(sb[1]).min(sc[1]).floor().max(0.0) as usize;
            let max_y = sa[1].max(sb[1]).max(sc[1]).ceil().min(h as f32 - 1.0);
            if max_x < 0.0 || max_y < 0.0 {
                continue;
            }
            let (max_x, max_y) = (max_x as usize, max_y as usize);
            let denom = (sb[1] - sc[1]) * (sa[0] - sc[0]) + (sc[0] - sb[0]) * (sa[1] - sc[1]);
            if denom.abs() < 1e-12 {
                continue;
            }
            let albedo = |v: usize| -> [f32; 3] {
                let base = prim
                    .colors
                    .as_ref()
                    .map(|cols| cols[v])
                    .unwrap_or([1.0, 1.0, 1.0, 1.0]);
                [
                    base[0] * prim.tint[0],
                    base[1] * prim.tint[1],
                    base[2] * prim.tint[2],
                ]
            };
            let (cola, colb, colc) = (albedo(a), albedo(b), albedo(c));
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let w0 =
                        ((sb[1] - sc[1]) * (px - sc[0]) + (sc[0] - sb[0]) * (py - sc[1])) / denom;
                    let w1 =
                        ((sc[1] - sa[1]) * (px - sc[0]) + (sa[0] - sc[0]) * (py - sc[1])) / denom;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = w0 * sa[2] + w1 * sb[2] + w2 * sc[2];
                    let di = y * w + x;
                    if z >= depth[di] {
                        continue;
                    }
                    depth[di] = z;
                    let n = norm([
                        w0 * nrm[a][0] + w1 * nrm[b][0] + w2 * nrm[c][0],
                        w0 * nrm[a][1] + w1 * nrm[b][1] + w2 * nrm[c][1],
                        w0 * nrm[a][2] + w1 * nrm[b][2] + w2 * nrm[c][2],
                    ]);
                    // Wrap lighting: soft, forgiving, reads as
                    // illustration rather than product render.
                    let diff = ((dot(n, light) + 0.35) / 1.35).clamp(0.0, 1.0);
                    let shade = 0.30 + 0.70 * diff;
                    let col = [
                        w0 * cola[0] + w1 * colb[0] + w2 * colc[0],
                        w0 * cola[1] + w1 * colb[1] + w2 * colc[1],
                        w0 * cola[2] + w1 * colb[2] + w2 * colc[2],
                    ];
                    let out = &mut rgba[di * 4..di * 4 + 4];
                    out[0] = (col[0] * shade * 255.0).clamp(0.0, 255.0) as u8;
                    out[1] = (col[1] * shade * 255.0).clamp(0.0, 255.0) as u8;
                    out[2] = (col[2] * shade * 255.0).clamp(0.0, 255.0) as u8;
                    out[3] = 255;
                }
            }
        }
    }
    stats.raster_ms = t2.elapsed().as_secs_f32() * 1000.0;
    Ok((
        PreviewImage {
            width,
            height,
            rgba,
        },
        stats,
    ))
}

/// The record's morph weight map type, re-exported for callers
/// that build param lists.
pub type WeightMap = BTreeMap<String, f32>;
