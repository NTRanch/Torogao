// The toon shading model (gap G1; Brief Amendment A3 and
// the R6 channel assignment). Estate-authored: the ramp
// does the terminator work (quantized steps, not a smooth
// NdotL), faces read the painted SDF by head-relative
// light angle and never use NdotL for the key light, hair
// adds a sphere-map sheen. The Blender parity fixture
// implements this same contract from the same textures.
// Pure functions over sampled values; character.frag owns
// the sampler declarations and the orchestration.
//
// Gap-fix round (2026-11-26, from the StellarToon/LSCherry
// grammar study): the per-ID parameter table (toon_params,
// decoded below) carries the colour/parameter set the
// grammar requires per material — SSS tint, rim, specular
// shape, back-face colour, hair highlight — so no shading
// term below hardcodes a per-material value. Placement
// stays authored (packed lightmap, face SDF, sphere map,
// planar TEXCOORD_2); this file only bands and tints.

// Hair sheen strength at full spec mask and full sun.
const float TOON_SHEEN_SCALE = 0.6;

struct ToonSample {
    float ao;
    float spec;
    float metal;
    int id;
};

// Decode one packed-lightmap texel: the colour channels
// come from the LINEAR sample; the material ID comes
// from the NEAREST sample of the same texture, so IDs
// never blend across island borders.
ToonSample toon_decode(vec4 packed_linear, float id_alpha_nearest) {
    ToonSample s;
    s.ao = packed_linear.r;
    s.spec = packed_linear.g;
    s.metal = packed_linear.b;
    s.id = int(clamp(id_alpha_nearest * 15.0 + 0.5, 0.0, 15.0));
    return s;
}

// Per-ID ramp lookup: u is the remapped lighting term,
// v picks the ID's row in the warm bank (rows 0..15) or
// the cool bank (rows 16..31), blended by warm_cool.
vec3 toon_ramp(sampler2D ramp_tex, float u, int id, float warm_cool) {
    float uu = clamp(u, 0.0, 1.0);
    vec3 warm = texture(ramp_tex, vec2(uu, (float(id) + 0.5) / 32.0)).rgb;
    vec3 cool = texture(ramp_tex, vec2(uu, (float(id) + 16.5) / 32.0)).rgb;
    return mix(warm, cool, warm_cool);
}

// The per-ID parameter set, decoded from the params
// texture (16x5; layout contract in toon.rs
// ToonMaterialParams). texelFetch: IDs never blend.
struct ToonParams {
    vec3 sss_color;
    float sss_width;
    vec3 rim_color;
    float rim_size;
    float rim_smooth;
    float spec_exponent;
    float spec_tint;
    vec3 back_color;
    vec3 highlight_color;
    float highlight_strength;
};

ToonParams toon_params(sampler2D params_tex, int id) {
    vec4 r0 = texelFetch(params_tex, ivec2(id, 0), 0);
    vec4 r1 = texelFetch(params_tex, ivec2(id, 1), 0);
    vec4 r2 = texelFetch(params_tex, ivec2(id, 2), 0);
    vec4 r3 = texelFetch(params_tex, ivec2(id, 3), 0);
    vec4 r4 = texelFetch(params_tex, ivec2(id, 4), 0);
    ToonParams p;
    p.sss_color = r0.rgb;
    p.sss_width = r0.a * 0.5;
    p.rim_color = r1.rgb;
    p.rim_size = r1.a;
    p.rim_smooth = r2.r;
    p.spec_tint = r2.g;
    p.spec_exponent = 1.0 + r2.b * 255.0;
    p.back_color = r3.rgb;
    p.highlight_color = r4.rgb;
    p.highlight_strength = r4.a * 4.0;
    return p;
}

// The face's lit factor: the painted SDF field (remapped
// to 0.1..1.0) against the sweep threshold the
// head-relative light angle sets. 1 = fully lit.
float toon_face_lit(float sdf_r, float sweep, float softness) {
    float field = sdf_r * 0.9 + 0.1;
    return smoothstep(sweep - softness, sweep + softness, field);
}

// Toon specular: one Blinn lobe per light, gated by the
// packed spec mask. The tint blends toward the albedo by
// metalness AND by the per-ID specular tint (the tint
// acts on the dielectric part; metalness still wins
// fully at 1). The exponent is per-ID (GAP-3).
vec3 toon_spec(vec3 n, vec3 v, vec3 l, float spec_mask, float metal,
               vec3 albedo, float exponent, float spec_tint) {
    vec3 h = normalize(l + v);
    float ndh = max(dot(n, h), 0.0);
    float tint_amount = clamp(metal + spec_tint * (1.0 - metal), 0.0, 1.0);
    vec3 tint = mix(vec3(1.0), albedo, tint_amount);
    return tint * (pow(ndh, exponent) * spec_mask);
}

// SSS transition band (GAP-1): a bump centred on the
// ramp's midpoint, width from the per-ID table, shaped
// independently of the diffuse ramp. Black SSS colour
// disables it (the grammar's disable-by-black rule).
float toon_sss_zone(float ramp_u, float width) {
    float w = max(width, 0.001);
    return smoothstep(0.5 - w, 0.5, ramp_u)
        * (1.0 - smoothstep(0.5, 0.5 + w, ramp_u));
}

// Rim band (GAP-2): the Fresnel range's outer `size`
// fraction, softened by `smooth`. Colour and the master
// strength multiply outside.
float toon_rim_band(float ndv, float size, float softness) {
    float fres = 1.0 - clamp(ndv, 0.0, 1.0);
    float lo = max(1.0 - size - softness, 0.0);
    float hi = min(1.0 - size + softness, 1.0);
    if (hi <= lo) {
        return step(1.0 - size, fres);
    }
    return smoothstep(lo, hi, fres);
}

// Hair planar bands (GAP-4): repeating bands along the
// authored planar along-length coordinate t (TEXCOORD_2,
// contract in toon.rs / model.rs). Two layouts — the
// base frequency and its quarter — blended by `mixv`;
// the result is windowed to [win_lo, win_hi] on t and
// phase-shifted by `offset` cycles. sin^2 gives one soft
// pulse per cycle.
float toon_hair_bands(float t, float frequency, float mixv, float offset,
                      float win_lo, float win_hi) {
    float ph_hi = fract(t * frequency + offset);
    float ph_lo = fract(t * frequency * 0.25 + offset);
    float b_hi = pow(sin(3.14159265 * ph_hi), 2.0);
    float b_lo = pow(sin(3.14159265 * ph_lo), 2.0);
    float band = mix(b_lo, b_hi, mixv);
    float window = smoothstep(win_lo - 0.05, win_lo + 0.05, t)
        * (1.0 - smoothstep(win_hi - 0.05, win_hi + 0.05, t));
    return band * window;
}

// World-colour calibration (GAP-7): HSV rebalance of
// the ambient colour — saturation and value multipliers
// plus the value-enhance gain — applied once where the
// character shader consumes the world colour, so every
// material reads the same calibrated ambient.
vec3 toon_world_calibrate(vec3 rgb, float saturation, float value,
                          float enhance) {
    vec3 c = max(rgb, vec3(0.0));
    float mx = max(max(c.r, c.g), c.b);
    float mn = min(min(c.r, c.g), c.b);
    float d = mx - mn;
    float h = 0.0;
    if (d > 1e-6 && mx > 1e-6) {
        if (mx == c.r) {
            h = mod((c.g - c.b) / d, 6.0);
        } else if (mx == c.g) {
            h = (c.b - c.r) / d + 2.0;
        } else {
            h = (c.r - c.g) / d + 4.0;
        }
        h /= 6.0;
    }
    float s = (mx > 1e-6) ? d / mx : 0.0;
    s = clamp(s * saturation, 0.0, 1.0);
    float v = mx * value * (1.0 + enhance);
    float h6 = h * 6.0;
    float cc = v * s;
    float x = cc * (1.0 - abs(mod(h6, 2.0) - 1.0));
    vec3 base;
    if (h6 < 1.0) {
        base = vec3(cc, x, 0.0);
    } else if (h6 < 2.0) {
        base = vec3(x, cc, 0.0);
    } else if (h6 < 3.0) {
        base = vec3(0.0, cc, x);
    } else if (h6 < 4.0) {
        base = vec3(0.0, x, cc);
    } else if (h6 < 5.0) {
        base = vec3(x, 0.0, cc);
    } else {
        base = vec3(cc, 0.0, x);
    }
    return base + vec3(v - cc);
}
