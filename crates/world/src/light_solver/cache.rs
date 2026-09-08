use std::cell::Cell;

use crate::LightChannel;

use super::{
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
            .is_some_and(|index| self.present[index])
    }

    pub(super) fn insert(&mut self, position: BlockPos) {
        let index = light_dense_index(self.bounds, self.y_len, self.z_len, position)
            .expect("solver provenance stays inside validated light bounds");
        self.present[index] = true;
    }
}

pub(super) struct CachedLightBlockAccess<'a, A> {
    source: &'a A,
    bounds: LightBounds,
    y_len: usize,
    z_len: usize,
    samples: Box<[LightBlockSample]>,
    sky_seeds: Box<[u8]>,
}

impl<'a, A: LightBlockAccess> CachedLightBlockAccess<'a, A> {
    pub(super) fn new(source: &'a A, bounds: LightBounds, volume: usize) -> Self {
        let y_len = light_axis_len(bounds.min.y, bounds.max.y);
        let z_len = light_axis_len(bounds.min.z, bounds.max.z);
        let mut samples = Vec::with_capacity(volume);
        let mut sky_seeds = Vec::with_capacity(volume);
        for position in bounds.positions() {
            samples.push(source.sample(position));
            sky_seeds.push(source.sky_seed(position));
        }
        Self {
            source,
            bounds,
            y_len,
            z_len,
            samples: samples.into_boxed_slice(),
            sky_seeds: sky_seeds.into_boxed_slice(),
        }
    }

    fn index(&self, position: BlockPos) -> Option<usize> {
        light_dense_index(self.bounds, self.y_len, self.z_len, position)
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

pub(super) struct CachedLightReadAccess<'a, P> {
    source: &'a P,
    bounds: LightBounds,
    y_len: usize,
    z_len: usize,
    light: Box<[[Cell<Option<u8>>; 2]]>,
    direct_sky: Box<[Cell<Option<bool>>]>,
}

impl<'a, P: LightReadAccess> CachedLightReadAccess<'a, P> {
    pub(super) fn new(source: &'a P, bounds: LightBounds, volume: usize) -> Self {
        Self {
            source,
            bounds,
            y_len: light_axis_len(bounds.min.y, bounds.max.y),
            z_len: light_axis_len(bounds.min.z, bounds.max.z),
            light: (0..volume)
                .map(|_| [Cell::new(None), Cell::new(None)])
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            direct_sky: (0..volume)
                .map(|_| Cell::new(None))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }

    fn index(&self, position: BlockPos) -> Option<usize> {
        light_dense_index(self.bounds, self.y_len, self.z_len, position)
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
        let cached = &self.light[index][light_channel_index(channel)];
        if let Some(value) = cached.get() {
            return value;
        }
        let value = self.source.read_light(dimension, position, channel);
        cached.set(Some(value));
        value
    }

    fn has_direct_sky_provenance(&self, dimension: i32, position: BlockPos) -> bool {
        if dimension != self.bounds.dimension {
            return self.source.has_direct_sky_provenance(dimension, position);
        }
        let Some(index) = self.index(position) else {
            return self.source.has_direct_sky_provenance(dimension, position);
        };
        let cached = &self.direct_sky[index];
        if let Some(direct_sky) = cached.get() {
            return direct_sky;
        }
        let direct_sky = self.source.has_direct_sky_provenance(dimension, position);
        cached.set(Some(direct_sky));
        direct_sky
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
        let cache = CachedLightReadAccess::new(&source, bounds, 1);

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
