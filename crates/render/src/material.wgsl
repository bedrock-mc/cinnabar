#define_import_path cinnabar::material

struct MaterialGpu {
    texture: u32,
    flags: u32,
    animation: u32,
    variation_start: u32,
    variation_count: u32,
    variation_weight: u32,
}

@group(0) @binding(3) var<storage, read> materials: array<MaterialGpu>;

const TWO_SIDED: u32 = MATERIAL_TWO_SIDED_FLAG;
const NATIVE_LEAF_COLOUR: u32 = MATERIAL_NATIVE_LEAF_COLOUR_FLAG;
const OVERLAY_MASK: u32 = MATERIAL_OVERLAY_MASK_FLAG;

fn material_face_is_visible(flags: u32, front: bool) -> bool {
    return front || (flags & TWO_SIDED) != 0u;
}

fn material_uses_native_leaf_colour(flags: u32) -> bool {
    return (flags & NATIVE_LEAF_COLOUR) != 0u;
}

fn material_uses_overlay_mask(flags: u32) -> bool {
    return (flags & OVERLAY_MASK) != 0u;
}

// Native texture choices and isotropic UVs use the same wrapping position mix.
fn material_position_random(position: vec3<i32>) -> u32 {
    let p = vec3<u32>(position);
    let h = (p.z * 0x06ebfff5u) ^ (p.x * 0x002fc20fu) ^ p.y;
    return (h * 0x0285b825u + 11u) * h;
}

// Current cube tessellation applies this only to faces
// enabled by blocks.json's pack-authored isotropic mask.
fn material_uv_flags(flags: u32, position: vec3<i32>) -> u32 {
    if ((flags & MATERIAL_ISOTROPIC_FLAG) == 0u || (flags & 3u) != 0u) {
        return flags;
    }
    let native_code = (material_position_random(position) >> 24u) & 3u;
    // Native codes 2/3 are quarter-turn/half-turn; our packed codes swap them.
    return flags | select(native_code, native_code ^ 1u, native_code >= 2u);
}

// Vanilla ambient occlusion raises the four-sample average * face
// coefficient to the source block's exponent before vertex interpolation.
fn material_leaf_shade(ao_face: f32, flags: u32) -> f32 {
    let encoded = (flags & MATERIAL_LEAF_AO_EXPONENT_MASK) >> MATERIAL_LEAF_AO_EXPONENT_SHIFT;
    if (!material_uses_native_leaf_colour(flags) || encoded == 0u) { return ao_face; }
    return pow(ao_face, f32(encoded) / MATERIAL_LEAF_AO_EXPONENT_SCALE);
}

// Vanilla uses wrapping coordinates, subtractive weights, last fallback.
fn positional_material(id: u32, position: vec3<i32>) -> MaterialGpu {
    let base = materials[id];
    if (base.variation_count == 0u) { return base; }
    if (base.variation_count == 1u) { return materials[base.variation_start]; }
    var sample = f32(material_position_random(position) >> 16u) / 65535.0;
    for (var i = 0u; i < base.variation_count; i += 1u) {
        let candidate = materials[base.variation_start + i];
        let weight = bitcast<f32>(candidate.variation_weight);
        if (sample <= weight || i + 1u == base.variation_count) { return candidate; }
        sample -= weight;
    }
    return base;
}
