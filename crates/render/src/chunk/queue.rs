use crate::chunk::*;
use bevy::prelude::DetectChangesMut;
use render_scene::{
    ChunkAnimationClock, ChunkRenderInstance, ChunkRenderQueue, ChunkTextureReload,
    ChunkUploadAcknowledgements, ChunkUploadBudget, RuntimeStageProfiler,
};

#[cfg(test)]
mod priority_tests;

/// Retains exact distance/age buckets, updating only changed or removed priorities.
pub(in crate::chunk) struct RetainedUploadPriorities<I, P> {
    entries: HashMap<I, RetainedPriority<P>>,
    buckets: BTreeMap<P, Vec<I>>,
    sweep: bool,
    ordinal: usize,
}

struct RetainedPriority<P> {
    priority: P,
    sweep: bool,
    ordinal: usize,
}

impl<I, P> Default for RetainedUploadPriorities<I, P> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            buckets: BTreeMap::new(),
            sweep: false,
            ordinal: 0,
        }
    }
}

impl<I: Copy + Eq + std::hash::Hash, P: Copy + Ord> RetainedUploadPriorities<I, P> {
    /// Starts reconciliation against the caller's complete active set.
    pub(in crate::chunk) fn begin(&mut self) {
        self.sweep = !self.sweep;
        self.ordinal = 0;
    }

    /// Returns a retained priority so unchanged camera distances need no recalculation.
    pub(in crate::chunk) fn priority(&self, id: I) -> Option<P> {
        self.entries.get(&id).map(|entry| entry.priority)
    }

    /// Marks an active identity, moving it between buckets only when its priority changes.
    pub(in crate::chunk) fn update(&mut self, id: I, priority: P) {
        if let Some(entry) = self.entries.get_mut(&id) {
            if entry.priority != priority {
                Self::remove(&mut self.buckets, entry.priority, id);
                self.buckets.entry(priority).or_default().push(id);
                entry.priority = priority;
            }
            entry.sweep = self.sweep;
            entry.ordinal = self.ordinal;
        } else {
            self.buckets.entry(priority).or_default().push(id);
            self.entries.insert(
                id,
                RetainedPriority {
                    priority,
                    sweep: self.sweep,
                    ordinal: self.ordinal,
                },
            );
        }
        self.ordinal += 1;
    }

    /// Drops absent identities and preserves input order for completely equal priorities.
    pub(in crate::chunk) fn finish(&mut self) {
        self.entries.retain(|&id, entry| {
            if entry.sweep == self.sweep {
                return true;
            }
            Self::remove(&mut self.buckets, entry.priority, id);
            false
        });
        for ids in self.buckets.values_mut().filter(|ids| ids.len() > 1) {
            ids.sort_by_key(|id| self.entries[id].ordinal);
        }
    }

    /// Walks retained buckets in priority order without sorting the backlog again.
    pub(in crate::chunk) fn ordered(&self) -> impl Iterator<Item = I> + '_ {
        self.buckets.values().flat_map(|ids| ids.iter().copied())
    }

    /// Removes one identity and releases empty buckets immediately.
    fn remove(buckets: &mut BTreeMap<P, Vec<I>>, priority: P, id: I) {
        if let std::collections::btree_map::Entry::Occupied(mut bucket) = buckets.entry(priority) {
            bucket.get_mut().retain(|&member| member != id);
            if bucket.get().is_empty() {
                bucket.remove();
            }
        }
    }
}

/// Encodes the same complete floating-point order as `f32::total_cmp`.
pub(in crate::chunk) fn upload_distance_key(distance: f32) -> u32 {
    let bits = distance.to_bits();
    if bits & (1 << 31) == 0 {
        bits ^ (1 << 31)
    } else {
        !bits
    }
}

pub(in crate::chunk) fn update_chunk_animation_clock(
    time: Option<Res<Time>>,
    mut clock: ResMut<ChunkAnimationClock>,
) {
    let elapsed_seconds = time.map_or(0.0, |time| time.elapsed_secs_f64());
    *clock = ChunkAnimationClock::from_elapsed_seconds(elapsed_seconds);
}

type MainUploadPriorities = RetainedUploadPriorities<(SubChunkKey, bool), (u32, SubChunkKey)>;

#[derive(SystemParam)]
pub(in crate::chunk) struct RenderQueueRuntime<'w, 's> {
    terrain_scene: Option<Res<'w, render_scene::FarTerrainScene>>,
    gpu_removals: Res<'w, ChunkGpuRemovalQueue>,
    acknowledgements: Res<'w, ChunkUploadAcknowledgements>,
    immediate_terrain: Option<
        ResMut<'w, crate::dropped_item_render::terrain_items::ImmediateTerrainMeshPublications>,
    >,
    profiler: Option<Res<'w, RuntimeStageProfiler>>,
    priorities: Local<'s, MainUploadPriorities>,
}

pub(in crate::chunk) fn apply_chunk_render_queue(
    mut commands: Commands,
    mut queue: ResMut<ChunkRenderQueue>,
    budget: Res<ChunkUploadBudget>,
    mut entities: ResMut<ChunkEntities>,
    existing_instances: Query<&ChunkRenderInstance>,
    runtime: RenderQueueRuntime,
    reload: Option<Res<ChunkTextureReload>>,
) {
    let mut runtime = runtime;
    // Clearing an already-empty list would only mark it changed and re-extract nothing new.
    if let Some(immediate) = runtime.immediate_terrain.as_mut()
        && !immediate.bypass_change_detection().0.is_empty()
    {
        immediate.0.clear();
    }
    if !queue.session_reset_pending
        && reload
            .as_ref()
            .is_some_and(|reload| reload.geometry_pending())
    {
        return;
    }
    let RenderQueueRuntime {
        terrain_scene,
        gpu_removals,
        acknowledgements,
        mut immediate_terrain,
        profiler,
        mut priorities,
    } = runtime;
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::RenderQueueApplication));
    if std::mem::take(&mut queue.session_reset_pending) {
        // A held reload snapshot names the chunks despawned below; keeping the hold would
        // stop every upload of the new session.
        if let Some(reload) = &reload {
            reload.discard_geometry();
        }
        drop(gpu_removals.take_ready(usize::MAX, |_| true));
        acknowledgements.clear();
        for (_, entity) in entities.0.drain() {
            if let Ok(instance) = existing_instances.get(entity)
                && let Some(slot) = &instance.publication_permit
            {
                drop(slot.take());
            }
            commands.entity(entity).despawn();
        }
    }
    let maximum_zero_byte_operations = budget
        .max_zero_byte_operations_per_frame
        .min(PublicationServiceConfig::DEFAULT.maximum_zero_byte_operations_per_frame);
    let mut ready = std::mem::take(&mut queue.ready_scratch);
    ready.clear();
    priorities.begin();
    for (key, priority, removal) in queue
        .pending
        .iter()
        .map(|(&key, pending)| (key, pending.priority, false))
        .chain(
            queue
                .removals
                .iter()
                .map(|(&key, pending)| (key, pending.priority, true)),
        )
    {
        priorities.update(
            (key, removal),
            (upload_distance_key(priority.distance_squared()), key),
        );
    }
    priorities.finish();
    ready.extend(priorities.ordered().map(|(key, removal)| {
        let priority = if removal {
            queue.removals[&key].priority
        } else {
            queue.pending[&key].priority
        };
        (key, priority, removal)
    }));

    let mut payload_applications = 0;
    let mut total_bytes = 0;
    let mut zero_byte_applications = 0;
    for (key, priority, removal) in ready.iter().copied() {
        let token = if removal {
            queue.removals.get(&key).and_then(|pending| pending.token)
        } else {
            queue.pending.get(&key).and_then(|pending| pending.token)
        };
        let empty = removal
            || queue
                .pending
                .get(&key)
                .is_some_and(|pending| pending.mesh.is_empty());
        if empty
            && let Some(scene) = terrain_scene.as_ref()
            && let Some(target) = scene.target.as_ref()
            && target.contains(key.chunk())
        {
            if let Some(expected) = target
                .near
                .get(&key.chunk())
                .and_then(|keys| keys.get(&key))
            {
                if token.is_some_and(|token| token.generation < *expected) {
                    // A cancelled withdrawal must not despawn a newer near target's old cover.
                    if removal {
                        queue.removals.remove(&key);
                    } else if let Some(pending) = queue.pending.remove(&key) {
                        queue.pending_bytes = queue
                            .pending_bytes
                            .saturating_sub(pending_upload_byte_len(&pending));
                    }
                    continue;
                }
            } else if !scene
                .residency
                .may_remove_near(far_terrain::ColumnPos::new(key.x, key.z))
            {
                // Physical deletion follows the render-side draw switch, never its main ack.
                continue;
            }
        }
        if removal {
            let permitted = queue
                .removals
                .get(&key)
                .is_some_and(|pending| pending.publication_permit.is_some());
            if zero_byte_applications >= maximum_zero_byte_operations
                || (permitted && !gpu_removals.has_capacity_for(key, priority))
            {
                continue;
            }
            let token = queue.removals.get(&key).and_then(|pending| pending.token);
            if !permitted && token.is_some_and(|token| !acknowledgements.try_reserve(key, token)) {
                continue;
            }
            let Some(pending) = queue.removals.remove(&key) else {
                continue;
            };
            queue.tracked_generations.remove(&key);
            let render_permit = pending
                .publication_permit
                .map(PublicationPermit::into_render_entity)
                .transpose();
            let render_permit = match render_permit {
                Ok(permit) => permit,
                Err(permit) => {
                    drop(permit);
                    continue;
                }
            };
            if let Some(entity) = entities.0.remove(&key) {
                if let Ok(instance) = existing_instances.get(entity)
                    && let Some(slot) = &instance.publication_permit
                {
                    drop(slot.take());
                }
                commands.entity(entity).despawn();
            }
            if let Some(permit) = render_permit {
                gpu_removals
                    .push(PendingGpuRemoval {
                        publications: terrain_scene
                            .as_ref()
                            .and_then(|scene| scene.publications.clone()),
                        kind: pending.kind,
                        key,
                        token,
                        priority,
                        permit,
                    })
                    .unwrap_or_else(|_| unreachable!("removal mailbox capacity was checked"));
            } else if let Some(token) = token {
                if let Some(source) = terrain_scene
                    .as_ref()
                    .and_then(|scene| scene.publications.as_ref())
                {
                    source.record(
                        key,
                        token.generation,
                        pending.kind == render_api::TerrainRemovalKind::Empty,
                    );
                }
                acknowledgements.complete(key, token, Instant::now());
                if let Some(immediate) = immediate_terrain.as_deref_mut() {
                    immediate.0.push((key, token.generation));
                }
            }
            zero_byte_applications = zero_byte_applications.saturating_add(1);
            continue;
        }
        let Some(pending) = queue.pending.get(&key) else {
            continue;
        };
        let pending_bytes = if pending.mesh.is_empty() {
            0
        } else {
            pending_upload_byte_len(pending)
        };
        if pending.publication_permit.is_none()
            && pending_bytes != 0
            && !budget.can_fit(payload_applications, total_bytes, 1, pending_bytes)
        {
            continue;
        }
        if pending.mesh.is_empty()
            && ((zero_byte_applications >= maximum_zero_byte_operations
                || (pending.publication_permit.is_some()
                    && !gpu_removals.has_capacity_for(key, pending.priority)))
                || pending.token.is_some_and(|token| {
                    pending.publication_permit.is_none()
                        && !acknowledgements.try_reserve(key, token)
                }))
        {
            continue;
        }
        let Some(pending) = queue.pending.remove(&key) else {
            continue;
        };
        queue.pending_bytes = queue
            .pending_bytes
            .saturating_sub(pending_upload_byte_len(&pending));
        let render_permit = pending
            .publication_permit
            .map(PublicationPermit::into_render_entity)
            .transpose();
        let render_permit = match render_permit {
            Ok(permit) => permit,
            Err(permit) => {
                drop(permit);
                continue;
            }
        };
        gpu_removals.cancel(key);
        if pending.mesh.is_empty() {
            queue.tracked_generations.remove(&key);
            if let Some(entity) = entities.0.remove(&key) {
                if let Ok(instance) = existing_instances.get(entity)
                    && let Some(slot) = &instance.publication_permit
                {
                    drop(slot.take());
                }
                commands.entity(entity).despawn();
            }
            if let Some(permit) = render_permit {
                gpu_removals
                    .push(PendingGpuRemoval {
                        publications: terrain_scene
                            .as_ref()
                            .and_then(|scene| scene.publications.clone()),
                        kind: render_api::TerrainRemovalKind::Empty,
                        key,
                        token: pending.token,
                        priority: pending.priority,
                        permit,
                    })
                    .unwrap_or_else(|_| unreachable!("removal mailbox capacity was checked"));
            } else if let Some(token) = pending.token {
                if let Some(source) = terrain_scene
                    .as_ref()
                    .and_then(|scene| scene.publications.as_ref())
                {
                    source.record(key, token.generation, true);
                }
                acknowledgements.complete(key, token, Instant::now());
                if let Some(immediate) = immediate_terrain.as_deref_mut() {
                    immediate.0.push((key, token.generation));
                }
            }
            zero_byte_applications = zero_byte_applications.saturating_add(1);
            continue;
        }

        let origin = chunk_origin(key);
        let cube_layout = pending.mesh.cube_layout();
        let (
            cube_quads,
            cube_lighting,
            model_refs,
            model_lighting,
            model_draw_refs,
            transparent_model_draw_refs,
            liquid_quads,
            liquid_lighting,
        ) = pending.mesh.into_streams();
        debug_assert_eq!(cube_quads.len(), cube_lighting.len());
        let depth_liquid_start = liquid_quads
            .iter()
            .position(|quad| quad.is_depth_writing())
            .and_then(|index| u32::try_from(index).ok());
        debug_assert!(depth_liquid_start.is_none_or(|start| {
            liquid_quads[start as usize..]
                .iter()
                .all(|quad| quad.is_depth_writing())
        }));
        let has_depth_liquid = depth_liquid_start.is_some();
        let has_transparent_liquid = liquid_quads
            .first()
            .is_some_and(|quad| !quad.is_depth_writing());
        let bounds = super::bounds::aabb(!model_refs.is_empty());
        let instance = ChunkRenderInstance {
            key,
            cube_quads: Arc::from(cube_quads),
            cube_lighting: Arc::from(cube_lighting),
            cube_layout,
            model_refs: Arc::from(model_refs),
            model_lighting: Arc::from(model_lighting),
            model_draw_refs: Arc::from(model_draw_refs),
            transparent_model_draw_refs: Arc::from(transparent_model_draw_refs),
            liquid_quads: Arc::from(liquid_quads),
            liquid_lighting: Arc::from(liquid_lighting),
            has_depth_liquid,
            has_transparent_liquid,
            depth_liquid_start,
            biome: pending.biome,
            tint_identity: pending.tint_identity,
            generation: pending.generation,
            priority: pending.priority,
            token: pending.token,
            publication_permit: render_permit.map(PublicationPermitSlot::new),
            origin,
        };
        if let Some(&entity) = entities.0.get(&key) {
            if let Ok(existing) = existing_instances.get(entity)
                && let Some(slot) = &existing.publication_permit
            {
                drop(slot.take());
            }
            commands.entity(entity).insert((instance, bounds));
        } else {
            // Chunks never move or join a hierarchy, so they carry only the world transform that
            // culling reads; a `Transform` would add every chunk to Bevy's per-frame sync scan.
            let entity = commands
                .spawn((
                    instance,
                    Visibility::default(),
                    GlobalTransform::from_xyz(origin[0] as f32, origin[1] as f32, origin[2] as f32),
                    bounds,
                ))
                .id();
            entities.0.insert(key, entity);
        }
        if pending_bytes != 0 {
            payload_applications = payload_applications.saturating_add(1);
            total_bytes = total_bytes.saturating_add(pending_bytes);
        }
    }
    queue.ready_scratch = ready;
}
