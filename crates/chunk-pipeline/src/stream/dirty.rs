use super::*;

/// Dependency facts stay valid while the section's blocks and asset registry stay the same.
pub(super) struct RegisteredMeshDependencies {
    #[cfg(test)]
    generation: u64,
    mask: MeshDependencyMask,
    source: Option<std::sync::Weak<SubChunk>>,
    assets: std::sync::Weak<RuntimeAssets>,
}

/// How far a source change propagates: its scheduling priority, light and neighbour reach.
#[derive(Clone, Copy)]
struct ChangeScope<'a> {
    urgent: bool,
    relight: bool,
    reach: NeighbourReach,
    cells: Option<&'a [BlockPos]>, // Known changed cells; `None` revisits the whole source.
}

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
        self.mark_block_mutation_changed(key, now, relight, NeighbourReach::ALL, &[]);
    }
    /// Marks a block change urgent, remeshing only the neighbours its changed cells `reach`;
    /// neighbours whose light it changes are remeshed when the relight lands. Known changed
    /// `cells` let the relight revisit only what they affect.
    pub(super) fn mark_block_mutation_changed(
        &mut self,
        key: SubChunkKey,
        now: Instant,
        relight: bool,
        reach: NeighbourReach,
        cells: &[BlockPos],
    ) {
        self.mark_changed_sources_with_mesh_dirty_priority(
            std::iter::once(key),
            std::iter::empty(),
            now,
            ChangeScope {
                urgent: true,
                relight,
                reach,
                cells: (!cells.is_empty()).then_some(cells),
            },
        );
        self.urgent_work_due = true;
    }
    /// Keys of `prepared` whose committed replacement leaves every mesh unchanged: the key is
    /// resident with blocks, needs no relight, and [`meshing::block_changes_preserve_meshes`]
    /// proves its resident section and replacement feed the mesher identically. A batch whose
    /// keys are not strictly ascending, and so may repeat, proves nothing. Call before commit.
    pub(super) fn mesh_neutral_mutations(
        &self,
        prepared: &[PreparedSubChunkMutation],
        relight: &BTreeSet<SubChunkKey>,
    ) -> BTreeSet<SubChunkKey> {
        if !prepared
            .windows(2)
            .all(|pair| pair[0].key() < pair[1].key())
        {
            return BTreeSet::new();
        }
        let terrain = self.authority.terrain();
        prepared
            .iter()
            .filter(|mutation| {
                let key = mutation.key();
                // A resident non-air key keeps its residency through the commit.
                mutation.changed()
                    && !relight.contains(&key)
                    && self.resident.contains(&key)
                    && !self.known_air.contains(&key)
            })
            .filter(|mutation| {
                let (Some(previous), Some(replacement)) = (
                    terrain.sub_chunk_ref(mutation.key()),
                    mutation.replacement(),
                ) else {
                    return false;
                };
                ::meshing::block_changes_preserve_meshes(
                    self.classifier,
                    self.authority.runtime_assets(),
                    self.authority.network_id_mode(),
                    previous,
                    replacement,
                    mutation.changed_positions().map(|position| {
                        [position.x, position.y, position.z]
                            .map(|axis| axis.rem_euclid(world::SUB_CHUNK_SIDE as i32) as u8)
                    }),
                    self.java_meshing.as_deref().unwrap_or_default(),
                )
            })
            .map(PreparedSubChunkMutation::key)
            .collect()
    }
    /// Records a committed block change that [`Self::mesh_neutral_mutations`] proved leaves
    /// every mesh unchanged: its far summary still refreshes and urgent work still runs, but no
    /// mesh is invalidated.
    pub(super) fn mark_mesh_neutral_block_change(&mut self, key: SubChunkKey) {
        if self.far_terrain_enabled() {
            self.note_far_change(key.chunk());
        }
        self.urgent_work_due = true;
    }
    pub(super) fn mark_changed_sources(
        &mut self,
        sources: impl IntoIterator<Item = SubChunkKey>,
        now: Instant,
    ) {
        self.mark_changed_sources_with_mesh_dirty(sources, std::iter::empty(), now);
    }
    /// Expands one inline commit before invalidating meshes, and relights only changed inputs.
    pub(super) fn mark_inline_changed_sources(
        &mut self,
        sources: BTreeSet<SubChunkKey>,
        preexpanded_dirty: impl IntoIterator<Item = SubChunkKey>,
        light_neutral: &BTreeSet<SubChunkKey>,
        now: Instant,
    ) {
        self.mark_light_changed_sources_reaching(
            sources.difference(light_neutral).copied(),
            false,
            NeighbourReach::ALL,
            None,
        );
        self.mark_changed_sources_with_mesh_dirty_priority(
            sources,
            preexpanded_dirty,
            now,
            ChangeScope {
                urgent: false,
                relight: false,
                reach: NeighbourReach::ALL,
                cells: None,
            },
        );
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
            ChangeScope {
                urgent: false,
                relight: true,
                reach: NeighbourReach::ALL,
                cells: None,
            },
        );
    }
    fn mark_changed_sources_with_mesh_dirty_priority(
        &mut self,
        sources: impl IntoIterator<Item = SubChunkKey>,
        preexpanded_dirty: impl IntoIterator<Item = SubChunkKey>,
        now: Instant,
        scope: ChangeScope<'_>,
    ) {
        let ChangeScope {
            urgent,
            relight,
            reach,
            cells,
        } = scope;
        let sources = sources.into_iter().collect::<BTreeSet<_>>();
        // A far summary reads only its own column's blocks, so only sources refresh one.
        if self.far_terrain_enabled() {
            for column in sources
                .iter()
                .map(|key| key.chunk())
                .collect::<BTreeSet<_>>()
            {
                self.note_far_change(column);
            }
        }
        if relight {
            self.mark_light_changed_sources_reaching(sources.iter().copied(), urgent, reach, cells);
        }
        let mut dirty = preexpanded_dirty.into_iter().collect::<BTreeSet<_>>();
        for &key in &sources {
            let reached = |dependent: &SubChunkKey| {
                reach.mesh_reaches([
                    i64::from(dependent.x) - i64::from(key.x),
                    i64::from(dependent.y) - i64::from(key.y),
                    i64::from(dependent.z) - i64::from(key.z),
                ])
            };
            dirty.extend(key.mesh_dependents().filter(reached));
            // A roof/log several sub-chunks above a seasonal leaf can change
            // its tint. Restrict the extra dependency to the same X/Z column,
            // resident lower chunks, and meshes whose palette actually has leaves.
            if let Some(column) = self.authority.terrain().chunk(key.chunk()) {
                for (y, _) in column.sub_chunks() {
                    if y >= key.y {
                        continue;
                    }
                    let dependent = SubChunkKey::from_chunk(key.chunk(), y);
                    if self.resident.contains(&dependent)
                        && self
                            .current_mesh_dependency_mask(dependent)
                            .is_none_or(|mask| mask.seasonal_foliage)
                    {
                        dirty.insert(dependent);
                    }
                }
            }
            for dependent in key.mesh_neighbourhood_dependents().filter(reached) {
                let ao_needed = self.resident.contains(&dependent)
                    && self
                        .current_mesh_dependency_mask(dependent)
                        .is_none_or(|mask| mask.diagonal_ao);
                if ao_needed {
                    dirty.insert(dependent);
                }
            }
            for dependent in key.liquid_mesh_dependents().filter(reached) {
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
            // Only resident, non-empty neighbours sample this source; own changes still apply.
            if !sources.contains(&dependent)
                && (!self.resident.contains(&dependent) || self.known_air.contains(&dependent))
            {
                continue;
            }
            self.invalidate_mesh_with_priority(dependent, now, urgent);
        }
    }
    /// Neighbour invalidations do not change dependency facts about unchanged source blocks.
    pub(super) fn current_mesh_dependency_mask(
        &self,
        key: SubChunkKey,
    ) -> Option<MeshDependencyMask> {
        let registered = self.mesh_dependency_masks.get(&key)?;
        if !std::ptr::eq(
            registered.assets.as_ptr(),
            Arc::as_ptr(self.authority.runtime_assets()),
        ) {
            return None;
        }
        let source = self.authority.terrain().sub_chunk(key);
        let same_blocks = match (&registered.source, &source) {
            (Some(previous), Some(current)) => {
                std::ptr::eq(previous.as_ptr(), Arc::as_ptr(current))
            }
            (None, None) => true,
            _ => false,
        };
        same_blocks.then_some(registered.mask)
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
        self.mesh_dependency_masks.insert(
            key,
            RegisteredMeshDependencies {
                #[cfg(test)]
                generation,
                mask,
                source: self
                    .authority
                    .terrain()
                    .sub_chunk(key)
                    .map(|source| Arc::downgrade(&source)),
                assets: Arc::downgrade(self.authority.runtime_assets()),
            },
        );
        true
    }
    #[cfg(test)]
    pub(super) fn mesh_dependency_mask(
        &self,
        key: SubChunkKey,
    ) -> Option<(u64, MeshDependencyMask)> {
        self.mesh_dependency_masks
            .get(&key)
            .map(|registered| (registered.generation, registered.mask))
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
        // Geometry follows full detail; delivered air always publishes zero-byte evidence.
        if !self.wants_mesh(key.chunk())
            && self.terrain_removal_kind(key) != render_api::TerrainRemovalKind::Empty
        {
            return 0;
        }
        self.mark_dirty_exact_ungated(key, now, urgent)
    }
    /// Marks `key` for a mesh or removal whether or not its column is full detail.
    pub(super) fn mark_dirty_exact_ungated(
        &mut self,
        key: SubChunkKey,
        now: Instant,
        urgent: bool,
    ) -> u64 {
        let urgent = urgent
            || self
                .mesh_jobs
                .pending
                .get(&key)
                .is_some_and(|pending| pending.urgent)
            || self.urgent_mesh_in_flight.contains(&key);
        self.cancel_mesh_job(key);
        let revision = self.revisions.mark_dirty(key, now);
        let since = self.revisions.dirty(key).map_or(now, |dirty| dirty.since);
        let startup = self.is_startup_dependency(key);
        let pending = PendingMesh {
            revision,
            since,
            queued_at: now,
            urgent,
        };
        self.enqueue_mesh_revision(key, pending, startup);
        revision
    }
    /// Routes geometry to the worker scheduler and air directly to bounded removal service.
    pub(super) fn enqueue_mesh_revision(
        &mut self,
        key: SubChunkKey,
        pending: PendingMesh,
        startup: bool,
    ) {
        if self.mesh_requires_geometry(key) {
            self.air_removals.remove(&key);
            self.mesh_jobs.enqueue_prioritized(key, pending, startup);
        } else {
            self.mesh_jobs.enqueue_unrouted(key, pending);
            self.air_removals.insert(key, pending.revision);
            if pending.urgent || startup {
                let candidate = PendingSchedulerCandidate::new(
                    key,
                    pending.revision,
                    self.scheduler_view(self.last_camera_position),
                    pending.urgent,
                );
                self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                    .ready
                    .push(candidate);
            }
        }
    }
    #[cfg(test)]
    pub(super) fn mark_light_mesh_dependents(&mut self, source: SubChunkKey, now: Instant) {
        for dependent in source.mesh_neighbourhood_dependents() {
            if self.resident.contains(&dependent)
                && self.authority.terrain().sub_chunk(dependent).is_some()
            {
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
                && self.authority.terrain().sub_chunk(dependent).is_some()
            {
                self.invalidate_mesh_with_priority(dependent, now, urgent);
            }
        }
    }
    /// Pending work takes its snapshot at dispatch, so repeated invalidations need one record.
    fn invalidate_mesh_with_priority(&mut self, key: SubChunkKey, now: Instant, urgent: bool) {
        if let Some(pending) = self.mesh_jobs.pending.get_mut(&key) {
            // No new scan record follows, so a deferred nearby record is offered directly.
            self.mesh_jobs.refresh.wake_near(key);
            if urgent && !pending.urgent {
                pending.urgent = true;
                self.mesh_jobs.scan.push_front((key, pending.revision));
            }
            let revision = pending.revision;
            // A coalesced change that leaves only air queues its removal now, so no state of the
            // geometry lane can hold the old mesh on screen.
            if !self.mesh_requires_geometry(key) {
                self.air_removals.insert(key, revision);
            }
            return;
        }
        self.mark_dirty_exact_with_priority(key, now, urgent);
    }
    pub(super) fn mark_mesh_neighbourhood_dirty(&mut self, source: SubChunkKey, now: Instant) {
        for dependent in source.mesh_neighbourhood_dependents() {
            self.mark_dirty_exact(dependent, now);
        }
    }
    pub(super) fn mark_forced_dirty_exact(&mut self, key: SubChunkKey, now: Instant) -> u64 {
        let urgent = self
            .mesh_jobs
            .pending
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
        self.cancel_mesh_job(key);
        let revision = self.revisions.force_dirty_since(key, now);
        self.enqueue_mesh_revision(
            key,
            PendingMesh {
                revision,
                since: now,
                queued_at: now,
                urgent,
            },
            false,
        );
        revision
    }
    /// Revises resident and unfinished geometry so queued old-quality results cannot publish.
    pub(super) fn invalidate_meshes(&mut self, now: Instant) {
        let renderable = self
            .resident
            .iter()
            .chain(self.mesh_jobs.pending.keys())
            .chain(self.mesh_jobs.in_flight.keys())
            .chain(
                self.staged_mesh_completions
                    .iter()
                    .map(|completion| &completion.key),
            )
            .copied()
            .filter(|key| self.authority.terrain().sub_chunk(*key).is_some())
            .collect::<HashSet<_>>();
        for key in renderable {
            self.mark_forced_dirty_exact(key, now);
            self.mesh_jobs.in_flight.remove(&key);
            self.urgent_mesh_in_flight.remove(&key);
        }
        self.retain_mesh_changes(|change| !matches!(change, WorldMeshChange::Upsert { .. }));
    }
}
