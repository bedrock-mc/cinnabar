use std::{collections::BTreeMap, sync::Arc};

use crate::{LightChannel, SUB_CHUNK_SIDE, SubChunkKey, SubChunkLight, light::PACKED_LIGHT_BYTES};

use super::{
    cache::DensePositionSet,
    types::{
        BlockPos, BoundaryLightSample, LightBounds, LightReadAccess, LightSolveStats,
        light_axis_len, light_channel_index, light_dense_index, split_position,
    },
};

/// Generation-tagged sparse result for every intersected sub-chunk.
#[derive(Debug, Clone)]
pub struct LightSolveOutput {
    pub(super) dimension: i32,
    pub(super) bounds: LightBounds,
    pub(super) sub_chunks: BTreeMap<SubChunkKey, Arc<SubChunkLight>>,
    pub(super) direct_sky: DensePositionSet,
    pub(super) stats: LightSolveStats,
}

impl LightSolveOutput {
    #[must_use]
    pub fn light_at(&self, position: BlockPos, channel: LightChannel) -> u8 {
        self.read_light(self.dimension, position, channel)
    }

    #[must_use]
    pub const fn stats(&self) -> LightSolveStats {
        self.stats
    }

    #[must_use]
    pub const fn sub_chunks(&self) -> &BTreeMap<SubChunkKey, Arc<SubChunkLight>> {
        &self.sub_chunks
    }
}

impl LightReadAccess for LightSolveOutput {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        if dimension != self.dimension || !self.bounds.contains(position) {
            return 0;
        }
        let (key, [x, y, z]) = split_position(dimension, position);
        self.sub_chunks
            .get(&key)
            .and_then(|light| light.get(channel, x, y, z))
            .unwrap_or(0)
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        dimension == self.dimension
            && self.bounds.contains(position)
            && self.direct_sky.contains(&position)
    }

    fn boundary_light(
        &self,
        dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        if dimension != self.dimension || !self.bounds.contains(position) {
            return BoundaryLightSample::unknown();
        }
        let level = self.read_light(dimension, position, channel);
        if level == 0 {
            BoundaryLightSample::unknown()
        } else {
            BoundaryLightSample::untrusted()
        }
    }
}

#[derive(Default)]
pub(super) struct MutableOutputScratch {
    pub(super) values: Vec<[u8; 2]>,
    pub(super) known: Vec<bool>,
}

pub(super) struct MutableOutput<'a> {
    bounds: LightBounds,
    generation: u64,
    y_len: usize,
    z_len: usize,
    values: &'a mut [[u8; 2]],
    known: &'a mut [bool],
}

impl<'a> MutableOutput<'a> {
    /// Clears retained dense buffers before a new bounded solve.
    pub(super) fn new(
        bounds: LightBounds,
        generation: u64,
        volume: usize,
        scratch: &'a mut MutableOutputScratch,
    ) -> Self {
        scratch.values.clear();
        scratch.values.resize(volume, [0; 2]);
        scratch.known.clear();
        scratch.known.resize(volume, false);
        let y_len = light_axis_len(bounds.min.y, bounds.max.y);
        let z_len = light_axis_len(bounds.min.z, bounds.max.z);
        Self {
            bounds,
            generation,
            y_len,
            z_len,
            values: &mut scratch.values,
            known: &mut scratch.known,
        }
    }

    /// Maps a bounded position to the retained dense buffers.
    pub(super) fn index(&self, position: BlockPos) -> Option<usize> {
        light_dense_index(self.bounds, self.y_len, self.z_len, position)
    }

    /// Reads zero outside the current solve region.
    #[cfg(test)]
    pub(super) fn get(&self, position: BlockPos, channel: LightChannel) -> u8 {
        let Some(index) = self.index(position) else {
            return 0;
        };
        self.get_at_index(index, channel)
    }

    /// Reads a cell whose dense index was validated for this solve.
    #[inline]
    pub(super) fn get_at_index(&self, index: usize, channel: LightChannel) -> u8 {
        self.values[index][light_channel_index(channel)]
    }

    /// Writes a validated cell and records that it belongs in the final output.
    #[inline]
    pub(super) fn set_at_index(&mut self, index: usize, channel: LightChannel, value: u8) {
        self.values[index][light_channel_index(channel)] = value;
        self.known[index] = true;
    }

    /// Copies completed light into independently owned sub-chunk results.
    pub(super) fn freeze(
        self,
        direct_sky: DensePositionSet,
        stats: LightSolveStats,
    ) -> LightSolveOutput {
        let mut sub_chunks = BTreeMap::new();
        let (min, _) = split_position(self.bounds.dimension, self.bounds.min);
        let (max, _) = split_position(self.bounds.dimension, self.bounds.max);
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    let key = SubChunkKey::new(self.bounds.dimension, x, y, z);
                    if let Some(light) = self.pack_section(key) {
                        sub_chunks.insert(key, Arc::new(light));
                    }
                }
            }
        }
        LightSolveOutput {
            dimension: self.bounds.dimension,
            bounds: self.bounds,
            sub_chunks,
            direct_sky,
            stats,
        }
    }

    fn pack_section(&self, key: SubChunkKey) -> Option<SubChunkLight> {
        let side = SUB_CHUNK_SIDE as i32;
        let extent = side - 1;
        let origin = BlockPos::new(key.x * side, key.y * side, key.z * side);
        let min = BlockPos::new(
            self.bounds.min.x.max(origin.x),
            self.bounds.min.y.max(origin.y),
            self.bounds.min.z.max(origin.z),
        );
        let max = BlockPos::new(
            self.bounds.max.x.min(origin.x + extent),
            self.bounds.max.y.min(origin.y + extent),
            self.bounds.max.z.min(origin.z + extent),
        );
        let mut block = [0; PACKED_LIGHT_BYTES];
        let mut sky = [0; PACKED_LIGHT_BYTES];
        let mut any_known = false;
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                let start = self
                    .index(BlockPos::new(x, y, min.z))
                    .expect("clipped section rows stay inside the solve bounds");
                let local_y = (y - origin.y) as usize;
                let row = ((x - origin.x) as usize) << 7 | (local_y >> 1);
                let shift = (local_y & 1) * 4;
                for (offset, z) in (min.z..=max.z).enumerate() {
                    let index = start + offset;
                    if !self.known[index] {
                        continue;
                    }
                    any_known = true;
                    let packed_index = row | (((z - origin.z) as usize) << 3);
                    let [block_value, sky_value] = self.values[index];
                    block[packed_index] |= block_value << shift;
                    sky[packed_index] |= sky_value << shift;
                }
            }
        }
        any_known.then(|| SubChunkLight::from_packed(block, sky, self.generation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_packing_matches_scalar_writes_for_partial_bounds_and_unknowns() {
        let cases = [
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 15, 15)),
            (BlockPos::new(-17, -3, -19), BlockPos::new(18, 20, 1)),
            (
                BlockPos::new(i32::MIN, i32::MIN, i32::MIN),
                BlockPos::new(i32::MIN + 2, i32::MIN + 3, i32::MIN + 4),
            ),
            (
                BlockPos::new(i32::MAX - 2, i32::MAX - 3, i32::MAX - 4),
                BlockPos::new(i32::MAX, i32::MAX, i32::MAX),
            ),
        ];
        for (min, max) in cases {
            for pattern in 0..4 {
                let bounds = LightBounds::new(-3, min, max).unwrap();
                let volume = bounds.volume().unwrap();
                let mut scratch = MutableOutputScratch::default();
                let output = MutableOutput::new(bounds, 137, volume, &mut scratch);
                let mut expected = BTreeMap::new();
                for (index, position) in bounds.positions().enumerate() {
                    let known = match pattern {
                        0 | 3 => true,
                        1 => index % 5 != 0 && position.x.div_euclid(16) % 2 == 0,
                        _ => false,
                    };
                    let values = if pattern == 3 {
                        [0; 2]
                    } else {
                        [((index * 7 + 3) & 15) as u8, ((index / 3 + 5) & 15) as u8]
                    };
                    output.values[index] = values;
                    output.known[index] = known;
                    if !known {
                        continue;
                    }
                    let (key, [x, y, z]) = split_position(bounds.dimension, position);
                    let light = expected
                        .entry(key)
                        .or_insert_with(|| SubChunkLight::dark(137));
                    for (channel, value) in [
                        (LightChannel::Block, values[0]),
                        (LightChannel::Sky, values[1]),
                    ] {
                        light.set(channel, x, y, z, value).unwrap();
                    }
                }
                let frozen = output.freeze(
                    DensePositionSet::new(bounds, volume),
                    LightSolveStats::default(),
                );
                assert_eq!(frozen.sub_chunks.len(), expected.len());
                for (key, light) in expected {
                    assert_eq!(frozen.sub_chunks[&key].as_ref(), &light);
                }
            }
        }
    }
}
