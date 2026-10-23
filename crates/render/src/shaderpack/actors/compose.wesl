// Composes one actor artwork layer as its native material does, into an opaque layer the
// pack's `gtexture` samples. Native colour-mask and multitexture materials have neither
// alpha test nor blending, so their alpha weights colour and is never coverage. Texels are
// raw gamma bytes, the space both native materials compose in.

struct ComposeParams {
    // x: base layer; y and z: the multitexture overlay layers.
    layers: vec4<u32>,
    // Packed RGBA8 dye of a colour-mask layer; zero leaves its colour unchanged.
    tint: u32,
    mode: u32,
}

const COMPOSE_MULTITEXTURE: u32 = COMPOSE_MULTITEXTURE_MODE;

@group(0) @binding(0) var artwork: texture_2d_array<f32>;
@group(0) @binding(1) var composed: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> params: ComposeParams;

// Array indices clamp as the native material's sampled layers do.
fn artwork_texel(coord: vec2<u32>, layer: u32) -> vec4<f32> {
    return textureLoad(artwork, coord, min(layer, textureNumLayers(artwork) - 1u), 0);
}

@compute @workgroup_size(COMPOSE_WORKGROUP_SIDE, COMPOSE_WORKGROUP_SIDE)
fn compose_layer(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= textureDimensions(composed))) {
        return;
    }
    let color = artwork_texel(id.xy, params.layers.x);
    var rgb = color.rgb;
    if (params.mode == COMPOSE_MULTITEXTURE) {
        // llama:entity_multitexture: each overlay's alpha weights its RGB over the base.
        let tex1 = artwork_texel(id.xy, params.layers.y);
        let tex2 = artwork_texel(id.xy, params.layers.z);
        rgb = mix(mix(rgb, tex1.rgb, tex1.a), tex2.rgb, tex2.a);
    } else if (params.tint != 0u) {
        // entity_change_color: alpha weights the dye.
        rgb = mix(rgb, rgb * unpack4x8unorm(params.tint).rgb, color.a);
    }
    textureStore(composed, id.xy, vec4(rgb, 1.0));
}
