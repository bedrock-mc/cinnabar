import render::lighting::{tint_to_gamma, tint_to_linear};

// The CPU packs vanilla's 27-sample biome counts at each 3D lattice point.
// BIOME_CONSTANTS

struct BiomeTintGpu {
    grass: u32,
    foliage: u32,
    birch: u32,
    evergreen: u32,
    dry_foliage: u32,
    water: u32,
    flags: u32,
    water_opacity: f32,
    seasonal_foliage: array<vec4<f32>, SEASONAL_FOLIAGE_COUNT>,
}

@group(0) @binding(7) var<storage, read> biome_records: array<u32>;
@group(0) @binding(8) var<storage, read> biome_tints: array<BiomeTintGpu>;

// Query kernel tables live in a uniform block: a dynamically indexed `const` array is copied
// into per-invocation scratch by several compilers, once per tinted fragment.
struct BiomeQueryTables {
    stencils: array<vec4<u32>, BIOME_STENCIL_VEC4S>,
    weights: array<vec4<f32>, BIOME_WEIGHT_VEC4S>,
    permutation: array<vec4<u32>, GRASS_PERMUTATION_VEC4S>,
}
@group(0) @binding(BIOME_QUERY_TABLES_BINDING) var<uniform> biome_query_tables: BiomeQueryTables;

fn query_stencil(index: u32) -> vec2<u32> {
    let pair = biome_query_tables.stencils[index / 2u];
    return select(pair.xy, pair.zw, (index & 1u) != 0u);
}

fn grass_permutation(index: u32) -> u32 {
    return biome_query_tables.permutation[index / 4u][index % 4u];
}

// Check before addition: a stale record must not wrap into another GPU allocation.
fn biome_record_span_valid(start: u32, words: u32) -> bool {
    let length = arrayLength(&biome_records);
    if (start > length) { return false; }
    return words <= length - start;
}

fn lattice_biome_count(start: u32) -> u32 {
    // Buffer bounds alone do not bound shader work. The packed format has a
    // fixed maximum even if a bad address reads a header or floating-point bits.
    return min(biome_records[start], BIOME_BIOME_LIMIT);
}

fn unpack_linear_rgb10(packed: u32) -> vec3<f32> {
    return vec3<f32>(
        f32(packed & 0x3ffu),
        f32((packed >> 10u) & 0x3ffu),
        f32((packed >> 20u) & 0x3ffu),
    ) / 1023.0;
}

fn unpack_water_rgb8(packed: u32) -> vec3<f32> {
    let gamma = vec3(f32(packed & 255u), f32((packed >> 8u) & 255u), f32((packed >> 16u) & 255u)) / 255.0;
    return tint_to_linear(vec4(gamma, 1.0)).rgb;
}

fn packed_payload_tint_index(payload: u32, coordinate: vec3<u32>) -> u32 {
    if (!biome_record_span_valid(payload, 1u)) { return 0u; }
    let header = biome_records[payload];
    let bits = header & 0xffu;
    let palette_len = (header >> 8u) & 0x1fffu;
    if (palette_len == 0u || bits > 32u) {
        return 0u;
    }
    var word_count = 0u;
    var palette_index = 0u;
    if (bits != 0u) {
        let per_word = 32u / bits;
        word_count = (4096u + per_word - 1u) / per_word;
    }
    if (!biome_record_span_valid(payload, 1u + word_count + palette_len)) { return 0u; }
    if (bits != 0u) {
        let per_word = 32u / bits;
        let linear = (coordinate.x << 8u) | (coordinate.z << 4u) | coordinate.y;
        let word = biome_records[payload + 1u + linear / per_word];
        palette_index = (word >> ((linear % per_word) * bits)) & (0xffffffffu >> (32u - bits));
    }
    if (palette_index >= palette_len) {
        return 0u;
    }
    return biome_records[payload + 1u + word_count + palette_index];
}

fn packed_biome_tint_index(record: u32, source_coordinate: vec3<i32>) -> u32 {
    if (!biome_record_span_valid(record, BIOME_DESCRIPTOR_WORDS)) { return 0u; }
    if (biome_records[record] != BIOME_DESCRIPTOR_MAGIC) {
        return 0u;
    }
    let offset = source_coordinate >> vec3(4u);
    if (any(offset < vec3(-1)) || any(offset > vec3(1))) { return 0u; }
    let layer = select(select(2u, 1u, offset.y < 0), 0u, offset.y == 0);
    let slot = layer * 9u + u32((offset.z + 1) * 3 + offset.x + 1);
    let relative = biome_records[record + 2u + slot];
    if (relative == 0u || relative >= arrayLength(&biome_records) - record) { return 0u; }
    return packed_payload_tint_index(record + relative, vec3<u32>(source_coordinate & vec3(15)));
}

// Lookups take an index and read single fields: copying a whole `BiomeTintGpu` (with its
// dynamically indexed seasonal array) per sample spills it to per-fragment scratch memory.
fn safe_tint_index(index: u32) -> u32 {
    return select(0u, index, index < arrayLength(&biome_tints));
}

fn tint_domain_colour(index: u32, tint_kind: u32, material_flags: u32, world_position: vec3<i32>) -> vec4<f32> {
    let tint = safe_tint_index(index);
    if (tint_kind == 0x10u) {
        if ((biome_tints[tint].flags & BIOME_SWAMP_GRASS) != 0u && arrayLength(&biome_tints) >= BIOME_TINT_MAP_SIZE) {
            let palette = grass_palette_index(world_position.xz);
            return vec4(unpack_linear_rgb10(biome_tints[arrayLength(&biome_tints) - BIOME_TINT_MAP_SIZE + palette].grass), 1.0);
        }
        return vec4(unpack_linear_rgb10(biome_tints[tint].grass), 1.0);
    }
    if (tint_kind == 0x30u) {
        return vec4(unpack_water_rgb8(biome_tints[tint].water), biome_tints[tint].water_opacity);
    }
    return vec4(special_foliage_tint(tint, material_flags), 1.0);
}

fn special_foliage_tint(tint: u32, material_flags: u32) -> vec3<f32> {
    if ((material_flags & MATERIAL_SEASONAL_FOLIAGE) != 0u && (biome_tints[tint].flags & BIOME_SEASONAL_FOLIAGE) != 0u) {
        var species = SEASONAL_DEFAULT_CELL;
        if ((material_flags & 0x600u) == 0x400u) { species = SEASONAL_EVERGREEN_CELL; }
        if ((material_flags & 0x600u) == 0x200u) { species = SEASONAL_BIRCH_CELL; }
        let exposed = select(0u, SEASONAL_FOLIAGE_EXPOSED_OFFSET, (material_flags & MATERIAL_EXPOSED_FOLIAGE) != 0u);
        return biome_tints[tint].seasonal_foliage[min(species + exposed, SEASONAL_FOLIAGE_COUNT - 1u)].rgb;
    }
    switch material_flags & 0x600u {
        case 0x200u: { return unpack_linear_rgb10(biome_tints[tint].birch); }
        case 0x400u: { return unpack_linear_rgb10(biome_tints[tint].evergreen); }
        case 0x600u: { return unpack_linear_rgb10(biome_tints[tint].dry_foliage); }
        default: { return unpack_linear_rgb10(biome_tints[tint].foliage); }
    }
}

// Alpha marks gamma tints that remain constant across every block of a uniform record.

fn uniform_biome_tint_gamma(tint_kind: u32, material_flags: u32, record: u32) -> vec4<f32> {
    if (tint_kind == 0u) { return vec4(1.0); }
    if (!biome_record_span_valid(record, BIOME_DESCRIPTOR_WORDS)) { return vec4(0.0); }
    if (biome_records[record] != BIOME_DESCRIPTOR_MAGIC) { return vec4(0.0); }
    let index = biome_records[record + 1u];
    if (index == 0xffffffffu) { return vec4(0.0); }
    let tint = safe_tint_index(index);
    // Swamp grass varies with world-position noise even in a uniform biome.
    if (tint_kind == 0x10u && (biome_tints[tint].flags & BIOME_SWAMP_GRASS) != 0u) { return vec4(0.0); }
    return vec4(tint_to_gamma(tint_domain_colour(tint, tint_kind, material_flags, vec3(0))).rgb, 1.0);
}

fn lattice_point_index(position: vec3<i32>) -> u32 {
    let axis = vec3<u32>((position + vec3(BIOME_LATTICE_STEP)) / BIOME_LATTICE_STEP);
    return (axis.x * BIOME_LATTICE_SIDE + axis.y) * BIOME_LATTICE_SIDE + axis.z;
}

fn blended_biome_tint(
    tint_kind: u32,
    material_flags: u32,
    record: u32,
    local_position: vec3<f32>,
    world_origin: vec3<f32>,
) -> vec4<f32> {
    // Blend the block that owns the fragment, the domain `blend_samples` answers. Interpolated
    // positions leave the sub-chunk at face edges and in derivative helper lanes extrapolated
    // past the horizon; unclamped, their lattice index reads unrelated words as sample counts.
    let coordinate = clamp(vec3<i32>(floor(local_position)), vec3(0), vec3(BIOME_QUERY_SIDE - 1));
    let block_position = coordinate + vec3<i32>(world_origin);
    if (!biome_record_span_valid(record, BIOME_DESCRIPTOR_WORDS)) { return tint_domain_colour(0u, tint_kind, material_flags, block_position); }
    if (biome_records[record] != BIOME_DESCRIPTOR_MAGIC) { return tint_domain_colour(0u, tint_kind, material_flags, block_position); }
    let uniform_tint = biome_records[record + 1u];
    if (uniform_tint != 0xffffffffu) {
        return tint_domain_colour(uniform_tint, tint_kind, material_flags, block_position);
    }
    // Native seasonal foliage samples the block's biome directly, rather than
    // interpolating the ordinary foliage lattice.
    if ((material_flags & MATERIAL_SEASONAL_FOLIAGE) != 0u) {
        let tint = safe_tint_index(packed_biome_tint_index(record, coordinate));
        if ((biome_tints[tint].flags & BIOME_SEASONAL_FOLIAGE) != 0u) {
            return tint_domain_colour(tint, tint_kind, material_flags, block_position);
        }
    }
    let lattice_words = BIOME_LATTICE_SIDE * BIOME_LATTICE_SIDE * BIOME_LATTICE_SIDE * BIOME_POINT_WORDS;
    if (!biome_record_span_valid(record, BIOME_DESCRIPTOR_WORDS + lattice_words)) { return tint_domain_colour(0u, tint_kind, material_flags, block_position); }
    let base = (coordinate - vec3(BIOME_CACHE_ORIGIN)) / BIOME_LATTICE_STEP * BIOME_LATTICE_STEP + vec3(BIOME_CACHE_ORIGIN);
    let residue = vec3<u32>(coordinate - base + vec3(BIOME_RESIDUE_RADIUS));
    if (any(residue >= vec3(BIOME_RESIDUE_SIDE))) { return tint_domain_colour(0u, tint_kind, material_flags, block_position); }
    let stencil = query_stencil((residue.x * BIOME_RESIDUE_SIDE + residue.y) * BIOME_RESIDUE_SIDE + residue.z);
    let magnitude = vec3<u32>(abs(vec3<i32>(residue) - vec3(BIOME_RESIDUE_RADIUS)));
    let magnitude_side = u32(BIOME_RESIDUE_RADIUS) + 1u;
    let weights = ((magnitude.x * magnitude_side + magnitude.y) * magnitude_side + magnitude.z) * 2u;
    var sum = vec4(0.0);
    var denominator = 0.0;
    for (var point = 0u; point < BIOME_QUERY_POINTS; point += 1u) {
        let cell = (stencil[point / 4u] >> (point % 4u * 8u)) & 0xffu;
        let weight = biome_query_tables.weights[weights + point / 4u][point % 4u];
        let position = base + (vec3<i32>(vec3(cell / 9u, cell / 3u % 3u, cell % 3u)) - vec3(1)) * BIOME_LATTICE_STEP;
        if (lattice_point_index(position) >= BIOME_LATTICE_SIDE * BIOME_LATTICE_SIDE * BIOME_LATTICE_SIDE) { return tint_domain_colour(0u, tint_kind, material_flags, block_position); }
        let start = record + BIOME_DESCRIPTOR_WORDS + lattice_point_index(position) * BIOME_POINT_WORDS;
        var colour = vec4(0.0);
        let count = lattice_biome_count(start);
        for (var i = 0u; i < count; i += 1u) {
            let tint = biome_records[start + 1u + i];
            let fraction = bitcast<f32>(biome_records[start + 1u + BIOME_BIOME_LIMIT + i]);
            if (!(fraction >= 0.0 && fraction <= 1.0)) { continue; }
            colour += tint_to_gamma(tint_domain_colour(tint, tint_kind, material_flags, position + vec3<i32>(world_origin))) * fraction;
        }
        sum += clamp(colour, vec4(0.0), vec4(1.0)) * weight;
        denominator += weight;
    }
    return tint_to_linear(sum / denominator);
}

// Legacy world leaf fragments compose in gamma space (native UNORM atlas and
// framebuffer). This preserves extended seasonal RGB; no saturation is applied.
fn blended_biome_tint_gamma(
    tint_kind: u32,
    material_flags: u32,
    record: u32,
    local_position: vec3<f32>,
    world_origin: vec3<f32>,
) -> vec4<f32> {
    return tint_to_gamma(blended_biome_tint(tint_kind, material_flags, record, local_position, world_origin));
}

// Vanilla uses float simplex coordinates and the 12-entry gradient order.
fn grass_corner(cell: vec2<i32>, offset: vec2<f32>) -> f32 {
    let gradients = array<vec2<f32>, 12>(vec2(1.0,1.0),vec2(-1.0,1.0),vec2(1.0,-1.0),vec2(-1.0,-1.0),vec2(1.0,0.0),vec2(-1.0,0.0),vec2(1.0,0.0),vec2(-1.0,0.0),vec2(0.0,1.0),vec2(0.0,-1.0),vec2(0.0,1.0),vec2(0.0,-1.0));
    let hash = grass_permutation((u32(cell.x) + grass_permutation(u32(cell.y) & GRASS_PERMUTATION_MASK)) & GRASS_PERMUTATION_MASK) % 12u;
    let t = max(0.0, (0.5 - offset.x * offset.x) - offset.y * offset.y);
    let gradient = gradients[hash];
    return (offset.y * gradient.y + offset.x * gradient.x) * t * t * t * t;
}

// Vanilla samples row 255 at clamp(int((noise + .6) * 255)).
fn grass_palette_index(world_xz: vec2<i32>) -> u32 {
    let p = vec2<f32>(world_xz) * 0.0225;
    let skew = (p.x + p.y) * bitcast<f32>(0x3ebb67aeu);
    let skewed = p + vec2(skew);
    let cell = vec2<i32>(skewed) - select(vec2(0), vec2(1), skewed <= vec2(0.0));
    let unskew = f32(cell.x + cell.y) * bitcast<f32>(0x3e58658cu);
    let first = p - (vec2<f32>(cell) - vec2(unskew));
    let step = select(vec2(0, 1), vec2(1, 0), first.x > first.y);
    let second = first - vec2<f32>(step) + vec2(bitcast<f32>(0x3e58658cu));
    let third = first - vec2(1.0) + vec2(bitcast<f32>(0x3e58658cu)) + vec2(bitcast<f32>(0x3e58658cu));
    let noise = (grass_corner(cell, first) + grass_corner(cell + step, second) + grass_corner(cell + vec2(1), third)) * 70.0;
    let maximum = i32(BIOME_TINT_MAP_SIZE - 1u);
    return u32(clamp(i32((noise + 0.6) * f32(maximum)), 0, maximum));
}
