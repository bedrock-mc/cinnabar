#version 120

// Terrain with gentle foliage sway, written against the compatibility profile.

attribute vec4 mc_Entity;
attribute vec4 at_tangent;
attribute vec2 mc_midTexCoord;

uniform float frameTimeCounter;
uniform bool isRaining;

varying vec2 texcoord;
varying vec2 lightcoord;
varying vec3 viewNormal;
varying float blockId;

void main() {
    texcoord = gl_MultiTexCoord0.xy;
    lightcoord = gl_MultiTexCoord1.xy / 240.0;
    gl_FrontColor = gl_Color;
    viewNormal = normalize(gl_NormalMatrix * gl_Normal);
    blockId = mc_Entity.x;

    vec2 sway = (texcoord - mc_midTexCoord) * at_tangent.w;
    float strength = isRaining ? 0.02 : 0.01;
    vec4 viewPos = gl_ModelViewMatrix * gl_Vertex;
    viewPos.xy += sway * sin(frameTimeCounter) * strength;
    gl_Position = gl_ProjectionMatrix * viewPos;
}
