#version 120
// Original fixture: albedo to colortex0, lightmap coordinates and block id to colortex1.
uniform sampler2D gtexture;
varying vec2 texcoord;
varying vec2 lightcoord;
varying vec4 tint;
varying float blockId;

void main() {
    /* DRAWBUFFERS:01 */
    gl_FragData[0] = texture2D(gtexture, texcoord) * tint;
    gl_FragData[1] = vec4(lightcoord, blockId / 255.0, 1.0);
}
