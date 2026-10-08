use super::super::*;

impl WorldStream {
    pub(in crate::stream) fn diagnose_light_mutations(
        &self,
        prepared: &[PreparedSubChunkMutation],
        relight: &BTreeSet<SubChunkKey>,
    ) {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        for mutation in prepared
            .iter()
            .filter(|mutation| relight.contains(&mutation.key()))
        {
            let count = COUNT.fetch_add(1, Ordering::Relaxed).saturating_add(1);
            if !count.is_power_of_two() {
                continue;
            }
            let key = mutation.key();
            let previous = self.authority.terrain().sub_chunk(key);
            let sample = |sub_chunk: Option<&SubChunk>, position| {
                sub_chunk.map_or(LightBlockSample::KnownAir, |sub_chunk| {
                    sample_resident_light(sub_chunk, position, self.classifier, |id| {
                        let light = self
                            .authority
                            .runtime_assets()
                            .resolve(self.authority.network_id_mode(), id)
                            .light_properties();
                        SolverLightProperties::new(light.emission(), light.filter())
                            .expect("carrier light nibbles are validated")
                    })
                })
            };
            let changed = (0_u8..16)
                .flat_map(|y| (0_u8..16).flat_map(move |z| (0_u8..16).map(move |x| [x, y, z])))
                .find_map(|position| {
                    let before = sample(previous.as_deref(), position);
                    let after = sample(mutation.replacement(), position);
                    (before != after).then_some((position, before, after))
                });
            eprintln!("RUST_MCBE_LIGHT_MUTATION count={count} key={key:?} changed={changed:?}");
        }
    }

    /// Samples exponentially so a stalled queue cannot flood the frame thread's logs.
    pub(in crate::stream) fn record_stale_light(
        &mut self,
        key: SubChunkKey,
        identity: LightJobIdentity,
        reason: &str,
    ) {
        self.stats.stale_light_jobs = self.stats.stale_light_jobs.saturating_add(1);
        let count = self.stats.stale_light_jobs;
        if !count.is_power_of_two() {
            return;
        }
        eprintln!(
            "RUST_MCBE_STALE_LIGHT count={count} reason={reason} key={key:?} expected={identity:?} current_revision={:?} current_block_generation={:?} current_light_generation={:?} current_direct_generation={:?} resident={} pending={}",
            self.lighting
                .revisions
                .dirty(key)
                .map(|dirty| dirty.revision),
            self.lighting.block_generations.get(&key),
            self.lighting
                .store
                .light(key)
                .map(|light| light.generation()),
            self.lighting
                .direct_sky
                .get(&key)
                .map(|direct| direct.light_revision),
            self.resident.contains(&key),
            self.lighting.jobs.pending.contains_key(&key),
        );
    }
}
