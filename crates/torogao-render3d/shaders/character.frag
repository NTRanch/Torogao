#version 460
// Character fragment shader: two shading models behind
// one pipeline. The legacy model (half-Lambert wrap +
// single GGX lobe, R5) renders when the scene carries no
// toon data; the toon model (gap G1, Amendment A3) renders
// when it does. Both share the sun shadow map (3x3 PCF,
// manual compare — hardware comparison samplers vary
// across vendors and this frame must be deterministic on
// a fixed adapter) and the display transform.

layout(std140, set = 0, binding = 0) uniform Uniforms {
    mat4 view_proj;
    mat4 light_view_proj;
    vec4 sun;
    vec4 fill;
    vec4 world;
    vec4 camera_pos;
    // Toon extension (gap G1). Shaders that predate the
    // extension (set, depth) declare only the prefix
    // above; the buffer always carries the full block.
    mat4 view;
    vec4 toon0; // x: toon enabled, y: warm_cool,
                // z: outline width (m), w: rim strength
    vec4 toon1; // x: head joint index (-1 = face branch
                // off), y: SDF softness, zw: reserved
    vec4 head_forward; // xyz: head-local forward axis
    vec4 head_right;   // xyz: head-local right axis
} u;
layout(set = 0, binding = 13) uniform sampler2D shadow_map;
// The joint matrices ride along in the fragment stage:
// the face branch needs the head joint's world rotation.
layout(std430, set = 0, binding = 1) readonly buffer JointMatrices {
    mat4 in_joint_matrices[];
};
layout(set = 0, binding = 14) uniform sampler2D toon_packed;
layout(set = 0, binding = 17) uniform sampler2D toon_ramp_tex;
layout(set = 0, binding = 18) uniform sampler2D toon_face;
layout(set = 0, binding = 19) uniform sampler2D toon_sphere;
// The packed texture again through a NEAREST sampler:
// the material ID channel must never blend.
layout(set = 0, binding = 20) uniform sampler2D toon_packed_id;

layout(push_constant) uniform Push {
    vec4 base_color;
    float roughness;
    float metallic;
} push;

#include "shade.glsl"
#include "toon.glsl"

layout(location = 0) in vec3 v_albedo;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in vec3 v_world_pos;
layout(location = 3) in vec4 v_color1;
layout(location = 4) in vec4 v_uvs;

layout(location = 0) out vec4 out_colour;

float shadow_factor(vec3 world_pos, vec3 n) {
    // Normal-offset bias (0.5 cm) plus a small constant
    // depth bias; both are rung-1 tuned constants.
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

// The toon model (toon.glsl carries the shared math).
// v1 scope, declared: the SDF governs the SUN term only;
// the fill still ramps by its own NdotL on faces.
vec3 shade_toon(vec3 albedo, vec3 n, vec3 v, float shadow) {
    vec4 packed_linear = texture(toon_packed, v_uvs.zw);
    float id_alpha = texture(toon_packed_id, v_uvs.zw).a;
    ToonSample ts = toon_decode(packed_linear, id_alpha);
    float ndl_sun = dot(n, u.sun.xyz);
    float ramp_u = ndl_sun * 0.5 + 0.5;
    if (ts.id == 1) {
        // Face: painted SDF by head-relative light angle.
        // The light's horizontal direction against the
        // head basis sets a sweep threshold; the side of
        // the light mirrors the SDF sample.
        vec3 hf = u.head_forward.xyz;
        vec3 hr = u.head_right.xyz;
        if (u.toon1.x >= 0.0) {
            mat4 jm = in_joint_matrices[int(u.toon1.x + 0.5)];
            hf = normalize((jm * vec4(hf, 0.0)).xyz);
            hr = normalize((jm * vec4(hr, 0.0)).xyz);
        }
        vec2 fuv = v_uvs.xy;
        if (dot(hr.xz, u.sun.xyz.xz) <= 0.0) {
            fuv.x = 1.0 - fuv.x;
        }
        vec4 sdf = texture(toon_face, fuv);
        float fdotl = dot(hf.xz, u.sun.xyz.xz);
        float sweep = 1.0 - (fdotl * 0.5 + 0.5);
        ramp_u = toon_face_lit(sdf.r, sweep, u.toon1.y);
        albedo = mix(albedo, TOON_BLUSH_RGB, sdf.g * 0.55);
        albedo *= 1.0 - sdf.b * 0.15;
        albedo = mix(albedo, TOON_LIP_RGB, sdf.a * 0.6);
    }
    vec3 ramp_sun = toon_ramp(toon_ramp_tex, ramp_u, ts.id, u.toon0.y);
    float fill_u = dot(n, u.fill.xyz) * 0.5 + 0.5;
    vec3 ramp_fill = toon_ramp(toon_ramp_tex, fill_u, ts.id, u.toon0.y);
    vec3 col = albedo * u.world.rgb * ts.ao;
    col += albedo * ramp_sun * (u.sun.w * shadow * ts.ao);
    col += albedo * ramp_fill * (u.fill.w * ts.ao);
    col += toon_spec(n, v, u.sun.xyz, ts.spec, ts.metal, albedo)
        * (u.sun.w * shadow);
    col += toon_spec(n, v, u.fill.xyz, ts.spec, ts.metal, albedo) * u.fill.w;
    // Rim, gated by the terminator so it lives at the
    // lit side's edge.
    float fres = pow(1.0 - max(dot(n, v), 0.0), 3.0);
    col += vec3(fres * (ndl_sun * 0.5 + 0.5) * u.toon0.w * u.sun.w
        * shadow * ts.ao);
    if (ts.id == 2) {
        // Hair: sphere-map sheen by view-space normal.
        vec3 nv = normalize((u.view * vec4(n, 0.0)).xyz);
        vec3 sheen = texture(toon_sphere, nv.xy * 0.5 + 0.5).rgb;
        col += albedo * sheen
            * (ts.spec * (ndl_sun * 0.5 + 0.5) * TOON_SHEEN_SCALE
                * u.sun.w * shadow);
    }
    return col;
}

void main() {
    vec3 N = normalize(v_normal);
    vec3 V = normalize(u.camera_pos.xyz - v_world_pos);
    float shadow = shadow_factor(v_world_pos, N);
    vec3 linear;
    if (u.toon0.x > 0.5) {
        linear = shade_toon(v_albedo, N, V, shadow);
    } else {
        linear = shade_point(
            v_albedo, N, V, push.roughness,
            u.world.rgb,
            u.sun.xyz, u.sun.w, shadow,
            u.fill.xyz, u.fill.w
        );
    }
    out_colour = vec4(display_transform(linear), 1.0);
}
