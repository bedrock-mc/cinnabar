#version 330 compatibility

in vec2 uv;

#include "/lib/does_not_exist.glsl"

/* RENDERTARGETS: 0 */
layout(location = 0) out vec4 color;

void main() {
    color = vec4(uv, 0.0, 1.0);
}
