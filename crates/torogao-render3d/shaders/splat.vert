#version 460
// Splat vertex shader: the projection already happened
// on the CPU at pack time (the camera is scene-static
// in this renderer — see splat.rs and DESIGN.md §3).
// This stage only forwards the packed quad vertex:
// clip position, conic + opacity, SH colour, and the
// pixel offset the fragment evaluates the Gaussian on.

layout(location = 0) in vec4 in_clip;
layout(location = 1) in vec4 in_conic;
layout(location = 2) in vec4 in_colour;
layout(location = 3) in vec2 in_offset_px;

layout(location = 0) out vec4 v_conic;
layout(location = 1) out vec4 v_colour;
layout(location = 2) out vec2 v_offset_px;

void main() {
    gl_Position = in_clip;
    v_conic = in_conic;
    v_colour = in_colour;
    v_offset_px = in_offset_px;
}
