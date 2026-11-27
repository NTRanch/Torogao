#version 460
// Outline fragment: the hull's flat ink — the vertex
// paint darkened by the outline factor (applied in the
// vertex shader), through the shared display transform
// so the ink sits in the same output space as the body.

#include "shade.glsl"

layout(location = 0) in vec3 v_color;
layout(location = 0) out vec4 out_colour;

void main() {
    out_colour = vec4(display_transform(v_color), 1.0);
}
