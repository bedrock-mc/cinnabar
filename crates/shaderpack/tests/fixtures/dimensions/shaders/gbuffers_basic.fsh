#version 120

varying vec4 tint;

void main() {
/* DRAWBUFFERS:0 */
    gl_FragData[0] = tint;
}
