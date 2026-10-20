#version 330 compatibility

// Nether-only final pass: warms the image.
uniform sampler2D colortex0;

in vec2 uv;

layout(location = 0) out vec4 color;

void main() {
    color = texture(colortex0, uv) * vec4(1.1, 0.95, 0.9, 1.0);
}
