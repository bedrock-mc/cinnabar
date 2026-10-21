#version 330 compatibility

#include "/lib/common.glsl"
#include "/lib/broken.glsl"

uniform sampler2D colortex0;

in vec2 uv;

/* RENDERTARGETS: 0 */
layout(location = 0) out vec4 sharpened;

void main() {
    sharpened = vec4(sharpen(gradeColor(texture(colortex0, uv).rgb)), 1.0);
}
