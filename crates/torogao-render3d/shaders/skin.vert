#version 460
// Shadow-depth vertex shader for skinned meshes: the same
// morph+skin path as the colour pass, projected by the
// light's view-projection. No fragment outputs (paired
// with depthonly.frag).

layout(std140, set = 0, binding = 0) uniform Uniforms {
    mat4 view_proj;
    mat4 light_view_proj;
    vec4 sun;
    vec4 fill;
    vec4 world;
    vec4 camera_pos;
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

#include "skin.glsl"

void main() {
    vec3 pos;
    vec3 nrm;
    skin_vertex(uint(gl_VertexIndex), pos, nrm);
    gl_Position = u.light_view_proj * vec4(pos, 1.0);
}
