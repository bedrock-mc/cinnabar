use std::{collections::BTreeMap, sync::Arc};

use crate::{LightChannel, SubChunkKey, SubChunkLight};

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
        let mut sub_chunks = BTreeMap::<SubChunkKey, SubChunkLight>::new();
        for position in self.bounds.positions() {
            let index = self
                .index(position)
                .expect("bounded positions always have a dense light index");
            if !self.known[index] {
                continue;
            }
            let (key, [x, y, z]) = split_position(self.bounds.dimension, position);
            let light = sub_chunks
                .entry(key)
                .or_insert_with(|| SubChunkLight::dark(self.generation));
            for channel in [LightChannel::Block, LightChannel::Sky] {
                light
                    .set_deferred(
                        channel,
                        x,
                        y,
                        z,
                        self.values[index][light_channel_index(channel)],
                    )
                    .expect("solver only emits validated nibble values");
            }
        }
        for light in sub_chunks.values_mut() {
            light.canonicalize();
        }
        LightSolveOutput {
            dimension: self.bounds.dimension,
            bounds: self.bounds,
            sub_chunks: sub_chunks
                .into_iter()
                .map(|(key, light)| (key, Arc::new(light)))
                .collect(),
            direct_sky,
            stats,
        }
    }
}
