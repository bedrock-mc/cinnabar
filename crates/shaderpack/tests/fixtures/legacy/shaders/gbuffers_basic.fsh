#version 400 compatibility

in vec4 lineColor;

void main() {
    gl_FragData[0] = lineColor;
}
