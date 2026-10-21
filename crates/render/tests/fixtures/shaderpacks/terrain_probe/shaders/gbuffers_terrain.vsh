#version 120
// Original fixture: forwards the terrain attributes the geometry adapter supplies.
varying vec2 texcoord;
varying vec2 lightcoord;
varying vec4 tint;
varying float blockId;
attribute vec4 mc_Entity;

void main() {
    gl_Position = gl_ProjectionMatrix * (gl_ModelViewMatrix * gl_Vertex);
    texcoord = (gl_TextureMatrix[0] * gl_MultiTexCoord0).xy;
    lightcoord = (gl_TextureMatrix[1] * gl_MultiTexCoord1).xy;
    tint = gl_Color;
    blockId = mc_Entity.x;
}
