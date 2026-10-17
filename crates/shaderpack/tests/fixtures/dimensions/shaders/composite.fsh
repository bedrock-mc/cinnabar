#version 330 compatibility

// Root composite: used by every dimension without its own.
uniform sampler2D colortex0;

in vec2 uv;

/* RENDERTARGETS: 0 */
layout(location = 0) out vec4 color;

void main() {
    color = texture(colortex0, uv);
}
