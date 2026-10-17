#version 430

// Seeds a scratch image once per frame; the image format comes from shaders.properties.

layout(local_size_x = 8, local_size_y = 8) in;
const ivec3 workGroups = ivec3(4, 2, 1);

uniform image2D scratchImg;
uniform float frameTimeCounter;

void main() {
    ivec2 texel = ivec2(gl_GlobalInvocationID.xy);
    vec2 cell = vec2(texel) / vec2(32.0, 16.0);
    imageStore(scratchImg, texel, vec4(cell, fract(frameTimeCounter), 1.0));
}
