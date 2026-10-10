use super::biome::{BIOME_NEIGHBOUR_SLOT_COUNT, BiomeBlendSample, PackedBiomeRecord};

/// Vanilla lattice spacing and equal-weight sample radius, in blocks.
pub const BIOME_LATTICE_STEP: i32 = 4;
pub const BIOME_CACHE_ORIGIN: i32 = 8;
pub const BIOME_RESIDUE_RADIUS: i32 = BIOME_LATTICE_STEP - 1;
pub const BIOME_RESIDUE_SIDE: i32 = 2 * BIOME_RESIDUE_RADIUS + 1;
pub const BIOME_BLEND_RADIUS: i32 = BIOME_LATTICE_STEP;
pub const BIOME_DISTANCE_EPSILON: f32 = f32::EPSILON;
pub const LATTICE_SIDE: usize = 7;
pub const LATTICE_BIOME_LIMIT: usize = 4;
pub const LATTICE_QUERY_POINTS: usize = 8;
pub const LATTICE_STENCIL_SAMPLES: usize = 27;
pub const LATTICE_POINT_WORDS: usize = 1 + 2 * LATTICE_BIOME_LIMIT;
pub const LATTICE_WORDS: usize = LATTICE_SIDE * LATTICE_SIDE * LATTICE_SIDE * LATTICE_POINT_WORDS;
pub const DESCRIPTOR_WORDS: usize = 2 + BIOME_NEIGHBOUR_SLOT_COUNT;
pub const BLEND_SAMPLE_COUNT: usize = LATTICE_QUERY_POINTS * LATTICE_BIOME_LIMIT;

/// A record answers block positions inside its own sub-chunk, on every axis.
pub const BIOME_QUERY_SIDE: i32 = crate::SIDE as i32;

/// Maps a local lattice point into the packed cache.
pub fn lattice_index([x, y, z]: [i32; 3]) -> usize {
    let axis = |value| ((value + BIOME_LATTICE_STEP) / BIOME_LATTICE_STEP) as usize;
    (axis(x) * LATTICE_SIDE + axis(y)) * LATTICE_SIDE + axis(z)
}

/// Selects the nearest eight points, preserving vanilla's X/Y/Z tie order.
pub fn nearest_lattice_points(coordinate: [i32; 3]) -> [([i32; 3], f32); LATTICE_QUERY_POINTS] {
    let base = coordinate.map(|v| {
        (v - BIOME_CACHE_ORIGIN) / BIOME_LATTICE_STEP * BIOME_LATTICE_STEP + BIOME_CACHE_ORIGIN
    });
    let mut points = [([0; 3], f32::INFINITY); LATTICE_STENCIL_SAMPLES];
    let mut index = 0;
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let position = [
                    base[0] + dx * BIOME_LATTICE_STEP,
                    base[1] + dy * BIOME_LATTICE_STEP,
                    base[2] + dz * BIOME_LATTICE_STEP,
                ];
                let distance = (0..3)
                    .map(|i| (coordinate[i] - position[i]).pow(2))
                    .sum::<i32>() as f32;
                points[index] = (position, distance);
                index += 1;
            }
        }
    }
    points.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut selected = std::array::from_fn(|i| {
        (
            points[i].0,
            1.0 / (points[i].1.sqrt() + BIOME_DISTANCE_EPSILON),
        )
    });
    let total: f32 = selected.iter().map(|p| p.1).sum();
    for point in &mut selected {
        point.1 /= total;
    }
    selected
}

/// Shader query kernel tables; `query_table_words` packs them for the renderer's uniform block.
pub(crate) struct QueryTables {
    /// Per residue, the nearest points' 3x3x3 stencil indices in blend order, one byte each.
    pub(crate) stencils: Vec<[u32; 2]>,
    /// Blend weights, keyed by the residue's per-axis magnitude; reflecting an axis keeps them.
    pub(crate) weights: Vec<[f32; LATTICE_QUERY_POINTS]>,
}

const _: () = assert!(
    LATTICE_QUERY_POINTS == 8,
    "stencils pack four bytes per word"
);
const MAGNITUDE_SIDE: usize = BIOME_RESIDUE_RADIUS as usize + 1;

/// `vec4` slots of each table in the shader's uniform block, in `query_table_words` order.
pub const BIOME_STENCIL_VEC4S: usize = (BIOME_RESIDUE_SIDE as usize).pow(3).div_ceil(2);
pub const BIOME_WEIGHT_VEC4S: usize = MAGNITUDE_SIDE.pow(3) * LATTICE_QUERY_POINTS / 4;
pub const GRASS_PERMUTATION_VEC4S: usize = assets::GRASS_PERMUTATION_SIZE / 4;
pub const BIOME_QUERY_TABLE_WORDS: usize =
    (BIOME_STENCIL_VEC4S + BIOME_WEIGHT_VEC4S + GRASS_PERMUTATION_VEC4S) * 4;

/// The query kernel's lookup tables packed for a uniform buffer: stencils two per `vec4<u32>`,
/// weights as `vec4<f32>` bit patterns, then the grass noise permutation four per `vec4<u32>`.
/// Shader `const` arrays indexed dynamically are copied into per-invocation scratch memory by
/// several compilers, which made every tinted fragment copy about 7 KB before sampling.
pub fn query_table_words() -> Vec<u32> {
    let tables = query_tables();
    let mut words = Vec::with_capacity(BIOME_QUERY_TABLE_WORDS);
    words.extend(tables.stencils.iter().flatten());
    words.resize(BIOME_STENCIL_VEC4S * 4, 0);
    words.extend(
        tables
            .weights
            .iter()
            .flatten()
            .map(|weight| weight.to_bits()),
    );
    words.extend(assets::grass_noise_permutation());
    assert_eq!(words.len(), BIOME_QUERY_TABLE_WORDS);
    words
}

pub(crate) fn query_tables() -> QueryTables {
    let mut stencils = Vec::new();
    let mut weights: Vec<Option<[f32; LATTICE_QUERY_POINTS]>> =
        vec![None; MAGNITUDE_SIDE * MAGNITUDE_SIDE * MAGNITUDE_SIDE];
    let radius = -BIOME_RESIDUE_RADIUS..=BIOME_RESIDUE_RADIUS;
    for x in radius.clone() {
        for y in radius.clone() {
            for z in radius.clone() {
                let points = nearest_lattice_points([x, y, z].map(|v| v + BIOME_CACHE_ORIGIN));
                let mut words = [0; 2];
                for (point, (position, _)) in points.iter().enumerate() {
                    let [dx, dy, dz] = position
                        .map(|v| ((v - BIOME_CACHE_ORIGIN) / BIOME_LATTICE_STEP + 1) as u32);
                    words[point / 4] |= (dx * 9 + dy * 3 + dz) << (point % 4 * 8);
                }
                stencils.push(words);
                let [mx, my, mz] = [x, y, z].map(|v| v.unsigned_abs() as usize);
                let slot = &mut weights[(mx * MAGNITUDE_SIDE + my) * MAGNITUDE_SIDE + mz];
                let these = points.map(|(_, weight)| weight);
                assert!(
                    slot.is_none_or(|known| known.map(f32::to_bits) == these.map(f32::to_bits)),
                    "blend weights must not depend on residue sign",
                );
                *slot = Some(these);
            }
        }
    }
    QueryTables {
        stencils,
        weights: weights
            .into_iter()
            .map(|w| w.expect("every magnitude occurs"))
            .collect(),
    }
}

impl PackedBiomeRecord {
    /// Counts 27 biomes at each lattice point and retains the four most frequent.
    pub(crate) fn build_lattice(&mut self) {
        let mut cache = vec![0; LATTICE_WORDS];
        let side = LATTICE_SIDE + 2;
        let grid = (0..side * side * side)
            .map(|index| {
                let position = [index / (side * side), index / side % side, index % side]
                    .map(|v| (v as i32 - 2) * BIOME_LATTICE_STEP);
                self.tint_index_at(position).unwrap_or(0)
            })
            .collect::<Vec<_>>();
        for x in 0..LATTICE_SIDE {
            for y in 0..LATTICE_SIDE {
                for z in 0..LATTICE_SIDE {
                    let position = [x, y, z].map(|v| (v as i32 - 1) * BIOME_LATTICE_STEP);
                    let mut counts = [(0_u32, 0_u32); LATTICE_STENCIL_SAMPLES];
                    let mut count_len = 0;
                    for dz in 0..3 {
                        for dy in 0..3 {
                            for dx in 0..3 {
                                let tint = grid[((x + dx) * side + y + dy) * side + z + dz];
                                if let Some(entry) =
                                    counts[..count_len].iter_mut().find(|entry| entry.0 == tint)
                                {
                                    entry.1 += 1;
                                } else {
                                    counts[count_len] = (tint, 1);
                                    count_len += 1;
                                }
                            }
                        }
                    }
                    counts[..count_len].sort_by(|a, b| b.1.cmp(&a.1));
                    let start = lattice_index(position) * LATTICE_POINT_WORDS;
                    cache[start] = count_len.min(LATTICE_BIOME_LIMIT) as u32;
                    for (i, &(tint, count)) in counts[..count_len]
                        .iter()
                        .take(LATTICE_BIOME_LIMIT)
                        .enumerate()
                    {
                        cache[start + 1 + i] = tint;
                        cache[start + 1 + LATTICE_BIOME_LIMIT + i] =
                            (count as f32 / LATTICE_STENCIL_SAMPLES as f32).to_bits();
                    }
                }
            }
        }
        std::sync::Arc::make_mut(&mut self.words)
            [DESCRIPTOR_WORDS..DESCRIPTOR_WORDS + LATTICE_WORDS]
            .copy_from_slice(&cache);
    }

    /// CPU mirror of vanilla's block-position colour query, including 3D weights.
    pub fn blend_samples(
        &self,
        coordinate: [i32; 3],
    ) -> Option<[BiomeBlendSample; BLEND_SAMPLE_COUNT]> {
        if coordinate
            .iter()
            .any(|&v| !(0..BIOME_QUERY_SIDE).contains(&v))
        {
            return None;
        }
        let mut samples = [BiomeBlendSample {
            tint_index: 0,
            weight: 0.0,
        }; BLEND_SAMPLE_COUNT];
        if let Some(tint_index) = self.uniform_tint_index() {
            samples[0] = BiomeBlendSample {
                tint_index,
                weight: 1.0,
            };
            return Some(samples);
        }
        for (point, (position, weight)) in
            nearest_lattice_points(coordinate).into_iter().enumerate()
        {
            let start = DESCRIPTOR_WORDS + lattice_index(position) * LATTICE_POINT_WORDS;
            for i in 0..self.words[start] as usize {
                samples[point * LATTICE_BIOME_LIMIT + i] = BiomeBlendSample {
                    tint_index: self.words[start + 1 + i],
                    weight: weight
                        * f32::from_bits(self.words[start + 1 + LATTICE_BIOME_LIMIT + i]),
                };
            }
        }
        Some(samples)
    }
}

/// Supplies the shader layout and kernel constants from the CPU contract.
pub fn shader_source(source: &str) -> String {
    let mut constants = format!(
        "const BIOME_DESCRIPTOR_MAGIC: u32 = {}u;\nconst BIOME_LATTICE_STEP: i32 = {};\nconst BIOME_CACHE_ORIGIN: i32 = {};\nconst BIOME_RESIDUE_RADIUS: i32 = {};\nconst BIOME_RESIDUE_SIDE: u32 = {}u;\nconst BIOME_DESCRIPTOR_WORDS: u32 = {}u;\nconst BIOME_LATTICE_SIDE: u32 = {}u;\nconst BIOME_POINT_WORDS: u32 = {}u;\nconst BIOME_DISTANCE_EPSILON: f32 = {};\nconst BIOME_BIOME_LIMIT: u32 = {}u;\nconst BIOME_QUERY_POINTS: u32 = {}u;\nconst BIOME_QUERY_SIDE: i32 = {};\n",
        super::biome::DESCRIPTOR_MAGIC,
        BIOME_LATTICE_STEP,
        BIOME_CACHE_ORIGIN,
        BIOME_RESIDUE_RADIUS,
        BIOME_RESIDUE_SIDE,
        DESCRIPTOR_WORDS,
        LATTICE_SIDE,
        LATTICE_POINT_WORDS,
        BIOME_DISTANCE_EPSILON,
        LATTICE_BIOME_LIMIT,
        LATTICE_QUERY_POINTS,
        BIOME_QUERY_SIDE,
    );
    constants.push_str(&format!(
        "const BIOME_STENCIL_VEC4S: u32 = {}u;\nconst BIOME_WEIGHT_VEC4S: u32 = {}u;\nconst GRASS_PERMUTATION_VEC4S: u32 = {}u;\n",
        BIOME_STENCIL_VEC4S, BIOME_WEIGHT_VEC4S, GRASS_PERMUTATION_VEC4S,
    ));
    constants.push_str(&format!(
        "const SEASONAL_FOLIAGE_COUNT: u32 = {}u;\nconst SEASONAL_FOLIAGE_EXPOSED_OFFSET: u32 = {}u;\nconst BIOME_SEASONAL_FOLIAGE: u32 = {}u;\nconst MATERIAL_SEASONAL_FOLIAGE: u32 = {}u;\nconst MATERIAL_EXPOSED_FOLIAGE: u32 = {}u;\n",
        assets::SEASONAL_FOLIAGE_COUNT,
        assets::SEASONAL_FOLIAGE_EXPOSED_OFFSET,
        assets::BIOME_TINT_FLAG_SEASONAL_FOLIAGE,
        assets::MATERIAL_FLAG_SEASONAL_FOLIAGE,
        assets::MATERIAL_FLAG_EXPOSED_FOLIAGE,
    ));
    constants.push_str(&format!(
        "const SEASONAL_EVERGREEN_CELL: u32 = {}u;\nconst SEASONAL_BIRCH_CELL: u32 = {}u;\nconst SEASONAL_DEFAULT_CELL: u32 = {}u;\n",
        assets::seasonal_foliage_palette_index(assets::MATERIAL_FLAG_EVERGREEN_FOLIAGE, false),
        assets::seasonal_foliage_palette_index(assets::MATERIAL_FLAG_BIRCH_FOLIAGE, false),
        assets::seasonal_foliage_palette_index(0, false),
    ));
    constants.push_str(&format!(
        "const BIOME_TINT_MAP_SIZE: u32 = {}u;\nconst BIOME_SWAMP_GRASS: u32 = {}u;\nconst GRASS_PERMUTATION_MASK: u32 = {}u;\n",
        assets::TINT_MAP_SIZE,
        assets::BIOME_TINT_FLAG_SWAMP_GRASS,
        assets::GRASS_PERMUTATION_SIZE - 1,
    ));
    source.replace("// BIOME_CONSTANTS", &constants)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shader's compact query tables must reproduce the CPU kernel's points, order and bits.
    /// The shader reads each table at a fixed `vec4` offset, so the packing must not drift.
    #[test]
    fn query_table_words_pack_each_table_at_its_vec4_offset() {
        let tables = query_tables();
        let words = query_table_words();
        assert_eq!(words.len(), BIOME_QUERY_TABLE_WORDS);
        assert_eq!(words.len() % 4, 0);
        for (index, [low, high]) in tables.stencils.iter().enumerate() {
            assert_eq!(words[index * 2], *low, "stencil {index} low word");
            assert_eq!(words[index * 2 + 1], *high, "stencil {index} high word");
        }
        let weights = BIOME_STENCIL_VEC4S * 4;
        for (index, weight) in tables.weights.iter().flatten().enumerate() {
            assert_eq!(words[weights + index], weight.to_bits(), "weight {index}");
        }
        let permutation = weights + BIOME_WEIGHT_VEC4S * 4;
        assert_eq!(
            &words[permutation..],
            &assets::grass_noise_permutation()[..],
            "permutation follows the weights"
        );
    }

    #[test]
    fn query_tables_decode_to_the_exact_cpu_kernel() {
        let tables = query_tables();
        let side = BIOME_RESIDUE_SIDE as usize;
        for (query, words) in tables.stencils.iter().enumerate() {
            let residue = [query / (side * side), query / side % side, query % side]
                .map(|v| v as i32 - BIOME_RESIDUE_RADIUS);
            let expected = nearest_lattice_points(residue.map(|v| v + BIOME_CACHE_ORIGIN));
            let [mx, my, mz] = residue.map(|v| v.unsigned_abs() as usize);
            let weights = tables.weights[(mx * MAGNITUDE_SIDE + my) * MAGNITUDE_SIDE + mz];
            for (point, (position, weight)) in expected.into_iter().enumerate() {
                let cell = (words[point / 4] >> (point % 4 * 8)) & 0xff;
                let offset = [cell / 9, cell / 3 % 3, cell % 3]
                    .map(|v| (v as i32 - 1) * BIOME_LATTICE_STEP + BIOME_CACHE_ORIGIN);
                assert_eq!(offset, position, "residue {residue:?} point {point}");
                assert_eq!(weights[point].to_bits(), weight.to_bits());
            }
        }
    }
}
