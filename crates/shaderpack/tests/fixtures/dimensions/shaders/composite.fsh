#version 330 compatibility

// Root composite: part of the root program set, which `world0` replaces as the base set.
uniform sampler2D colortex0;

in vec2 uv;

/* RENDERTARGETS: 0 */
layout(location = 0) out vec4 color;

void main() {
    color = texture(colortex0, uv);
}
