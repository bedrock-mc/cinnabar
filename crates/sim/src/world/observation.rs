//! Collects query evidence at the palette reads that supply world-dependent decisions.

use std::{collections::BTreeMap, sync::Mutex};

use super::*;

/// Bounded column evidence or the first unavailable read in one query group.
pub(super) type ReadObservation =
    Mutex<Result<BTreeMap<ChunkKey, ChunkCollisionRevision>, WorldQueryError>>;

impl PaletteWorld<'_> {
    /// Runs related queries and returns every column they read in one canonical identity.
    /// An unavailable read fails the observation even if the caller treats that query as optional.
    pub fn observe<T>(
        mut self,
        query: impl FnOnce(&Self) -> T,
    ) -> Result<CollisionQuery<T>, WorldQueryError> {
        self.read_observation = Some(Mutex::new(Ok(BTreeMap::new())));
        let value = query(&self);
        let revisions = self
            .read_observation
            .expect("observation installed before querying")
            .into_inner()
            .expect("palette observation lock poisoned")?;
        Ok(CollisionQuery {
            value,
            identity: WorldCollisionIdentity::new(
                self.registry.identity(),
                revisions.into_values(),
            )?,
        })
    }

    /// Records availability before either a direct or cached palette read can use a cell.
    pub(super) fn record_cell_read(&self, key: SubChunkKey) -> Result<(), WorldQueryError> {
        if self.read_observation.is_none() {
            return Ok(());
        }
        if !self.store.is_sub_chunk_loaded(key)
            && self.unloaded_boundary_revision(key.chunk()).is_none()
        {
            return self
                .record_column_read(Err(WorldQueryError::UnloadedChunk(key.chunk())))
                .map(|_| ());
        }
        self.column_revision(key.chunk()).map(|_| ())
    }

    /// Adds column authority, including shape-owner metadata, or retains its first read failure.
    pub(super) fn record_column_read(
        &self,
        revision: Result<ChunkCollisionRevision, WorldQueryError>,
    ) -> Result<ChunkCollisionRevision, WorldQueryError> {
        let Some(observation) = &self.read_observation else {
            return revision;
        };
        let mut observation = observation
            .lock()
            .expect("palette observation lock poisoned");
        let result = (|| {
            let revisions = observation.as_mut().map_err(|error| error.clone())?;
            let revision = revision?;
            let at_capacity = revisions.len() == MAX_COLLISION_IDENTITY_CHUNKS;
            if let std::collections::btree_map::Entry::Vacant(entry) =
                revisions.entry(revision.chunk)
            {
                if at_capacity {
                    return Err(WorldQueryError::IdentityChunkLimitExceeded {
                        max: MAX_COLLISION_IDENTITY_CHUNKS,
                    });
                }
                entry.insert(revision);
            }
            Ok(revision)
        })();
        if let Err(error) = &result {
            *observation = Err(error.clone());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Supplies three loaded air columns with one shared collision registry.
    fn fixture() -> (ChunkStore, CollisionRegistry) {
        let mut store = ChunkStore::new();
        for (x, z) in [(0, 0), (1, 0), (1, 1)] {
            let key = SubChunkKey::new(0, x, 0, z);
            store.apply_request_mode_air(key).unwrap();
            store.mark_sub_chunk_loaded(key).unwrap();
        }
        let mut registry = CollisionRegistry::new();
        registry.register(0, []).unwrap();
        (store, registry)
    }

    #[test]
    fn palette_observation_unions_direct_and_cached_world_reads() {
        let (store, registry) = fixture();
        let observed = PaletteWorld::new(&store, &registry, 0, Default::default())
            .observe(|world| {
                assert_eq!(world.primary_runtime_id([15, 2, 15]).unwrap(), 0);
                assert!(world.is_air([16, 2, 15]).unwrap());
                assert!(world.is_air([17, 2, 16]).unwrap());
                assert!(world.is_air([18, 2, 16]).unwrap());
            })
            .unwrap();
        let expected = WorldCollisionIdentity::new(
            registry.identity(),
            [(0, 0), (1, 0), (1, 1)]
                .map(|(x, z)| store.collision_revision(ChunkKey::new(0, x, z)).unwrap()),
        )
        .unwrap();
        assert_eq!(observed.identity, expected);
    }

    #[test]
    fn palette_observation_fails_if_an_optional_read_was_unavailable() {
        let (store, registry) = fixture();
        let missing = ChunkKey::new(0, 2, 1);
        let observed =
            PaletteWorld::new(&store, &registry, 0, Default::default()).observe(|world| {
                world.primary_runtime_id([32, 2, 16]).ok();
                world.is_air([15, 2, 15]).unwrap_or(false)
            });
        assert_eq!(observed, Err(WorldQueryError::UnloadedChunk(missing)));
    }

    #[test]
    fn palette_observation_retains_the_revision_before_an_authoritative_write() {
        let (mut store, registry) = fixture();
        let observe = |store: &ChunkStore| {
            PaletteWorld::new(store, &registry, 0, Default::default())
                .observe(|world| world.primary_runtime_id([16, 2, 15]).unwrap())
                .unwrap()
        };
        let before = observe(&store);
        store
            .update_block(
                SubChunkKey::new(0, 1, 0, 0),
                world::BlockUpdate::new(0, 2, 15, 0, 1),
                0,
            )
            .unwrap();
        let after = observe(&store);
        assert_eq!((before.value, after.value), (0, 1));
        assert_ne!(before.identity, after.identity);
        assert_eq!(before.identity.registry, after.identity.registry);
    }
}
