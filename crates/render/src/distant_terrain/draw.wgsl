// Distant terrain draws: one indexed draw of every listed cluster's 64 quad slots, pulling
// vertices from the quad pool. Slots past a cluster's quads, dead sections and crack lists not
// drawn this frame collapse to one point outside the clip volume. Faces take their material's
// flat colour, tinted by the section's biome grid, lit through the world lightmap, shaded per
// direction and faded toward the fog colour. The prelude comes first.

@group(0) @binding(0) var<uniform> view: LodView;
@group(0) @binding(1) var<storage, read> sections: array<u32>;
@group(0) @binding(2) var<storage, read> clusters: array<vec4<u32>>;
@group(0) @binding(3) var<storage, read> pool: array<vec2<u32>>;
@group(0) @binding(4) var<storage, read> lists: array<u32>;
@group(0) @binding(5) var<storage, read> tables: array<u32>;
@group(0) @binding(6) var<uniform> lightmap: array<vec4<f32>, LIGHTMAP_ENTRIES>;
@group(0) @binding(7) var<uniform> pass_info: DrawPass;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    // Position relative to the integer eye, for the fog distance.
    @location(0) rel: vec3<f32>,
    // Section-local voxel X and Z of the face's voxel, for the biome grid.
    @location(1) biome_xz: vec2<f32>,
    @location(2) @interpolate(flat) material_face: vec2<u32>,
    // Lightmap entry, then the section's biome grid unit in the pool.
    @location(3) @interpolate(flat) light_biomes: vec2<u32>,
}

fn field(word: u32, shift: u32, mask: u32) -> u32 {
    return (word >> shift) & mask;
}

fn section_word(slot: u32, word: u32) -> u32 {
    return sections[slot * SECTION_RECORD_WORDS + word];
}

fn list_drawn(info: u32, face: u32, list: u32) -> bool {
    if (list == LIST_MAIN) {
        return true;
    }
    let neighbours = field(info, SECTION_NEIGHBOURS_SHIFT, SECTION_NEIGHBOURS_MASK);
    let code = field(neighbours, face * NEIGHBOUR_LEVEL_BITS, (1u << NEIGHBOUR_LEVEL_BITS) - 1u);
    return (list == LIST_TO_COARSER && code == NEIGHBOUR_COARSER)
        || (list == LIST_TO_FINER && code == NEIGHBOUR_FINER);
}

@vertex
fn lod_vertex(
    @builtin(vertex_index) vertex: u32,
    @builtin(instance_index) instance: u32,
) -> VertexOut {
    var out: VertexOut;
    out.clip = vec4(2.0, 2.0, 2.0, 1.0);
    out.rel = vec3(0.0);
    out.biome_xz = vec2(0.0);
    out.material_face = vec2(0u);
    out.light_biomes = vec2(0u);
    var entry = pass_info.base + instance;
    if (pass_info.backward != 0u) {
        entry = pass_info.base - instance;
    }
    let record = clusters[lists[entry]];
    let slot = field(record.y, CLUSTER_SECTION_SHIFT, CLUSTER_SECTION_MASK);
    let quad_slot = vertex >> 2u;
    if (slot == 0u || quad_slot > field(record.y, CLUSTER_COUNT_SHIFT, CLUSTER_COUNT_MASK)) {
        return out;
    }
    let info = section_word(slot, SECTION_INFO_WORD);
    let face = field(record.y, CLUSTER_FACE_SHIFT, CLUSTER_FACE_MASK);
    if (field(info, SECTION_LIVE_SHIFT, SECTION_LIVE_MASK) == 0u
        || !list_drawn(info, face, field(record.y, CLUSTER_LIST_SHIFT, CLUSTER_LIST_MASK))) {
        return out;
    }
    let quad = pool[record.x + quad_slot];
    let axis = face >> 1u;
    let positive = (face & 1u) == 1u;
    // The face's in-plane axes in `Face::in_plane` order.
    let first_axis = select(0u, 1u, axis == 0u);
    let second_axis = select(2u, 1u, axis == 2u);
    var voxel = vec3<f32>(vec3(
        field(quad.x, QUAD_X_SHIFT, QUAD_X_MASK),
        field(quad.x, QUAD_Y_SHIFT, QUAD_Y_MASK),
        field(quad.x, QUAD_Z_SHIFT, QUAD_Z_MASK),
    ));
    var corner = voxel;
    if (positive) {
        corner[axis] += 1.0;
    }
    let point = vertex & 3u;
    if (point == 1u || point == 2u) {
        corner[first_axis] += f32(field(quad.x, QUAD_WIDTH_SHIFT, QUAD_WIDTH_MASK) + 1u);
    }
    if (point >= 2u) {
        corner[second_axis] += f32(field(quad.x, QUAD_HEIGHT_SHIFT, QUAD_HEIGHT_MASK) + 1u);
    }
    let level = field(info, SECTION_LEVEL_SHIFT, SECTION_LEVEL_MASK);
    let cells = vec3(
        bitcast<i32>(section_word(slot, 0u)),
        bitcast<i32>(section_word(slot, 1u)),
        bitcast<i32>(section_word(slot, 2u)),
    );
    let shift = vec3(firstTrailingBit(SECTION_SIDE) + level);
    let origin = vec3<f32>((cells << shift) - view.camera.xyz);
    let rel = origin + corner * f32(1u << level);
    out.clip = view.clip_from_rel * vec4(rel, 1.0);
    out.rel = rel;
    // Half a voxel behind the face, inside the voxel it belongs to.
    var inside = corner;
    inside[axis] += select(0.5, -0.5, positive);
    out.biome_xz = inside.xz;
    let material = field(quad.y, VOXEL_MATERIAL_SHIFT, VOXEL_MATERIAL_MASK);
    out.material_face = vec2(material, face);
    let light = field(quad.y, VOXEL_BLOCK_LIGHT_SHIFT, VOXEL_BLOCK_LIGHT_MASK)
        | (field(quad.y, VOXEL_SKY_LIGHT_SHIFT, VOXEL_SKY_LIGHT_MASK) << 4u);
    out.light_biomes = vec2(light, section_word(slot, SECTION_BIOMES_WORD));
    return out;
}

fn srgb_to_linear(srgb: vec3<f32>) -> vec3<f32> {
    return select(srgb / 12.92, pow((srgb + 0.055) / 1.055, vec3(2.4)), srgb > vec3(0.04045));
}

// The linear tint of `tint_class` in the biome of voxel column `xz` of the grid at `unit`.
fn biome_tint(unit: u32, xz: vec2<f32>, tint_class: u32) -> vec3<f32> {
    let records = view.tables_extra.x;
    if (records == 0u) {
        return vec3(1.0);
    }
    let cell = vec2<u32>(clamp(floor(xz), vec2(0.0), vec2(f32(SECTION_SIDE - 1u))));
    let index = cell.y * SECTION_SIDE + cell.x;
    let pair = pool[unit + index / (2u * BIOME_CELLS_PER_WORD)];
    let word = select(pair.x, pair.y, ((index / BIOME_CELLS_PER_WORD) & 1u) == 1u);
    let raw = (word >> ((index % BIOME_CELLS_PER_WORD) * BIOME_CELL_BITS))
        & ((1u << BIOME_CELL_BITS) - 1u);
    var record = 0u;
    if (raw < view.tables.y) {
        record = min(tables[view.tables.z + raw], records - 1u);
    }
    let base = view.tables.w + (record * TINT_CLASSES + tint_class) * 3u;
    return vec3(
        bitcast<f32>(tables[base]),
        bitcast<f32>(tables[base + 1u]),
        bitcast<f32>(tables[base + 2u]),
    );
}

@fragment
fn lod_fragment(in: VertexOut) -> @location(0) vec4<f32> {
    let material = in.material_face.x;
    let face = in.material_face.y;
    var word = UNKNOWN_COLOUR;
    if (material == HIDDEN_MATERIAL) {
        word = HIDDEN_COLOUR;
    } else if (material < view.tables.x) {
        word = tables[material * FACES + face];
    }
    var colour = srgb_to_linear(vec3<f32>(vec3(
        field(word, COLOUR_RED_SHIFT, COLOUR_RED_MASK),
        field(word, COLOUR_GREEN_SHIFT, COLOUR_GREEN_MASK),
        field(word, COLOUR_BLUE_SHIFT, COLOUR_BLUE_MASK),
    )) / 255.0);
    let tint_class = field(word, COLOUR_TINT_SHIFT, COLOUR_TINT_MASK);
    let weight = f32(field(word, COLOUR_TINT_WEIGHT_SHIFT, COLOUR_TINT_WEIGHT_MASK))
        / f32(COLOUR_TINT_WEIGHT_MASK);
    if (tint_class != 0u && weight > 0.0) {
        let tint = biome_tint(in.light_biomes.y, in.biome_xz, tint_class);
        colour *= mix(vec3(1.0), tint, weight);
    }
    colour *= lightmap[in.light_biomes.x % LIGHTMAP_ENTRIES].rgb * view.shade[face / 4u][face % 4u];
    let distance = length(in.rel - view.camera_frac.xyz);
    let start = view.fog.w;
    let end = view.fog_end.x;
    var fog = select(0.0, 1.0, distance >= end);
    if (end > start) {
        fog = clamp((distance - start) / (end - start), 0.0, 1.0);
    }
    colour = mix(colour, view.fog.rgb, fog);
    return vec4(colour, select(1.0, view.fog_end.y, pass_info.translucent != 0u));
}
