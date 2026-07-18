use super::*;

/// Identity assigned by the real dirty-revision tracker to a deterministic
/// publication fixture item.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicationFixtureIdentity {
    pub key: SubChunkKey,
    pub generation: u64,
    pub dirty_since: Instant,
}

/// Opaque terminal state exposed only to the cross-crate production-pipeline
/// acceptance test.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicationFixtureSnapshot {
    pub pending_mesh_jobs: usize,
    pub in_flight_mesh_jobs: usize,
    pub pending_mesh_changes: usize,
    pub unacknowledged_meshes: usize,
}

impl WorldStream {
    fn install_publication_fixture_light(&mut self, key: SubChunkKey) {
        self.next_block_generation = self.next_block_generation.wrapping_add(1).max(1);
        let block_generation = self.next_block_generation;
        let light_revision = block_generation.wrapping_add(10_000);
        self.block_generations.insert(key, block_generation);
        self.light_store.insert_resident(
            key,
            SubChunkLight::uniform(0, 15, light_revision)
                .expect("fixture light channels are bounded"),
        );
        self.light_ownership.insert(
            key,
            LightOwnership {
                block_generation,
                light_revision,
            },
        );
        self.direct_sky.insert(
            key,
            StoredDirectSky {
                light_revision,
                mask: Arc::new(DirectSkyMask::Uniform(true)),
            },
        );
        self.light_revisions.entries.remove(&key);
        self.pending_light.remove(&key);
    }

    /// Stages current resident completions through the real bounded worker
    /// result channel. Sources and light state are installed for the whole
    /// batch before its exact current halos are captured. The next
    /// [`WorldStream::poll`] performs the same validation and publication
    /// admission used for Rayon mesh results.
    #[doc(hidden)]
    pub fn stage_publication_fixture_completions(
        &mut self,
        entries: Vec<(SubChunkKey, ChunkMesh, PackedBiomeRecord)>,
    ) -> Vec<PublicationFixtureIdentity> {
        assert!(
            entries.len() <= WORK_RESULT_CAPACITY,
            "one fixture batch respects the production result capacity"
        );
        for (key, _, _) in &entries {
            let source =
                SubChunk::decode(&[8, 1, 1, 2]).expect("decode publication fixture source");
            self.store
                .commit_sub_chunk(*key, source)
                .expect("commit publication fixture source");
            self.resident.insert(*key);
            self.known_air.remove(key);
            self.install_publication_fixture_light(*key);
        }

        entries
            .into_iter()
            .map(|(key, mesh, biome)| {
                let dirty_since = Instant::now();
                let generation = self.mark_dirty_exact(key, dirty_since);
                self.pending_mesh.remove(&key);
                self.in_flight.insert(key, generation);
                let source = self
                    .store
                    .sub_chunk(key)
                    .expect("fixture source remains resident");
                let completion = MeshCompletion {
                    key,
                    revision: generation,
                    source,
                    biome_sources: self.biome_neighbourhood(key),
                    biome,
                    tint_identity: self.biome_tint_identity(),
                    mesh,
                    dependency_mask: MeshDependencyMask::default(),
                    light_halo: self
                        .mesh_light_halo(key)
                        .expect("fixture light halo is current"),
                    queue_wait: Duration::ZERO,
                    duration: Duration::ZERO,
                };
                self.mesh_tx
                    .try_send(completion)
                    .expect("publication fixture respects the production result capacity");
                PublicationFixtureIdentity {
                    key,
                    generation,
                    dirty_since,
                }
            })
            .collect()
    }

    /// Stages a current known-air dirty revision. The next real poll dispatch
    /// converts it to a permitted zero-byte removal.
    #[doc(hidden)]
    pub fn stage_publication_fixture_known_air(
        &mut self,
        key: SubChunkKey,
    ) -> PublicationFixtureIdentity {
        assert!(
            self.store.sub_chunk(key).is_none(),
            "known-air fixture key must not have resident block storage"
        );
        self.resident.insert(key);
        self.known_air.insert(key);
        let dirty_since = Instant::now();
        let generation = self.mark_dirty_exact(key, dirty_since);
        PublicationFixtureIdentity {
            key,
            generation,
            dirty_since,
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub fn publication_fixture_snapshot(&self) -> PublicationFixtureSnapshot {
        PublicationFixtureSnapshot {
            pending_mesh_jobs: self.pending_mesh.len(),
            in_flight_mesh_jobs: self.in_flight.len(),
            pending_mesh_changes: self.mesh_changes.len(),
            unacknowledged_meshes: self.revisions.entries.len(),
        }
    }
}
