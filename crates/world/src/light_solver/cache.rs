use super::{
    BlockPos, LightBlockAccess, LightBlockSample, LightBounds, light_axis_len, light_dense_index,
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
