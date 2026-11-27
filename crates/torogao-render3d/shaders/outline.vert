#version 460
// Inverted-hull outline pass (Amendment A4): the skinned
// vertex is displaced along the smoothed outline normal
// (the second normal set) by the per-vertex width carried
// in COLOR_0 alpha, scaled by the scene's outline width.
// Paired with a front-face-culling raster state, only the
// hull's far side rasterizes, ringing the silhouette.
// The fragment side is a flat darkened paint colour.

layout(std140, set = 0, binding = 0) uniform Uniforms {
    mat4 view_proj;
    mat4 light_view_proj;
    vec4 sun;
    vec4 fill;
    vec4 world;
    vec4 camera_pos;
    mat4 view;
    vec4 toon0; // z: outline width (m)
    vec4 toon1;
    vec4 head_forward;
    vec4 head_right;
} u;

layout(std430, set = 0, binding = 1) readonly buffer JointMatrices {
    mat4 in_joint_matrices[];
};
layout(std430, set = 0, binding = 2) readonly buffer Positions {
    vec4 in_positions[];
};
layout(std430, set = 0, binding = 3) readonly buffer Normals {
    vec4 in_normals[];
};
layout(std430, set = 0, binding = 4) readonly buffer Color0 {
    vec4 in_color0[];
};
layout(std430, set = 0, binding = 6) readonly buffer Joints {
    uvec4 in_joints[];
};
layout(std430, set = 0, binding = 7) readonly buffer Weights {
    vec4 in_weights[];
};
layout(std430, set = 0, binding = 8) readonly buffer MorphRanges {
    uvec2 in_ranges[];
};
layout(std430, set = 0, binding = 9) readonly buffer MorphDpos {
    vec4 in_entry_dpos[];
};
layout(std430, set = 0, binding = 10) readonly buffer MorphDnrm {
    vec4 in_entry_dnrm[];
};
layout(std430, set = 0, binding = 11) readonly buffer MorphTarget {
    uint in_entry_target[];
};
layout(std430, set = 0, binding = 12) readonly buffer MorphWeights {
    float in_morph_weights[];
};
layout(std430, set = 0, binding = 16) readonly buffer OutlineData {
    vec4 in_outline[];
};

#include "skin.glsl"

layout(location = 0) out vec3 v_color;

void main() {
    uint v = uint(gl_VertexIndex);
    vec3 pos;
    vec3 nrm;
    skin_vertex(v, pos, nrm);
    vec4 od = in_outline[v];
    // Skin the outline normal by the same joint weights
    // (rotation only; morph deltas do not bend the hull
    // direction in v1).
    vec3 onrm = vec3(0.0);
    uvec4 joints = in_joints[v];
    vec4 w = in_weights[v];
    for (int i = 0; i < 4; i++) {
        onrm += w[i] * (mat3(in_joint_matrices[joints[i]]) * od.xyz);
    }
    float nl = length(onrm);
    onrm = nl > 1e-8 ? onrm / nl : nrm;
    vec3 displaced = pos + onrm * (od.w * u.toon0.z);
    v_color = in_color0[v].rgb * 0.22;
    gl_Position = u.view_proj * vec4(displaced, 1.0);
}
