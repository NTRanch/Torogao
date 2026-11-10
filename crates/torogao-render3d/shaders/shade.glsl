// The rung-1 shading model (gap analysis R5): albedo
// under the fixed two-light rig with half-Lambert wrapped
// diffuse, a single GGX specular lobe (dielectric F0 =
// 0.04, metallic 0), the world colour as ambient, and an
// AgX-approximating tonemap followed by the sRGB encode.
// This is NOT Cycles/EEVEE parity; it is the named target
// approximation, tuned against the plates of record.
//
// The AgX approximation is the widely used polynomial fit
// by Krzysztof Narkowicz (the form Filament ships), with
// the standard inset/outset matrices. The Rust mirror of
// this curve lives in setscene.rs (used for the
// background clear colour) — keep the two in step.

const float PI = 3.141592653589793;

float wrap_diffuse(float ndl) {
    // Valve half-Lambert: remaps the terminator so the
    // dark side wraps halfway around.
    return ndl * 0.5 + 0.5;
}

float distribution_ggx(float ndh, float roughness) {
    float a = roughness * roughness;
    float a2 = a * a;
    float d = ndh * ndh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

float geometry_schlick(float ndx, float roughness) {
    float k = roughness * roughness * 0.5;
    return ndx / (ndx * (1.0 - k) + k);
}

vec3 fresnel_schlick(float cos_theta, vec3 f0) {
    return f0 + (1.0 - f0) * pow(1.0 - cos_theta, 5.0);
}

// Specular contribution of one directional light.
vec3 ggx_lobe(vec3 N, vec3 V, vec3 L, float roughness) {
    vec3 H = normalize(V + L);
    float ndv = max(dot(N, V), 0.0);
    float ndl = max(dot(N, L), 0.0);
    float ndh = max(dot(N, H), 0.0);
    float vdh = max(dot(V, H), 0.0);
    float d = distribution_ggx(ndh, roughness);
    float g = geometry_schlick(ndv, roughness) * geometry_schlick(ndl, roughness);
    vec3 f = fresnel_schlick(vdh, vec3(0.04));
    return d * g * f / max(4.0 * ndv * ndl, 1e-4);
}

// Full point shading. `shadow` is the sun's visibility
// factor in [0,1]; the fill light is unshadowed in rung 1
// (named omission: EEVEE casts fill shadows too — probe
// evidence in the rung report — and rung 3 owns closing
// it; the sun is the 3.0-energy key light, the fill 1.1).
vec3 shade_point(
    vec3 albedo,
    vec3 N,
    vec3 V,
    float roughness,
    vec3 world_colour,
    vec3 sun_to_light,
    float sun_energy,
    float shadow,
    vec3 fill_to_light,
    float fill_energy
) {
    float sun_wrap = wrap_diffuse(dot(N, sun_to_light));
    float fill_wrap = wrap_diffuse(dot(N, fill_to_light));
    vec3 diffuse = albedo * (world_colour
        + sun_energy * sun_wrap * shadow
        + fill_energy * fill_wrap);
    vec3 specular = sun_energy * shadow * ggx_lobe(N, V, sun_to_light, roughness)
        + fill_energy * ggx_lobe(N, V, fill_to_light, roughness);
    return diffuse + specular;
}

// AgX: the Filament fit of Blender's AgX (Filament PR
// 7236, as shipped in three.js r180's tonemapping chunk),
// transcribed verbatim — Rec.2020 conversion, inset
// matrix, log2 encoding over [-12.47393, +4.026069],
// the Narkowicz polynomial sigmoid on the encoded value,
// outset matrix, and the fit's pow(2.2) linearization.
// (An earlier draft of this file mixed constants from a
// different fit and dropped the log encoding; the result
// exploded outside [0, 0.4]. The constants below are one
// consistent set, unit-tested at the neutral points.)
vec3 agx_default_contrast_approx(vec3 x) {
    vec3 x2 = x * x;
    vec3 x4 = x2 * x2;
    return +15.5 * x4 * x2
        - 40.14 * x4 * x
        + 31.96 * x4
        - 6.868 * x2 * x
        + 0.4298 * x2
        + 0.1191 * x
        - 0.00232;
}

vec3 tonemap_agx(vec3 color) {
    const mat3 srgb_to_rec2020 = mat3(
        vec3(0.6274, 0.0691, 0.0164),
        vec3(0.3293, 0.9195, 0.0880),
        vec3(0.0433, 0.0113, 0.8956)
    );
    const mat3 rec2020_to_srgb = mat3(
        vec3(1.6605, -0.1246, -0.0182),
        vec3(-0.5876, 1.1329, -0.1006),
        vec3(-0.0728, -0.0083, 1.1187)
    );
    const mat3 inset = mat3(
        vec3(0.856627153315983, 0.137318972929847, 0.11189821299995),
        vec3(0.0951212405381588, 0.761241990602591, 0.0767994186031903),
        vec3(0.0482516061458583, 0.101439036467562, 0.811302368396859)
    );
    const mat3 outset = mat3(
        vec3(1.1271005818144368, -0.1413297634984383, -0.14132976349843826),
        vec3(-0.11060664309660323, 1.157823702216272, -0.11060664309660294),
        vec3(-0.016493938717834573, -0.016493938717834257, 1.2519364065950405)
    );
    const float agx_min_ev = -12.47393;
    const float agx_max_ev = 4.026069;
    color = srgb_to_rec2020 * color;
    color = inset * color;
    color = max(color, vec3(1e-10));
    color = log2(color);
    color = (color - agx_min_ev) / (agx_max_ev - agx_min_ev);
    color = clamp(color, 0.0, 1.0);
    color = agx_default_contrast_approx(color);
    color = outset * color;
    color = pow(max(vec3(0.0), color), vec3(2.2));
    color = rec2020_to_srgb * color;
    return clamp(color, 0.0, 1.0);
}

vec3 srgb_encode(vec3 c) {
    vec3 low = 12.92 * c;
    vec3 high = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return mix(low, high, step(vec3(0.0031308), c));
}

// Linear HDR -> display bytes, the pipeline's output
// transform.
vec3 display_transform(vec3 linear) {
    return srgb_encode(tonemap_agx(linear));
}
