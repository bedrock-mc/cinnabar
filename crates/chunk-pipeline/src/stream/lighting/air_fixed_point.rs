use super::super::*;

impl WorldStream {
    /// Air arrivals preserve maximal direct sky and zero block light; positive source output still propagates.
    pub(in crate::stream) fn air_fixed_point_survives_sources(
        &self,
        target: SubChunkKey,
        sources: &BTreeSet<SubChunkKey>,
    ) -> bool {
        if sources.contains(&target)
            || target.dimension != 0
            || self.lighting.jobs.pending.contains_key(&target)
            || self.lighting.jobs.in_flight.contains_key(&target)
            || !self.light_is_current(target)
            || !self.light_source_is_air(target)
        {
            return false;
        }
        let Some(light) = self.lighting.store.light(target) else {
            return false;
        };
        let Some(direct) = self.lighting.direct_sky.get(&target) else {
            return false;
        };
        if !is_uniform_direct_sky(light, direct.mask.as_ref()) {
            return false;
        }
        target
            .mesh_dependents()
            .filter(|source| sources.contains(source))
            .all(|source| self.air_source_has_no_retained_block_light(source))
    }

    fn light_source_is_air(&self, key: SubChunkKey) -> bool {
        self.known_air.contains(&key)
            || self
                .authority
                .terrain()
                .sub_chunk(key)
                .is_some_and(|sub_chunk| self.classifier.is_sub_chunk_air(&sub_chunk))
    }

    fn air_source_has_no_retained_block_light(&self, key: SubChunkKey) -> bool {
        if !self.light_source_is_known(key)
            || !self.light_source_is_air(key)
            || self.lighting.jobs.pending.contains_key(&key)
            || self.lighting.jobs.in_flight.contains_key(&key)
        {
            return false;
        }
        let Some(light) = self.lighting.store.light(key) else {
            return false;
        };
        if !light.channel(LightChannel::Block).is_uniform()
            || !light.channel(LightChannel::Sky).is_uniform()
            || light.get(LightChannel::Block, 0, 0, 0) != Some(0)
        {
            return false;
        }
        match (
            self.lighting.ownership.get(&key),
            self.lighting.direct_sky.get(&key),
        ) {
            (Some(ownership), Some(direct)) => {
                ownership.light_revision == light.generation()
                    && direct.light_revision == light.generation()
                    && matches!(direct.mask.as_ref(), DirectSkyMask::Uniform(_))
            }
            (None, None) => light.generation() == 0,
            _ => false,
        }
    }
}
