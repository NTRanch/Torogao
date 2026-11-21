#version 460
// Shadow-depth vertex shader for set pieces.

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
    vec4 albedo_rough;
    vec4 emission;
};

layout(std430, set = 0, binding = 2) readonly buffer Pieces {
    PieceDraw pieces[];
};

layout(push_constant) uniform SetPush {
    uint piece;
} push;

layout(location = 0) in vec3 in_position;
layout(location = 1) in vec3 in_normal;

void main() {
    gl_Position = u.light_view_proj * pieces[push.piece].model * vec4(in_position, 1.0);
}
