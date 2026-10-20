#version 120

uniform sampler2D colortex3;

varying vec2 uv;

void main() {
    vec4 glow = texture2D(colortex3, uv);
/* DRAWBUFFERS:02 */
    gl_FragData[0] = glow;
    gl_FragData[1] = vec4(glow.rgb * 0.5, 1.0);
    gl_FragData[2] = vec4(0.0);
}
