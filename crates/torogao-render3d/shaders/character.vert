#version 460
// Character vertex shader: vertex pulling over storage
// buffers, morph + skin in the shared chunk. One draw per
// mesh; gl_VertexIndex is the post-index vertex id.

layout(std140, set = 0, binding = 0) uniform Uniforms {
    mat4 view_proj;
    mat4 light_view_proj;
    vec4 sun;        // xyz: direction to light, w: energy
    vec4 fill;       // xyz: direction to light, w: energy
    vec4 world;      // xyz: ambient colour
    vec4 camera_pos; // xyz
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
layout(std430, set = 0, binding = 5) readonly buffer Color1 {
    vec4 in_color1[];
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
layout(std430, set = 0, binding = 15) readonly buffer Uvs {
    vec4 in_uv[];
};

layout(push_constant) uniform Push {
    vec4 base_color;
    float roughness;
    float metallic;
    // Alpha-overlay pass flag (1 = this mesh's material
    // is blend class); consumed by the fragment stage,
    // declared here so both stages share one block.
    float overlay;
    float _push_pad;
} push;

#include "skin.glsl"

layout(location = 0) out vec3 v_albedo;
layout(location = 1) out vec3 v_normal;
layout(location = 2) out vec3 v_world_pos;
layout(location = 3) out vec4 v_color1;
layout(location = 4) out vec4 v_uvs;

void main() {
    uint v = uint(gl_VertexIndex);
    vec3 pos;
    vec3 nrm;
    skin_vertex(v, pos, nrm);
    // xy: TEXCOORD_0 (face SDF space); zw: TEXCOORD_1
    // (packed-lightmap space). Consumed by the toon
    // model only; the legacy path ignores them.
    v_uvs = in_uv[v];
    // Binding 4 carries the mesh's PAINT set: COLOR_1
    // when the mesh has one, else COLOR_0 (the backend
    // packs it so; measured on Prima, COLOR_0 is constant
    // white and COLOR_1 is the paint). push.base_color is
    // the material factor, or white when the paint came
    // from COLOR_1 (the factor is baked into that paint).
    v_albedo = in_color0[v].rgb * push.base_color.rgb;
    v_normal = nrm;
    v_world_pos = pos;
    // The non-paint colour set, carried for evidence and
    // future shading terms; not consumed in rung 1.
    v_color1 = in_color1[v];
    gl_Position = u.view_proj * vec4(pos, 1.0);
}
