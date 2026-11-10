//! The CPU reference for morph blending + linear-blend
//! skinning, written directly from the glTF 2.0
//! specification's equations, in f64, over the same packed
//! data the GPU consumes. This is the arithmetic witness
//! for the GPU skinning path (comparison (b)): the bound is
//! stated in the cross-check tool before it runs, and this
//! module's job is to be an independent, obviously-correct
//! implementation — no shared code with the shader beyond
//! the input bytes.
//!
//! Equations (glTF 2.0, "Skins" and "Morph Targets"):
//!   morphed(v)  = base(v) + sum_t weight_t * delta_t(v)
//!   skinned(v)  = sum_j w_j * (jointMatrix_j * morphed(v))
//!   normal(v)   = normalize(sum_j w_j * (mat3(jointMatrix_j)
//!                              * morphedNormal(v)))
//! Weights are used as authored (the GLB's weights sum to
//! 1 per vertex; nothing is renormalized).

use crate::math::{Mat4, V3};
use crate::model::Model;
use crate::morph::PackedMorphs;

/// Skin one vertex of one mesh. Returns `(position, normal)`.
pub fn skin_vertex(
    model: &Model,
    mesh_index: usize,
    packed: &PackedMorphs,
    joint_matrices: &[Mat4],
    weights: &[f32],
    vertex: usize,
) -> (V3, V3) {
    let mesh = &model.meshes[mesh_index];
    let mut pos = V3::from_f32(mesh.positions[vertex]);
    let mut nrm = V3::from_f32(mesh.normals[vertex]);
    let range = packed.ranges[vertex];
    for entry in range[0]..range[1] {
        let entry = entry as usize;
        let w = f64::from(weights[packed.entry_target[entry] as usize]);
        if w != 0.0 {
            let dp = packed.entry_dpos[entry];
            let dn = packed.entry_dnrm[entry];
            pos = pos.plus(V3::new(f64::from(dp[0]), f64::from(dp[1]), f64::from(dp[2])).scale(w));
            nrm = nrm.plus(V3::new(f64::from(dn[0]), f64::from(dn[1]), f64::from(dn[2])).scale(w));
        }
    }
    let joints = mesh.joints[vertex];
    let skin_weights = mesh.weights[vertex];
    let mut out_pos = V3::ZERO;
    let mut out_nrm = V3::ZERO;
    for slot in 0..4 {
        let w = f64::from(skin_weights[slot]);
        if w == 0.0 {
            continue;
        }
        let joint_matrix = joint_matrices[joints[slot] as usize];
        out_pos = out_pos.plus(joint_matrix.transform_point(pos).scale(w));
        out_nrm = out_nrm.plus(joint_matrix.transform_vector(nrm).scale(w));
    }
    (out_pos, out_nrm.normalized())
}

/// Skin every vertex of one mesh: `(positions, normals)`.
pub fn skin_mesh(
    model: &Model,
    mesh_index: usize,
    packed: &PackedMorphs,
    joint_matrices: &[Mat4],
    weights: &[f32],
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let count = model.meshes[mesh_index].vertex_count();
    let mut positions = Vec::with_capacity(count);
    let mut normals = Vec::with_capacity(count);
    for vertex in 0..count {
        let (pos, nrm) = skin_vertex(model, mesh_index, packed, joint_matrices, weights, vertex);
        positions.push(pos.to_f32());
        normals.push(nrm.to_f32());
    }
    (positions, normals)
}
