#version 400 compatibility

out vec4 lineColor;

void main() {
    lineColor = gl_Color;
    gl_Position = ftransform();
}
