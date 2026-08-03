use super::*;

impl WorldStream {
    pub(super) fn mark_changed(&mut self, key: SubChunkKey, now: Instant) {
        self.mark_changed_sources(std::iter::once(key), now);
    }
    pub(super) fn mark_live_mutation_changed(
        &mut self,
        key: SubChunkKey,
        now: Instant,
        relight: bool,
    ) {
        self.mark_changed_sources_with_mesh_dirty_priority(
            std::iter::once(key),
            std::iter::empty(),
            now,
            true,
            relight,
        );
    }
    pub(super) fn mark_changed_sources(
        &mut self,
        sources: impl IntoIterator<Item = SubChunkKey>,
        now: Instant,
    ) {
        self.mark_changed_sources_with_mesh_dirty(sources, std::iter::empty(), now);
    }
    pub(super) fn mark_changed_sources_with_mesh_dirty(
        &mut self,
        sources: impl IntoIterator<Item = SubChunkKey>,
        preexpanded_dirty: impl IntoIterator<Item = SubChunkKey>,
        now: Instant,
    ) {
        self.mark_changed_sources_with_mesh_dirty_priority(
            sources,
            preexpanded_dirty,
            now,
            false,
            true,
        );
    }
    fn mark_changed_sources_with_mesh_dirty_priority(
        &mut self,
        sources: impl IntoIterator<Item = SubChunkKey>,
        preexpanded_dirty: impl IntoIterator<Item = SubChunkKey>,
        now: Instant,
        urgent: bool,
        relight: bool,
    ) {
        let sources = sources.into_iter().collect::<BTreeSet<_>>();
        if relight {
            self.mark_light_changed_sources_with_priority(sources.iter().copied(), urgent);
        }
        let mut dirty = preexpanded_dirty.into_iter().collect::<BTreeSet<_>>();
        for key in sources {
            dirty.extend(key.mesh_dependents());
            for dependent in key.mesh_neighbourhood_dependents() {
                let ao_needed = self.resident.contains(&dependent)
                    && self
                        .current_mesh_dependency_mask(dependent)
                        .is_none_or(|mask| mask.diagonal_ao);
                if ao_needed {
                    dirty.insert(dependent);
                }
            }
            for dependent in key.liquid_mesh_dependents() {
                let liquid_needed = self.resident.contains(&dependent)
                    && self
                        .current_mesh_dependency_mask(dependent)
                        .is_none_or(|mask| mask.liquid);
                if liquid_needed {
                    dirty.insert(dependent);
                }
            }
        }
        for dependent in dirty {
            self.mark_dirty_exact_with_priority(dependent, now, urgent);
        }
    }
    pub(super) fn current_mesh_dependency_mask(
        &self,
        key: SubChunkKey,
    ) -> Option<MeshDependencyMask> {
        let (generation, mask) = self.mesh_dependency_masks.get(&key).copied()?;
        let current_generation = self
            .revisions
            .dirty(key)
            .map(|dirty| dirty.revision)
            .or_else(|| self.applied_mesh_generations.get(&key).copied())?;
        (generation == current_generation).then_some(mask)
    }
    pub(super) fn register_mesh_dependency_mask(
        &mut self,
        key: SubChunkKey,
        generation: u64,
        mask: MeshDependencyMask,
    ) -> bool {
        if !self.resident.contains(&key) || !self.revisions.is_current(key, generation) {
            return false;
        }
        self.mesh_dependency_masks.insert(key, (generation, mask));
        true
    }
    #[cfg(test)]
    pub(super) fn mesh_dependency_mask(
        &self,
        key: SubChunkKey,
    ) -> Option<(u64, MeshDependencyMask)> {
        self.mesh_dependency_masks.get(&key).copied()
    }
    pub(super) fn mark_dirty_exact(&mut self, key: SubChunkKey, now: Instant) -> u64 {
        self.mark_dirty_exact_with_priority(key, now, false)
    }
    pub(super) fn mark_dirty_exact_with_priority(
        &mut self,
        key: SubChunkKey,
        now: Instant,
        urgent: bool,
    ) -> u64 {
        let urgent = urgent
            || self
                .pending_mesh
                .get(&key)
                .is_some_and(|pending| pending.urgent)
            || self.urgent_mesh_in_flight.contains(&key);
        let revision = self.revisions.mark_dirty(key, now);
        let since = self.revisions.dirty(key).map_or(now, |dirty| dirty.since);
        if urgent {
            self.pending_mesh_scan.push_front((key, revision));
        } else {
            self.pending_mesh_scan.push_back((key, revision));
        }
        self.pending_mesh.insert(
            key,
            PendingMesh {
                revision,
                since,
                queued_at: now,
                urgent,
            },
        );
        revision
    }
    #[cfg(test)]
    pub(super) fn mark_light_mesh_dependents(&mut self, source: SubChunkKey, now: Instant) {
        for dependent in source.mesh_neighbourhood_dependents() {
            if self.resident.contains(&dependent) && self.store.sub_chunk(dependent).is_some() {
                self.mark_dirty_exact(dependent, now);
            }
        }
    }
    pub(super) fn mark_changed_light_mesh_dependents(
        &mut self,
        source: SubChunkKey,
        changed_faces: [bool; 6],
        now: Instant,
        urgent: bool,
    ) {
        for dependent in source.mesh_neighbourhood_dependents() {
            let dx = dependent.x - source.x;
            let dy = dependent.y - source.y;
            let dz = dependent.z - source.z;
            let samples_changed_light = (dx == 0 || changed_faces[usize::from(dx > 0)])
                && (dy == 0 || changed_faces[2 + usize::from(dy > 0)])
                && (dz == 0 || changed_faces[4 + usize::from(dz > 0)]);
            if samples_changed_light
                && self.resident.contains(&dependent)
                && self.store.sub_chunk(dependent).is_some()
            {
                if !self.in_flight.contains_key(&dependent)
                    && let Some(pending) = self.pending_mesh.get_mut(&dependent)
                {
                    if urgent {
                        pending.urgent = true;
                        self.pending_mesh_scan
                            .push_front((dependent, pending.revision));
                    }
                    continue;
                }
                self.mark_dirty_exact_with_priority(dependent, now, urgent);
            }
        }
    }
    pub(super) fn mark_mesh_neighbourhood_dirty(&mut self, source: SubChunkKey, now: Instant) {
        for dependent in source.mesh_neighbourhood_dependents() {
            self.mark_dirty_exact(dependent, now);
        }
    }
    pub(super) fn mark_forced_dirty_exact(&mut self, key: SubChunkKey, now: Instant) -> u64 {
        let urgent = self
            .pending_mesh
            .get(&key)
            .is_some_and(|pending| pending.urgent)
            || self.urgent_mesh_in_flight.contains(&key)
            || self.mesh_changes.iter().any(|change| {
                matches!(
                    change,
                    WorldMeshChange::Upsert {
                        key: changed_key,
                        urgent: true,
                        ..
                    } if *changed_key == key
                )
            });
        let revision = self.revisions.force_dirty_since(key, now);
        if urgent {
            self.pending_mesh_scan.push_front((key, revision));
        } else {
            self.pending_mesh_scan.push_back((key, revision));
        }
        self.pending_mesh.insert(
            key,
            PendingMesh {
                revision,
                since: now,
                queued_at: now,
                urgent,
            },
        );
        revision
    }
    pub(super) fn invalidate_resident_biome_tints(&mut self, now: Instant) {
        let renderable = self
            .resident
            .iter()
            .copied()
            .filter(|key| self.store.sub_chunk(*key).is_some())
            .collect::<Vec<_>>();
        for key in renderable {
            self.mark_forced_dirty_exact(key, now);
            self.in_flight.remove(&key);
            self.urgent_mesh_in_flight.remove(&key);
        }
        self.mesh_changes
            .retain(|change| !matches!(change, WorldMeshChange::Upsert { .. }));
    }
}
