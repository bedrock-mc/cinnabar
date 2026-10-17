#version 120

uniform sampler2D gtexture;
uniform sampler2D lightmap;
uniform sampler2DShadow shadowtex0;
uniform bool isRaining;

varying vec2 texcoord;
varying vec2 lightcoord;
varying vec3 viewNormal;
varying float blockId;

void main() {
    vec4 albedo = texture2D(gtexture, texcoord) * gl_Color;
    vec3 sky = texture2D(lightmap, lightcoord).rgb;
    float lit = shadow2D(shadowtex0, vec3(texcoord, 0.5)).r;
    if (isRaining) {
        albedo.rgb *= 0.8;
    }

/* DRAWBUFFERS:01 */
    gl_FragData[0] = vec4(albedo.rgb * sky * lit, albedo.a);
    gl_FragData[1] = vec4(viewNormal * 0.5 + 0.5, blockId / 255.0);
}
