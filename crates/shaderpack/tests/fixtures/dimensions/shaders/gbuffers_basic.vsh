#version 120

varying vec4 tint;

void main() {
    tint = gl_Color;
    gl_Position = ftransform();
}
