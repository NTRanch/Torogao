#version 460
// Set vertex shader: classic vertex input, per-piece draw
// data (model matrix, normal matrix, material) indexed by
// a push-constant piece id into a shared SSBO.

layout(std140, set = 0, binding = 0) uniform Uniforms {
    mat4 view_proj;
    mat4 light_view_proj;
    vec4 sun;
    vec4 fill;
    vec4 world;
    vec4 camera_pos;
} u;

struct PieceDraw {
    mat4 model;
    vec4 normal_col0;
    vec4 normal_col1;
    vec4 normal_col2;
    vec4 albedo_rough;   // xyz albedo, w roughness
    vec4 emission;       // xyz colour, w strength
};

layout(std430, set = 0, binding = 2) readonly buffer Pieces {
    PieceDraw pieces[];
};

layout(push_constant) uniform SetPush {
    uint piece;
} push;

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;

layout(location = 0) out vec3 v_albedo;
layout(location = 1) out vec3 v_normal;
layout(location = 2) out vec3 v_world_pos;
layout(location = 3) out vec4 v_emission;
layout(location = 4) out float v_roughness;

void main() {
    PieceDraw piece = pieces[push.piece];
    vec4 world_pos = piece.model * vec4(in_position, 1.0);
    mat3 normal_mat = mat3(piece.normal_col0.xyz, piece.normal_col1.xyz, piece.normal_col2.xyz);
    v_albedo = piece.albedo_rough.rgb;
    v_roughness = piece.albedo_rough.a;
    v_emission = piece.emission;
    v_normal = normal_mat * in_normal;
    v_world_pos = world_pos.xyz;
    gl_Position = u.view_proj * world_pos;
}
