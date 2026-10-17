#version 430

// Halves the scratch image into a luminance image with an explicit format.

layout(local_size_x = 16) in;
const vec2 workGroupsRender = vec2(0.5, 0.5);

layout(rgba16f) uniform readonly image2D scratchImg;
layout(r32f) uniform writeonly image2D lumaImg;

void main() {
    ivec2 texel = ivec2(gl_GlobalInvocationID.xy);
    vec3 c = imageLoad(scratchImg, texel * 2).rgb;
    imageStore(lumaImg, texel, vec4(dot(c, vec3(0.25, 0.5, 0.25))));
}
