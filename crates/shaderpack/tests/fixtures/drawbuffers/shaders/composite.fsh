#version 330 compatibility

/*
const int colortex3Format = RGBA16F;
const bool colortex1Clear = false;
*/

uniform sampler2D colortex0;

in vec2 uv;

/* RENDERTARGETS: 3,1 */
layout(location = 0) out vec4 bloom;
layout(location = 1) out vec4 history;

#ifdef SHADERPACK_FIXTURE_NEVER_DEFINED
/* RENDERTARGETS: 7 */
/*
const int colortex1Format = RG8;
*/
#endif

void main() {
    vec4 c = texture(colortex0, uv);
    bloom = max(c - 1.0, 0.0);
    history = c;
}
