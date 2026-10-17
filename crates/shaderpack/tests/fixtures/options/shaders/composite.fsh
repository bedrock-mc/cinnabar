#version 330 compatibility

#include "/lib/settings.glsl"

uniform sampler2D colortex0;
uniform float shadowWeights[SHADOW_TAPS];

#ifdef SOFT_SHADOWS
uniform float penumbraScale;
#endif

#ifdef VOLUMETRIC_FOG
uniform sampler2D noisetex;
#endif

in vec2 uv;

/* RENDERTARGETS: 0 */
layout(location = 0) out vec4 color;

void main() {
    vec3 c = texture(colortex0, uv).rgb;
    float shade = 0.0;
    for (int i = 0; i < SHADOW_TAPS; i++) {
        shade += shadowWeights[i];
    }
#ifdef SOFT_SHADOWS
    shade *= penumbraScale;
#endif
#ifdef VOLUMETRIC_FOG
    c = mix(c, vec3(0.7), texture(noisetex, uv).r);
#endif
    color = vec4(c * shade, 1.0);
}
