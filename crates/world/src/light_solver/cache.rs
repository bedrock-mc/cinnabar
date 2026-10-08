use std::cell::Cell;

use crate::LightChannel;

use super::types::{
    BlockPos, BoundaryLightSample, LightBlockAccess, LightBlockSample, LightBounds,
    LightReadAccess, light_axis_len, light_channel_index, light_dense_index,
};

#[derive(Debug, Clone)]
pub(super) struct DensePositionSet {
    bounds: LightBounds,
    y_len: usize,
    z_len: usize,
    present: Box<[bool]>,
}

impl DensePositionSet {
    pub(super) fn new(bounds: LightBounds, volume: usize) -> Self {
        Self {
            bounds,
            y_len: light_axis_len(bounds.min.y, bounds.max.y),
            z_len: light_axis_len(bounds.min.z, bounds.max.z),
            present: vec![false; volume].into_boxed_slice(),
        }
    }

    pub(super) fn contains(&self, position: &BlockPos) -> bool {
        light_dense_index(self.bounds, self.y_len, self.z_len, *position)
            .is_some_and(|index| self.contains_at_index(index))
    }

    pub(super) fn insert(&mut self, position: BlockPos) {
        let index = light_dense_index(self.bounds, self.y_len, self.z_len, position)
            .expect("solver provenance stays inside validated light bounds");
        self.insert_at_index(index);
    }

    /// Reads provenance for an index validated against the current solve bounds.
    #[inline]
    pub(super) fn contains_at_index(&self, index: usize) -> bool {
        self.present[index]
    }

    /// Records provenance without remapping a validated cell coordinate.
    #[inline]
    pub(super) fn insert_at_index(&mut self, index: usize) {
        self.present[index] = true;
    }
}

#[derive(Default)]
pub(super) struct BlockCacheScratch {
    pub(super) samples: Vec<LightBlockSample>,
    pub(super) sky_seeds: Vec<u8>,
}

pub(super) struct CachedLightBlockAccess<'a, A> {
    source: &'a A,
    bounds: LightBounds,
    y_len: usize,
    z_len: usize,
    samples: &'a [LightBlockSample],
    sky_seeds: &'a [u8],
}

impl<'a, A: LightBlockAccess> CachedLightBlockAccess<'a, A> {
    /// Refreshes the bounded block cache without retaining the previous input source.
    pub(super) fn new(
        source: &'a A,
        bounds: LightBounds,
        volume: usize,
        scratch: &'a mut BlockCacheScratch,
    ) -> Self {
        let y_len = light_axis_len(bounds.min.y, bounds.max.y);
        let z_len = light_axis_len(bounds.min.z, bounds.max.z);
        scratch.samples.clear();
        scratch.samples.reserve(volume);
        scratch.sky_seeds.clear();
        scratch.sky_seeds.reserve(volume);
        let samples = &mut scratch.samples;
        let sky_seeds = &mut scratch.sky_seeds;
        for position in bounds.positions() {
            samples.push(source.sample(position));
            sky_seeds.push(source.sky_seed(position));
        }
        Self {
            source,
            bounds,
            y_len,
            z_len,
            samples,
            sky_seeds,
        }
    }

    /// Maps only the current bounded cache region to a dense slot.
    pub(super) fn index(&self, position: BlockPos) -> Option<usize> {
        light_dense_index(self.bounds, self.y_len, self.z_len, position)
    }

    /// Reads block properties for a dense index validated by this solve.
    #[inline]
    pub(super) fn sample_at_index(&self, index: usize) -> LightBlockSample {
        self.samples[index]
    }

    /// Reads the raw sky seed so the solver can retain its validation order.
    #[inline]
    pub(super) fn sky_seed_at_index(&self, index: usize) -> u8 {
        self.sky_seeds[index]
    }
}

impl<A: LightBlockAccess> LightBlockAccess for CachedLightBlockAccess<'_, A> {
    fn sample(&self, position: BlockPos) -> LightBlockSample {
        self.index(position)
            .map_or_else(|| self.source.sample(position), |index| self.samples[index])
    }

    fn sky_seed(&self, position: BlockPos) -> u8 {
        self.index(position).map_or_else(
            || self.source.sky_seed(position),
            |index| self.sky_seeds[index],
        )
    }
}

#[derive(Default)]
pub(super) struct PriorCacheScratch {
    pub(super) light: Vec<[Cell<Option<u8>>; 2]>,
    pub(super) direct_sky: Vec<Cell<Option<bool>>>,
}

pub(super) struct CachedLightReadAccess<'a, P> {
    source: &'a P,
    bounds: LightBounds,
    y_len: usize,
    z_len: usize,
    light: &'a [[Cell<Option<u8>>; 2]],
    direct_sky: &'a [Cell<Option<bool>>],
}

impl<'a, P: LightReadAccess> CachedLightReadAccess<'a, P> {
    /// Invalidates all lazy prior values while retaining their allocation.
    pub(super) fn new(
        source: &'a P,
        bounds: LightBounds,
        volume: usize,
        scratch: &'a mut PriorCacheScratch,
    ) -> Self {
        scratch.light.clear();
        scratch
            .light
            .resize(volume, [Cell::new(None), Cell::new(None)]);
        scratch.direct_sky.clear();
        scratch.direct_sky.resize(volume, Cell::new(None));
        Self {
            source,
            bounds,
            y_len: light_axis_len(bounds.min.y, bounds.max.y),
            z_len: light_axis_len(bounds.min.z, bounds.max.z),
            light: &scratch.light,
            direct_sky: &scratch.direct_sky,
        }
    }

    /// Maps only the current bounded cache region to a dense slot.
    fn index(&self, position: BlockPos) -> Option<usize> {
        light_dense_index(self.bounds, self.y_len, self.z_len, position)
    }

    /// Lazily reads a validated interior cell, leaving nibble validation to the solver.
    #[inline]
    pub(super) fn read_at_index(
        &self,
        index: usize,
        position: BlockPos,
        channel: LightChannel,
    ) -> u8 {
        let cached = &self.light[index][light_channel_index(channel)];
        if let Some(value) = cached.get() {
            return value;
        }
        let value = self
            .source
            .read_light(self.bounds.dimension, position, channel);
        cached.set(Some(value));
        value
    }

    /// Loads interior provenance once without repeating the cell's coordinate conversion.
    #[inline]
    pub(super) fn direct_sky_at_index(&self, index: usize, position: BlockPos) -> bool {
        let cached = &self.direct_sky[index];
        if let Some(direct_sky) = cached.get() {
            return direct_sky;
        }
        let direct_sky = self
            .source
            .has_direct_sky_provenance(self.bounds.dimension, position);
        cached.set(Some(direct_sky));
        direct_sky
    }
}

impl<P: LightReadAccess> LightReadAccess for CachedLightReadAccess<'_, P> {
    fn read_light(&self, dimension: i32, position: BlockPos, channel: LightChannel) -> u8 {
        if dimension != self.bounds.dimension {
            return self.source.read_light(dimension, position, channel);
        }
        let Some(index) = self.index(position) else {
            return self.source.read_light(dimension, position, channel);
        };
        self.read_at_index(index, position, channel)
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        if dimension != self.bounds.dimension {
            return self.source.has_direct_sky_provenance(dimension, position);
        }
        let Some(index) = self.index(position) else {
            return self.source.has_direct_sky_provenance(dimension, position);
        };
        self.direct_sky_at_index(index, position)
    }

    fn boundary_light(
        &self,
        dimension: i32,
        position: BlockPos,
        channel: LightChannel,
    ) -> BoundaryLightSample {
        self.source.boundary_light(dimension, position, channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingPrior {
        light_reads: Cell<usize>,
        provenance_reads: Cell<usize>,
        boundary_reads: Cell<usize>,
    }

    impl LightReadAccess for RecordingPrior {
        fn read_light(&self, _dimension: i32, _position: BlockPos, _channel: LightChannel) -> u8 {
            self.light_reads.set(self.light_reads.get() + 1);
            17
        }

        fn has_direct_sky_provenance(&self, _dimension: i32, _position: BlockPos) -> bool {
            self.provenance_reads.set(self.provenance_reads.get() + 1);
            true
        }

        fn boundary_light(
            &self,
            _dimension: i32,
            _position: BlockPos,
            _channel: LightChannel,
        ) -> BoundaryLightSample {
            self.boundary_reads.set(self.boundary_reads.get() + 1);
            BoundaryLightSample::trusted(9, true).unwrap()
        }
    }

    #[test]
    fn prior_cache_is_raw_inside_and_forwards_every_outside_or_boundary_read() {
        let source = RecordingPrior::default();
        let position = BlockPos::new(4, 5, 6);
        let bounds = LightBounds::new(3, position, position).unwrap();
        let mut scratch = PriorCacheScratch::default();
        let cache = CachedLightReadAccess::new(&source, bounds, 1, &mut scratch);

        assert_eq!(cache.read_light(3, position, LightChannel::Sky), 17);
        assert_eq!(cache.read_light(3, position, LightChannel::Sky), 17);
        assert_eq!(source.light_reads.get(), 1);
        assert!(cache.has_direct_sky_provenance(3, position));
        assert!(cache.has_direct_sky_provenance(3, position));
        assert_eq!(source.provenance_reads.get(), 1);

        let outside = BlockPos::new(5, 5, 6);
        for (dimension, read_position) in [(4, position), (3, outside)] {
            assert_eq!(
                cache.read_light(dimension, read_position, LightChannel::Sky),
                17
            );
            assert!(cache.has_direct_sky_provenance(dimension, read_position));
        }
        assert_eq!(source.light_reads.get(), 3);
        assert_eq!(source.provenance_reads.get(), 3);

        let expected = BoundaryLightSample::trusted(9, true).unwrap();
        assert_eq!(
            cache.boundary_light(3, outside, LightChannel::Sky),
            expected
        );
        assert_eq!(
            cache.boundary_light(3, outside, LightChannel::Sky),
            expected
        );
        assert_eq!(source.boundary_reads.get(), 2);
    }
}
