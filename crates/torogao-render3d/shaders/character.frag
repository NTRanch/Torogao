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
                // off), y: SDF softness, z: Family B
                // albedo-texture flag, w: reserved
    vec4 head_forward; // xyz: head-local forward axis
    vec4 head_right;   // xyz: head-local right axis
    // Gap-fix extension (2026-11-26). Appended after the
    // G1 fields; prefix-declaring shaders stay legal.
    vec4 toon2; // xyz: shadow tint colour (GAP-6),
                // w: shadow tint factor
    vec4 toon3; // x: exposure, y: world saturation,
                // z: world value, w: value enhance (GAP-7)
    vec4 toon4; // xyz: blush colour, w: blush strength
                // (GAP-9)
    vec4 toon5; // xyz: lip colour, w: lip strength
                // (GAP-9)
    vec4 toon6; // x: nose strength (GAP-9), y: band
                // frequency, z: band mix, w: band offset
                // (GAP-4)
    vec4 toon7; // x: band window lo, y: band window hi,
                // z: band view dependence (GAP-4),
                // w: reserved
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
// Family B: the scene's shared base-colour texture (the
// character's atlas), sampled in TEXCOORD_0 space. Bound
// always; sampled only when toon1.z flags it present.
layout(set = 0, binding = 21) uniform sampler2D albedo_tex;
// The per-ID parameter table (GAP-1/2/3/5 + hair
// highlight colour), fetched by texelFetch in toon.glsl.
layout(set = 0, binding = 22) uniform sampler2D toon_params_tex;

layout(push_constant) uniform Push {
    vec4 base_color;
    float roughness;
    float metallic;
    // Alpha-overlay pass flag (1 = blend class): the
    // fragment's alpha leaves with the framebuffer blend
    // instead of the opaque 1.0.
    float overlay;
    float _push_pad;
} push;

#include "shade.glsl"
#include "toon.glsl"

layout(location = 0) in vec3 v_albedo;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in vec3 v_world_pos;
layout(location = 3) in vec4 v_color1;
layout(location = 4) in vec4 v_uvs;
// GAP-4/GAP-8 channels: xy = planar UV (TEXCOORD_2),
// z = planar present flag.
layout(location = 5) in vec3 v_planar;
// xyz = skinned shading (toon) normal, w = present flag.
layout(location = 6) in vec4 v_toon_normal;

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
// `n_geom` is the mesh normal (shadow bias, sheen view
// transform); `n` is the shading normal — the authored
// toon normal when the mesh carries one (GAP-8).
vec3 shade_toon(vec3 albedo, vec3 n_geom, vec3 n, vec3 v, float shadow,
                bool front_facing) {
    vec4 packed_linear = texture(toon_packed, v_uvs.zw);
    float id_alpha = texture(toon_packed_id, v_uvs.zw).a;
    ToonSample ts = toon_decode(packed_linear, id_alpha);
    ToonParams prm = toon_params(toon_params_tex, ts.id);
    if (!front_facing) {
        // Back faces take the per-ID back colour as
        // their albedo; every ramp/spec/rim term below
        // then applies over it (GAP-5).
        albedo = prm.back_color;
    }
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
        // Face tint set: ToonData settings since the
        // gap-fix round (GAP-9), values unchanged.
        albedo = mix(albedo, u.toon4.xyz, sdf.g * u.toon4.w);
        albedo *= 1.0 - sdf.b * u.toon6.x;
        albedo = mix(albedo, u.toon5.xyz, sdf.a * u.toon5.w);
    }
    vec3 ramp_sun = toon_ramp(toon_ramp_tex, ramp_u, ts.id, u.toon0.y);
    float fill_u = dot(n, u.fill.xyz) * 0.5 + 0.5;
    vec3 ramp_fill = toon_ramp(toon_ramp_tex, fill_u, ts.id, u.toon0.y);
    // The calibrated world colour (GAP-7): every
    // material consumes the same rebalanced ambient.
    vec3 world_c = toon_world_calibrate(u.world.rgb, u.toon3.y, u.toon3.z,
                                         u.toon3.w);
    vec3 col = albedo * world_c * ts.ao;
    col += albedo * ramp_sun * (u.sun.w * shadow * ts.ao);
    col += albedo * ramp_fill * (u.fill.w * ts.ao);
    // SSS transition band (GAP-1): the per-ID tint in
    // the ramp's transition zone, sun term only.
    col += prm.sss_color
        * (toon_sss_zone(ramp_u, prm.sss_width) * u.sun.w * shadow * ts.ao);
    // Shadow tint injection (GAP-6): a scene-level tint
    // in the shaded band the sun ramp lookup implies,
    // applied after the sun/fill sum.
    float shaded_mask = 1.0 - smoothstep(0.35, 0.65, ramp_u);
    col += u.toon2.xyz * (shaded_mask * u.toon2.w * ts.ao);
    col += toon_spec(n, v, u.sun.xyz, ts.spec, ts.metal, albedo,
                     prm.spec_exponent, prm.spec_tint)
        * (u.sun.w * shadow);
    col += toon_spec(n, v, u.fill.xyz, ts.spec, ts.metal, albedo,
                     prm.spec_exponent, prm.spec_tint)
        * u.fill.w;
    // Rim (GAP-2): per-ID colour/size/smooth from the
    // parameter table; toon0.w stays the master strength.
    // Still gated by the terminator so it lives at the
    // lit side's edge.
    float rim = toon_rim_band(dot(n, v), prm.rim_size, prm.rim_smooth);
    col += prm.rim_color
        * (rim * (ndl_sun * 0.5 + 0.5) * u.toon0.w * u.sun.w
            * shadow * ts.ao);
    if (ts.id == 2) {
        // Hair: sphere-map sheen by view-space normal
        // (geometric normal — the sheen is view-locked
        // by construction).
        vec3 nv = normalize((u.view * vec4(n_geom, 0.0)).xyz);
        vec3 sheen = texture(toon_sphere, nv.xy * 0.5 + 0.5).rgb;
        col += albedo * sheen
            * (ts.spec * (ndl_sun * 0.5 + 0.5) * TOON_SHEEN_SCALE
                * u.sun.w * shadow);
        // Hair planar bands (GAP-4): the second,
        // authored-placement highlight, driven by the
        // planar along-length coordinate when the mesh
        // carries TEXCOORD_2. View dependence fades the
        // bands toward the camera-facing read by
        // toon7.z.
        if (v_planar.z > 0.5) {
            float view_term = mix(1.0, pow(max(dot(n, v), 0.0), 1.5),
                                  u.toon7.z);
            float bands = toon_hair_bands(v_planar.y, u.toon6.y, u.toon6.z,
                                          u.toon6.w, u.toon7.x, u.toon7.y);
            col += prm.highlight_color
                * (bands * prm.highlight_strength * ts.spec * view_term
                    * u.sun.w * shadow);
        }
    }
    return col;
}

void main() {
    vec3 N = normalize(v_normal);
    // The shading normal (GAP-8): the authored toon
    // normal when present, else the mesh normal.
    vec3 Ns = (v_toon_normal.w > 0.5) ? normalize(v_toon_normal.xyz) : N;
    vec3 V = normalize(u.camera_pos.xyz - v_world_pos);
    float shadow = shadow_factor(v_world_pos, N);
    // Family B: the base-colour texture multiplies the
    // vertex-paint albedo (the glTF base-colour rule:
    // factor x texture x vertex colour), on both shading
    // paths — the texture is a model property, not a toon
    // feature. The texel's alpha rides along: it is the
    // alpha-overlay route's coverage (the EyeShadowFilm
    // samples the face atlas, whose alpha channel is the
    // film's authored alpha).
    vec4 albedo_texel = vec4(1.0);
    if (u.toon1.z > 0.5) {
        albedo_texel = texture(albedo_tex, v_uvs.xy);
    }
    vec3 albedo = v_albedo * albedo_texel.rgb;
    vec3 linear;
    if (u.toon0.x > 0.5) {
        linear = shade_toon(albedo, N, Ns, V, shadow, gl_FrontFacing);
        // Display calibration (GAP-7): the exposure gain
        // of the global configuration block, applied to
        // the toon path before the shared display
        // transform. Identity at 1.0.
        linear *= u.toon3.x;
    } else {
        linear = shade_point(
            albedo, N, V, push.roughness,
            u.world.rgb,
            u.sun.xyz, u.sun.w, shadow,
            u.fill.xyz, u.fill.w
        );
    }
    // Opaque fragments write alpha 1. Overlay fragments
    // (drawn by the blend pipeline, after the opaque
    // pass, no depth write) write their coverage: atlas
    // alpha x material factor alpha. The factor alpha
    // rides push.base_color.a, which is 1.0 when the
    // paint came from COLOR_1 (the factor is baked into
    // that paint; its alpha is not carried — no estate
    // overlay material needs it).
    float alpha = push.overlay > 0.5
        ? clamp(albedo_texel.a * push.base_color.a, 0.0, 1.0)
        : 1.0;
    out_colour = vec4(display_transform(linear), alpha);
}
