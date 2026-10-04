use super::diagnostics::deterministic_chunk_key_hash;
use super::*;

// ClientLoadingProgressTickingSystem::mChunksNeededForLoadOffsets covers nine columns.
const STARTUP_RADIUS: i32 = 1;

/// Server publisher scope, the view cohort committed from it, and the columns it requires.
#[derive(Default)]
pub(super) struct PublisherScope {
    pub(super) center: Option<[i32; 3]>,
    pub(super) radius_blocks: Option<u32>,
    pub(super) radius_chunks: Option<i32>,
    pub(super) cohort: Option<ViewCohort>,
    /// A local teleport dropped the cohort; the next publisher update recommits it.
    pub(super) provisional_rebase: bool,
    pub(super) epoch: u64,
    pub(super) required_columns: BTreeSet<ChunkKey>,
    pub(super) source_columns: BTreeSet<ChunkKey>,
    pub(super) source_capture_sequence: Option<u64>,
    pub(super) local_reset: LocalResetEvidence,
}

impl PublisherScope {
    /// Drops the committed cohort until the server republishes after a local teleport.
    pub(super) fn begin_local_rebase(&mut self) {
        self.cohort = None;
        self.required_columns.clear();
        self.provisional_rebase = true;
        self.local_reset.arm();
    }

    /// Restarts scope at a new dimension's position; the epoch and source capture survive.
    pub(super) fn reset_for_dimension(&mut self, center: [i32; 3]) {
        self.center = Some(center);
        self.radius_blocks = None;
        self.radius_chunks = None;
        self.cohort = None;
        self.provisional_rebase = false;
        self.local_reset = LocalResetEvidence::default();
        self.required_columns.clear();
    }
}

/// Request classes dispatched after a local reset, until player-column work is sent.
#[derive(Default)]
pub(super) struct LocalResetEvidence {
    pub(super) armed: u64,
    pub(super) consumed: u64,
    pub(super) dispatch_count: u8,
    pub(super) dispatch_total: u64,
    pub(super) dispatch_active: bool,
    pub(super) dispatch_classes: [Option<RequestClass>; MAX_LOCAL_RESET_DISPATCH_EVIDENCE],
}

impl LocalResetEvidence {
    fn arm(&mut self) {
        self.armed = self.armed.saturating_add(1);
        self.dispatch_count = 0;
        self.dispatch_total = 0;
        self.dispatch_active = true;
        self.dispatch_classes = [None; MAX_LOCAL_RESET_DISPATCH_EVIDENCE];
    }

    pub(super) fn record_dispatch(&mut self, class: Option<RequestClass>) {
        if !self.dispatch_active {
            return;
        }
        let Some(class) = class else {
            return;
        };
        self.dispatch_total = self.dispatch_total.saturating_add(1);
        if matches!(
            class,
            RequestClass::PlayerInitial | RequestClass::PlayerRetry
        ) {
            self.dispatch_active = false;
        }
        if usize::from(self.dispatch_count) >= MAX_LOCAL_RESET_DISPATCH_EVIDENCE {
            return;
        }
        self.dispatch_classes[usize::from(self.dispatch_count)] = Some(class);
        self.dispatch_count = self.dispatch_count.saturating_add(1);
    }
}

impl WorldStream {
    /// Startup needs the player's loaded 3×3 neighborhood, not a drained distant view.
    /// Mesh acknowledgements additionally prevent exposing unpresented local terrain.
    #[must_use]
    pub fn local_terrain_ready(&self) -> bool {
        let position = self.authority.resolved_server_position().position;
        let center = ChunkKey::new(
            self.authority.current_dimension(),
            floor_to_i32(position[0]).div_euclid(16),
            floor_to_i32(position[2]).div_euclid(16),
        );
        let nearby = |column: ChunkKey| {
            column.dimension == center.dimension
                && column.x.abs_diff(center.x) <= STARTUP_RADIUS as u32
                && column.z.abs_diff(center.z) <= STARTUP_RADIUS as u32
        };
        if !(-STARTUP_RADIUS..=STARTUP_RADIUS).all(|x| {
            (-STARTUP_RADIUS..=STARTUP_RADIUS).all(|z| {
                self.loaded_columns.contains(&ChunkKey::new(
                    center.dimension,
                    center.x.saturating_add(x),
                    center.z.saturating_add(z),
                ))
            })
        }) {
            return false;
        }
        self.resident
            .iter()
            .filter(|key| nearby(key.chunk()))
            .all(|key| self.light_is_current(*key) && self.is_mesh_clean(*key))
    }

    /// Unique columns announced in the current publisher epoch through either
    /// admission path (request-mode or inline), after each path's decode and
    /// admission gates.
    #[must_use]
    pub fn required_columns(&self) -> &BTreeSet<ChunkKey> {
        &self.publisher.required_columns
    }

    pub fn loaded_column_count(&self) -> usize {
        self.loaded_columns.len()
    }
    pub fn capture_source_columns(&mut self) {
        self.publisher.source_columns = self.tracked_columns();
    }
    pub fn schedule_source_capture(&mut self, sequence: u64) {
        self.publisher.source_capture_sequence = Some(sequence);
    }
    /// Records whether the server sent terrain before spawn (see
    /// [`Self::startup_view_complete`]).
    pub fn set_startup_terrain_announced(&mut self, announced: bool) {
        self.startup_terrain_announced = announced;
    }

    /// Whether startup has all the terrain it waits for: the committed view's
    /// columns once the server publishes one, else nothing when the server sent
    /// no terrain before spawn (Dragonfly streams only after initialization).
    #[must_use]
    pub fn startup_view_complete(&self) -> bool {
        match self.publisher.cohort {
            Some(target) => self.cohort_status(target).target_is_complete(),
            None => !self.startup_terrain_announced,
        }
    }
    pub fn cohort_status(&self, target: ViewCohort) -> ViewCohortStatus {
        let uses_explicit_required =
            self.publisher.cohort == Some(target) && target.publisher_geometry.is_some();
        let expected_columns = if self.publisher.cohort == Some(target) {
            if uses_explicit_required {
                self.publisher.required_columns.clone()
            } else {
                target.classifier_columns()
            }
        } else {
            BTreeSet::new()
        };
        let loaded_target = self.loaded_columns.intersection(&expected_columns).count();
        let missing_target = expected_columns.difference(&self.loaded_columns).count();
        let foreign_loaded = self
            .loaded_columns
            .iter()
            .filter(|column| {
                if uses_explicit_required {
                    !expected_columns.contains(column)
                } else {
                    !target.contains_column(column.dimension, [column.x, column.z])
                }
            })
            .count();
        let foreign_requested = self
            .requests
            .requested
            .keys()
            .filter(|column| {
                if uses_explicit_required {
                    !expected_columns.contains(column)
                } else {
                    !target.contains_column(column.dimension, [column.x, column.z])
                }
            })
            .count();
        let foreign_resident = self
            .resident
            .iter()
            .chain(&self.known_air)
            .copied()
            .filter(|key| {
                let chunk = key.chunk();
                if uses_explicit_required {
                    !expected_columns.contains(&chunk)
                } else {
                    !target.contains_column(chunk.dimension, [chunk.x, chunk.z])
                }
            })
            .collect::<BTreeSet<_>>()
            .len();
        let source_leftover = self
            .tracked_columns()
            .intersection(&self.publisher.source_columns)
            .count();

        ViewCohortStatus {
            target,
            committed: self.publisher.cohort,
            publisher_epoch: self.publisher.epoch,
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
                    || self.authority.terrain().sub_chunk(*key).is_none()
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
