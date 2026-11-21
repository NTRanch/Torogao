#version 460
// Set fragment shader: the same R5 shading model as the
// character path, plus unshadowed emission for the hypno
// screen and rings (EEVEE adds Principled emission
// post-lighting, pre-tonemap — matched here).

layout(std140, set = 0, binding = 0) uniform Uniforms {
    mat4 view_proj;
    mat4 light_view_proj;
    vec4 sun;
    vec4 fill;
    vec4 world;
    vec4 camera_pos;
} u;
layout(set = 0, binding = 1) uniform sampler2D shadow_map;

#include "shade.glsl"

layout(location = 0) in vec3 v_albedo;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in vec3 v_world_pos;
layout(location = 3) in vec4 v_emission;
layout(location = 4) in float v_roughness;

layout(location = 0) out vec4 out_colour;

float shadow_factor(vec3 world_pos, vec3 n) {
    vec3 p = world_pos + n * 0.005;
    vec4 lp = u.light_view_proj * vec4(p, 1.0);
    vec3 proj = lp.xyz / lp.w;
    vec2 uv = proj.xy * 0.5 + 0.5;
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || proj.z > 1.0) {
        return 1.0;
    }
    vec2 texel = 1.0 / vec2(textureSize(shadow_map, 0));
    float lit = 0.0;
    for (int x = -1; x <= 1; x++) {
        for (int y = -1; y <= 1; y++) {
            float depth = texture(shadow_map, uv + vec2(x, y) * texel).r;
            lit += (proj.z - 0.0008 <= depth) ? 1.0 : 0.0;
        }
    }
    return lit / 9.0;
}

void main() {
    vec3 N = normalize(v_normal);
    vec3 V = normalize(u.camera_pos.xyz - v_world_pos);
    float shadow = shadow_factor(v_world_pos, N);
    vec3 linear = shade_point(
        v_albedo, N, V, v_roughness,
        u.world.rgb,
        u.sun.xyz, u.sun.w, shadow,
        u.fill.xyz, u.fill.w
    );
    linear += v_emission.rgb * v_emission.a;
    out_colour = vec4(display_transform(linear), 1.0);
}
