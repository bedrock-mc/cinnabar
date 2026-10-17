#version 330 compatibility

// Overworld composite: adds a sky tint from its own texture.
uniform sampler2D colortex0;
uniform sampler2D colortex4;

in vec2 uv;

/* RENDERTARGETS: 0 */
layout(location = 0) out vec4 color;

void main() {
    color = texture(colortex0, uv) + texture(colortex4, uv) * 0.1;
}
