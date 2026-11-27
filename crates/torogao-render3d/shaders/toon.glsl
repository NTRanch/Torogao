// The toon shading model (gap G1; Brief Amendment A3 and
// the R6 channel assignment). Estate-authored: the ramp
// does the terminator work (quantized steps, not a smooth
// NdotL), faces read the painted SDF by head-relative
// light angle and never use NdotL for the key light, hair
// adds a sphere-map sheen. The Blender parity fixture
// implements this same contract from the same textures.
// Pure functions over sampled values; character.frag owns
// the sampler declarations and the orchestration.

// Face-mask tint targets (linear), applied to the albedo
// before lighting.
const vec3 TOON_BLUSH_RGB = vec3(0.72, 0.30, 0.26);
const vec3 TOON_LIP_RGB = vec3(0.55, 0.16, 0.14);
// Hair sheen strength at full spec mask and full sun.
const float TOON_SHEEN_SCALE = 0.6;
// Toon specular exponent (Blinn lobe; the parity
// fixture's node setup uses the identical term).
const float TOON_SPEC_EXPONENT = 48.0;

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

// The face's lit factor: the painted SDF field (remapped
// to 0.1..1.0) against the sweep threshold the
// head-relative light angle sets. 1 = fully lit.
float toon_face_lit(float sdf_r, float sweep, float softness) {
    float field = sdf_r * 0.9 + 0.1;
    return smoothstep(sweep - softness, sweep + softness, field);
}

// Toon specular: one Blinn lobe per light, gated by the
// packed spec mask, tinted toward the albedo by the
// packed metalness. Returned per unit light energy; the
// caller scales by energy and shadow.
vec3 toon_spec(vec3 n, vec3 v, vec3 l, float spec_mask, float metal, vec3 albedo) {
    vec3 h = normalize(l + v);
    float ndh = max(dot(n, h), 0.0);
    vec3 tint = mix(vec3(1.0), albedo, metal);
    return tint * (pow(ndh, TOON_SPEC_EXPONENT) * spec_mask);
}
