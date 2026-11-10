// Morph blending + linear-blend skinning, shared verbatim
// by the character vertex shader and the cross-check
// compute shader, so the arithmetic the gate measures is
// the arithmetic that renders. Operates on the storage
// buffers declared by the including program.
//
// Equations (glTF 2.0): morphed(v) = base(v) + sum over the
// vertex's CSR entries of weight[target] * delta; skinned =
// sum_j w_j * (jointMatrix_j * morphed). Morph deltas live
// in a CSR packing (ranges/entry arrays) so cost follows
// the stored deltas, not vertices * targets (R2).

vec3 morphed_position(uint v) {
    vec3 p = in_positions[v].xyz;
    uint start = in_ranges[v].x;
    uint end = in_ranges[v].y;
    for (uint i = start; i < end; i++) {
        float w = in_morph_weights[in_entry_target[i]];
        p += w * in_entry_dpos[i].xyz;
    }
    return p;
}

vec3 morphed_normal(uint v) {
    vec3 n = in_normals[v].xyz;
    uint start = in_ranges[v].x;
    uint end = in_ranges[v].y;
    for (uint i = start; i < end; i++) {
        float w = in_morph_weights[in_entry_target[i]];
        n += w * in_entry_dnrm[i].xyz;
    }
    return n;
}

void skin_vertex(uint v, out vec3 pos, out vec3 nrm) {
    vec3 mp = morphed_position(v);
    vec3 mn = morphed_normal(v);
    uvec4 joints = in_joints[v];
    vec4 weights = in_weights[v];
    pos = vec3(0.0);
    nrm = vec3(0.0);
    for (int slot = 0; slot < 4; slot++) {
        float w = weights[slot];
        if (w == 0.0) {
            continue;
        }
        mat4 jm = in_joint_matrices[joints[slot]];
        pos += w * (jm * vec4(mp, 1.0)).xyz;
        nrm += w * (mat3(jm) * mn);
    }
    nrm = normalize(nrm);
}
