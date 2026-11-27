#version 460
// Splat fragment shader: evaluate the projected 2D
// Gaussian at this pixel, and emit the splat's SH
// colour through the renderer's display transform
// with the Gaussian weight as alpha. The pipeline's
// blend state is the "over" operator — the same
// convention as the film overlay pass, in display
// space (DESIGN.md §2.2).

#include "shade.glsl"

layout(location = 0) in vec4 v_conic;
layout(location = 1) in vec4 v_colour;
layout(location = 2) in vec2 v_offset_px;

layout(location = 0) out vec4 out_colour;

void main() {
    float dx = v_offset_px.x;
    float dy = v_offset_px.y;
    float power = -0.5 * (v_conic.x * dx * dx + v_conic.z * dy * dy)
        - v_conic.y * dx * dy;
    if (power > 0.0) {
        discard;
    }
    float alpha = v_conic.w * exp(power);
    if (alpha < 1.0 / 255.0) {
        discard;
    }
    out_colour = vec4(display_transform(v_colour.rgb), alpha);
}
