#version 330 compatibility

// Fog pass: only meaningful (and only compiles) with VOLUMETRIC_FOG on.
#include "/lib/settings.glsl"

uniform sampler2D colortex0;
#ifdef VOLUMETRIC_FOG
uniform sampler2D noisetex;
#endif

in vec2 uv;

/* RENDERTARGETS: 0 */
layout(location = 0) out vec4 color;

void main() {
    float density = texture(noisetex, uv * 4.0).r;
    color = vec4(mix(texture(colortex0, uv).rgb, vec3(0.6, 0.65, 0.7), density), 1.0);
}
