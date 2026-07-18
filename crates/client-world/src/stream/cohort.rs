use super::diagnostics::deterministic_chunk_key_hash;
use super::*;

impl WorldStream {
    pub fn loaded_column_count(&self) -> usize {
        self.loaded_columns.len()
    }
    pub fn capture_source_columns(&mut self) {
        self.source_columns = self.tracked_columns();
    }
    pub fn schedule_source_capture(&mut self, sequence: u64) {
        self.source_capture_sequence = Some(sequence);
    }
    pub fn cohort_status(&self, target: ViewCohort) -> ViewCohortStatus {
        let expected_columns = target.expected_columns();
        let loaded_target = self.loaded_columns.intersection(&expected_columns).count();
        let missing_target = expected_columns.difference(&self.loaded_columns).count();
        let foreign_loaded = self
            .loaded_columns
            .iter()
            .filter(|column| !target.contains_column(column.dimension, [column.x, column.z]))
            .count();
        let foreign_requested = self
            .requested_sub_chunks
            .keys()
            .filter(|column| !target.contains_column(column.dimension, [column.x, column.z]))
            .count();
        let foreign_resident = self
            .resident
            .iter()
            .chain(&self.known_air)
            .copied()
            .filter(|key| {
                let chunk = key.chunk();
                !target.contains_column(chunk.dimension, [chunk.x, chunk.z])
            })
            .collect::<BTreeSet<_>>()
            .len();
        let source_leftover = self
            .tracked_columns()
            .intersection(&self.source_columns)
            .count();

        ViewCohortStatus {
            target,
            committed: self.committed_view_cohort,
            expected: expected_columns.len(),
            required_hash: deterministic_chunk_key_hash(&expected_columns),
            loaded_target,
            missing_target,
            foreign_loaded,
            foreign_requested,
            foreign_resident,
            source_leftover,
            resident_count: self.resident.len(),
            resident_hash: deterministic_sub_chunk_key_hash(&self.resident),
            known_air_count: self.known_air.len(),
            known_air_hash: deterministic_sub_chunk_key_hash(&self.known_air),
        }
    }
    pub fn remesh_all_resident(&mut self, now: Instant) -> ForcedRemeshManifest {
        let keys = self
            .resident
            .iter()
            .chain(&self.known_air)
            .copied()
            .collect::<BTreeSet<_>>();
        let entries = keys
            .into_iter()
            .map(|key| (key, self.mark_forced_dirty_exact(key, now)))
            .collect::<Vec<_>>();
        ForcedRemeshManifest {
            started_at: now,
            entries: Arc::from(entries),
        }
    }
    pub fn remesh_published_manifest(
        &mut self,
        published: &[(SubChunkKey, u64)],
        now: Instant,
    ) -> Option<ForcedRemeshManifest> {
        let keys = published
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>();
        if published.is_empty()
            || keys.len() != published.len()
            || published.iter().any(|(key, generation)| {
                !self.resident.contains(key)
                    || self.known_air.contains(key)
                    || self.store.sub_chunk(*key).is_none()
                    || self.applied_mesh_generations.get(key) != Some(generation)
            })
        {
            return None;
        }

        let entries = keys
            .into_iter()
            .map(|key| (key, self.mark_forced_dirty_exact(key, now)))
            .collect::<Vec<_>>();
        Some(ForcedRemeshManifest {
            started_at: now,
            entries: Arc::from(entries),
        })
    }
    pub fn forced_remesh_manifest_state(
        &self,
        manifest: &ForcedRemeshManifest,
    ) -> ForcedRemeshManifestState {
        let current_keys = self
            .resident
            .iter()
            .chain(&self.known_air)
            .copied()
            .collect::<BTreeSet<_>>();
        let manifest_keys = manifest
            .entries
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>();
        if manifest.entries.is_empty()
            || manifest_keys.len() != manifest.entries.len()
            || !manifest_keys.is_subset(&current_keys)
        {
            return ForcedRemeshManifestState::Invalid;
        }

        let mut pending = false;
        for &(key, generation) in manifest.entries.iter() {
            match self.revisions.dirty(key) {
                Some(dirty)
                    if dirty.revision == generation && dirty.since == manifest.started_at =>
                {
                    pending = true;
                }
                Some(_) => return ForcedRemeshManifestState::Invalid,
                None if self.applied_mesh_generations.get(&key) == Some(&generation) => {}
                None => return ForcedRemeshManifestState::Invalid,
            }
        }
        if pending {
            ForcedRemeshManifestState::Pending
        } else {
            ForcedRemeshManifestState::Complete
        }
    }
}
