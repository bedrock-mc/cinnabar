//! Opaque terrain draw lists and CPU-planned indirect commands kept across frames. A view's
//! lists rebuild only when its visible set, a visible allocation, the tint table or a probe
//! changes them; each pass's commands rebuild only when its list or face selection changes.

use bevy::render::view::RenderVisibleEntitiesClass;
use render_scene::RuntimeStageProfiler;

use crate::chunk::*;

/// Past this many changed sub-chunks between queues, every view rebuilds instead of testing each.
const MAX_TRACKED_CHANGES: usize = 1024;

/// Allocation changes the opaque queue has not consumed yet.
#[derive(Default)]
pub(in crate::chunk) struct TerrainBatchChanges {
    /// Changed render entities with their main entities; empty once `all` is set.
    changed: Vec<(Entity, MainEntity)>,
    /// Every view rebuilds: the tint table changed, a changed entity has no main entity, or
    /// too many changed.
    all: bool,
    /// Some allocation changed, so the resident model summary is stale.
    any: bool,
}

impl TerrainBatchChanges {
    fn record(&mut self, entity: Entity, main: Option<MainEntity>) {
        self.any = true;
        match main {
            Some(main) if !self.all && self.changed.len() < MAX_TRACKED_CHANGES => {
                self.changed.push((entity, main));
            }
            _ => self.invalidate_all(),
        }
    }

    fn invalidate_all(&mut self) {
        self.any = true;
        self.all = true;
        self.changed.clear();
    }
}

/// Cumulative work counters of retained terrain draw preparation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::chunk) struct TerrainBatchStats {
    /// View lists rebuilt from Bevy's visible set and the allocations.
    pub(in crate::chunk) lists_rebuilt: u64,
    pub(in crate::chunk) lists_retained: u64,
    /// Front-to-back cube orders sorted again for a moved indirect view.
    pub(in crate::chunk) cube_sorts: u64,
    /// Per-pass indirect command batches rebuilt from their lists.
    pub(in crate::chunk) batches_rebuilt: u64,
    pub(in crate::chunk) batches_retained: u64,
}

/// Per-view terrain lists, the changes that stale them, and the indirect buffer's layout state.
#[derive(Resource, Default)]
pub(in crate::chunk) struct RetainedTerrainBatches {
    views: HashMap<Entity, ViewTerrainLists>,
    changes: TerrainBatchChanges,
    tint: Option<ChunkBiomeTintIdentity>,
    resident_models: Option<ModelWorkloadCount>,
    /// Queue runs so far; a view not refreshed in the latest run is forgotten.
    frame: u64,
    /// The queue dropped a view's indirect batches, so the remaining offsets must be laid out again.
    pub(in crate::chunk) layout_changed: bool,
    /// The indirect buffer the current layout was written to.
    uploaded_buffer: Option<BufferId>,
    /// Scratch for the concatenated indirect commands.
    commands: Vec<DrawIndexedIndirectArgs>,
    pub(in crate::chunk) stats: TerrainBatchStats,
}

impl RetainedTerrainBatches {
    /// Takes the changes recorded since the last queue and forgets views it did not refresh.
    pub(in crate::chunk) fn begin_frame(&mut self) -> TerrainBatchChanges {
        let frame = self.frame;
        self.views.retain(|_, lists| lists.frame == frame);
        self.frame = frame.wrapping_add(1);
        std::mem::take(&mut self.changes)
    }

    /// Model workload of every resident allocation, summarised again only after one changed.
    pub(in crate::chunk) fn resident_models(
        &mut self,
        changes: &TerrainBatchChanges,
        summarize: impl FnOnce() -> ModelWorkloadCount,
    ) -> ModelWorkloadCount {
        if changes.any || self.resident_models.is_none() {
            self.resident_models = Some(summarize());
        }
        self.resident_models.unwrap_or_default()
    }

    /// The view's lists, stale when its visible set, a visible allocation or the tint table
    /// changed, or when they were or will be built for a probe (`reusable` false).
    pub(in crate::chunk) fn refresh_view(
        &mut self,
        view: Entity,
        visible: Option<&RenderVisibleEntitiesClass>,
        changes: &TerrainBatchChanges,
        reusable: bool,
    ) -> &mut ViewTerrainLists {
        let current = || {
            visible
                .into_iter()
                .flat_map(|class| class.iter_visible())
                .map(|(entity, main)| (*entity, *main))
        };
        let lists = self.views.entry(view).or_default();
        lists.frame = self.frame;
        let same_source = lists.source.iter().copied().eq(current());
        let stale = !reusable
            || !lists.reusable
            || changes.all
            || !same_source
            || visible.is_some_and(|class| {
                changes
                    .changed
                    .iter()
                    .any(|&(entity, main)| class.entity_pair_is_visible(entity, main))
            });
        if !same_source {
            lists.source.clear();
            lists.source.extend(current());
        }
        if stale {
            lists.invalidate();
            self.stats.lists_rebuilt += 1;
        } else {
            self.stats.lists_retained += 1;
        }
        lists.reusable = reusable;
        lists
    }
}

/// A visible sub-chunk whose allocation draws under the active tint table.
#[derive(Clone, Copy, Debug)]
pub(in crate::chunk) struct DrawableTerrain {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) main: MainEntity,
    pub(in crate::chunk) key: SubChunkKey,
    /// The cube stream is valid, so a solid draw is queued even when no run faces the camera.
    pub(in crate::chunk) solid: bool,
    pub(in crate::chunk) cutout: bool,
    pub(in crate::chunk) model: bool,
    pub(in crate::chunk) depth_liquid: bool,
}

impl DrawableTerrain {
    /// `streams` evaluates the direct-draw stream checks; indirect passes filter when prepared.
    pub(in crate::chunk) fn new(
        entity: Entity,
        main: MainEntity,
        allocation: &GpuChunkAllocation,
        streams: bool,
    ) -> Self {
        Self {
            entity,
            main,
            key: allocation.key,
            solid: streams && cube_stream_drawable(allocation),
            cutout: streams && cutout_indirect_command(allocation).is_some(),
            model: streams && model_direct_draw_command(allocation).is_some(),
            depth_liquid: streams && depth_liquid_direct_draw_command(allocation).is_some(),
        }
    }
}

/// Indirect lists to queue for a view this frame; `None` where the queued batch still holds.
pub(in crate::chunk) struct IndirectListUpdate {
    /// Drawable entities by entity order, for the model and depth-liquid passes.
    pub(in crate::chunk) visible: Option<Vec<Entity>>,
    /// Drawable entities front to back, for the solid and cutout passes.
    pub(in crate::chunk) cubes: Option<Vec<Entity>>,
    /// Whether the cube order was sorted again.
    pub(in crate::chunk) sorted: bool,
}

/// One view's visible terrain, kept while its inputs hold.
#[derive(Default)]
pub(in crate::chunk) struct ViewTerrainLists {
    /// Bevy's visible chunk entities the lists were built from, in its order.
    source: Vec<(Entity, MainEntity)>,
    frame: u64,
    /// Built without a probe, so a later frame may reuse them.
    reusable: bool,
    visible_models: Option<ModelWorkloadCount>,
    drawable: Vec<DrawableTerrain>,
    drawable_built: bool,
    /// Indirect views: drawable entities and keys by entity, and the queued cube order.
    by_entity: Vec<(Entity, SubChunkKey)>,
    cubes: Vec<Entity>,
    indirect_built: bool,
    /// The view transform `cubes` was sorted for.
    cube_view: Option<Mat4>,
    units: Option<Arc<super::units::sources::UnitSources>>,
    units_stale: bool,
}

impl ViewTerrainLists {
    fn invalidate(&mut self) {
        self.units_stale = true;
        self.visible_models = None;
        self.drawable_built = false;
        self.indirect_built = false;
        self.cube_view = None;
    }

    /// Shares allocation descriptors until the same tracker that owns the drawable lists stales.
    pub(in crate::chunk) fn units(
        &mut self,
        allocations: &Query<&GpuChunkAllocation>,
    ) -> Arc<super::units::sources::UnitSources> {
        if self.units_stale || self.units.is_none() {
            self.units = Some(Arc::new(super::units::sources::UnitSources::build(
                &self.drawable,
                allocations,
                self.units.as_deref(),
            )));
            self.units_stale = false;
        }
        Arc::clone(self.units.as_ref().unwrap())
    }

    /// The visible model workload, summarised from the visible set only after the lists staled.
    pub(in crate::chunk) fn visible_models(
        &mut self,
        summarize: impl FnOnce(&[(Entity, MainEntity)]) -> ModelWorkloadCount,
    ) -> ModelWorkloadCount {
        let Self {
            source,
            visible_models,
            ..
        } = self;
        *visible_models.get_or_insert_with(|| summarize(source))
    }

    /// The drawable visible terrain in Bevy's order, admitted again only after the lists staled.
    pub(in crate::chunk) fn drawable(
        &mut self,
        mut admit: impl FnMut(Entity, MainEntity) -> Option<DrawableTerrain>,
    ) -> &[DrawableTerrain] {
        if !self.drawable_built {
            #[cfg(feature = "tracy")]
            let _span =
                bevy::log::info_span!("terrain.batch_lists", visible = self.source.len()).entered();
            let Self {
                source, drawable, ..
            } = self;
            drawable.clear();
            drawable.extend(
                source
                    .iter()
                    .filter_map(|&(entity, main)| admit(entity, main)),
            );
            self.drawable_built = true;
        }
        &self.drawable
    }

    /// The indirect lists that changed since the view's batches were queued. `requeue` rebuilds
    /// them all; otherwise only a moved view sorts its cubes again, and an unchanged order keeps
    /// the queued batches and their commands. Call after [`Self::drawable`].
    pub(in crate::chunk) fn indirect_lists(
        &mut self,
        world_from_view: Mat4,
        rangefinder: &ViewRangefinder3d,
        requeue: bool,
    ) -> IndirectListUpdate {
        let rebuilt = requeue || !self.indirect_built;
        if rebuilt {
            self.by_entity = sorted_visible_entities(
                self.drawable
                    .iter()
                    .map(|terrain| (terrain.entity, terrain.key)),
            );
            self.indirect_built = true;
        }
        let mut cubes = None;
        let sorted = rebuilt || self.cube_view != Some(world_from_view);
        if sorted {
            #[cfg(feature = "tracy")]
            let _span =
                bevy::log::info_span!("terrain.cube_order", cubes = self.by_entity.len()).entered();
            self.cube_view = Some(world_from_view);
            let order = front_to_back_cube_entities(self.by_entity.iter().copied(), rangefinder);
            if rebuilt || order != self.cubes {
                self.cubes.clone_from(&order);
                cubes = Some(order);
            }
        }
        IndirectListUpdate {
            visible: rebuilt.then(|| self.by_entity.iter().map(|(entity, _)| *entity).collect()),
            cubes,
            sorted,
        }
    }
}

/// Records allocation and tint-table changes for the next opaque queue, which may not run
/// every frame.
pub(in crate::chunk) fn track_terrain_batch_changes(
    mut retained: ResMut<RetainedTerrainBatches>,
    changed: Query<(Entity, Option<&MainEntity>), Changed<GpuChunkAllocation>>,
    mut removed: RemovedComponents<GpuChunkAllocation>,
    mains: Query<&MainEntity>,
    biome_tints: Res<ChunkBiomeTints>,
) {
    let retained = &mut *retained;
    let tint = biome_tints.table_identity();
    if retained.tint.replace(tint).is_some_and(|old| old != tint) {
        retained.changes.invalidate_all();
    }
    for (entity, main) in &changed {
        retained.changes.record(entity, main.copied());
    }
    for entity in removed.read() {
        retained
            .changes
            .record(entity, mains.get(entity).ok().copied());
    }
}

/// Drops batches whose views no longer retain CPU-planned commands, including across GPU probes.
/// Returns whether any were dropped.
pub(in crate::chunk) fn retain_indirect_views<T>(
    batches: &mut HashMap<Entity, T>,
    views: &[Entity],
) -> bool {
    let before = batches.len();
    batches.retain(|view, _| views.contains(view));
    batches.len() != before
}

/// Builds the stale passes' indirect commands and lays every batch into the shared buffer.
/// With nothing stale, the uploaded commands and offsets stand as they are.
#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_chunk_indirect_batches(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    allocations: Query<&GpuChunkAllocation>,
    biome_tints: Res<ChunkBiomeTints>,
    frame_probe: Res<ActiveFrameProbe>,
    mut solid_batches: ResMut<super::solid::ChunkSolidIndirectBatches>,
    mut batches: ResMut<ChunkIndirectBatches>,
    mut model_batches: ResMut<ChunkModelIndirectBatches>,
    mut depth_liquid_batches: ResMut<ChunkDepthLiquidIndirectBatches>,
    mut arena: ResMut<ChunkGpuArena>,
    mut retained: ResMut<RetainedTerrainBatches>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::IndirectPreparation));
    // A probe admits allocations by its own manifest; such commands are never reused.
    let probing = frame_probe.is_active();
    for batch in solid_batches.0.values_mut() {
        let facing = batch.camera.and_then(meshing::facing_cell);
        batch.cubes.stale |= probing || batch.facing != facing;
        batch.facing = facing;
    }
    for batch in batches
        .0
        .values_mut()
        .chain(model_batches.0.values_mut())
        .chain(depth_liquid_batches.0.values_mut())
    {
        batch.stale |= probing;
    }
    let total = solid_batches.0.len()
        + batches.0.len()
        + model_batches.0.len()
        + depth_liquid_batches.0.len();
    let stale = solid_batches
        .0
        .values()
        .filter(|batch| batch.cubes.stale)
        .count()
        + batches
            .0
            .values()
            .chain(model_batches.0.values())
            .chain(depth_liquid_batches.0.values())
            .filter(|batch| batch.stale)
            .count();
    let retained = &mut *retained;
    retained.stats.batches_rebuilt += stale as u64;
    retained.stats.batches_retained += (total - stale) as u64;
    let buffer = arena.indirect_buffer.id();
    if stale == 0 && !retained.layout_changed && retained.uploaded_buffer == Some(buffer) {
        return;
    }
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!(
        "terrain.indirect_batches",
        rebuilt = stale,
        retained = total - stale
    )
    .entered();

    let frame_probe = frame_probe.scope();
    let tint_identity = biome_tints.table_identity();
    for batch in solid_batches.0.values_mut() {
        if batch.cubes.stale {
            let draws = super::solid::prepare_solid_indirect_batch_draws(
                resident(&batch.cubes.visible_entities, &allocations),
                batch.camera,
                &frame_probe,
                tint_identity,
            );
            store(&mut batch.cubes, draws, probing);
        }
    }
    for batch in batches.0.values_mut().filter(|batch| batch.stale) {
        let draws = prepare_indirect_batch_draws(
            resident(&batch.visible_entities, &allocations),
            &frame_probe,
            tint_identity,
        );
        store(batch, draws, probing);
    }
    for batch in model_batches.0.values_mut().filter(|batch| batch.stale) {
        let draws = prepare_model_indirect_batch_draws(
            resident(&batch.visible_entities, &allocations),
            &frame_probe,
            tint_identity,
        );
        store(batch, draws, probing);
    }
    for batch in depth_liquid_batches
        .0
        .values_mut()
        .filter(|batch| batch.stale)
    {
        let draws = prepare_depth_liquid_indirect_batch_draws(
            resident(&batch.visible_entities, &allocations),
            &frame_probe,
            tint_identity,
        );
        store(batch, draws, probing);
    }

    // Solid runs go first so the alpha-tested cutout draws meet a filled depth buffer.
    let mut all_commands = std::mem::take(&mut retained.commands);
    all_commands.clear();
    for batch in solid_batches
        .0
        .values_mut()
        .map(|batch| &mut batch.cubes)
        .chain(batches.0.values_mut())
        .chain(model_batches.0.values_mut())
        .chain(depth_liquid_batches.0.values_mut())
    {
        append_indirect_batch(batch, &mut all_commands);
    }
    if !all_commands.is_empty() {
        upload_indirect_commands_if_changed(
            &mut arena,
            &render_device,
            &render_queue,
            &all_commands,
        );
    }
    retained.uploaded_buffer = Some(arena.indirect_buffer.id());
    retained.layout_changed = false;
    retained.commands = all_commands;
}

fn resident<'a>(
    entities: &'a [Entity],
    allocations: &'a Query<&GpuChunkAllocation>,
) -> impl Iterator<Item = (Entity, &'a GpuChunkAllocation)> {
    entities
        .iter()
        .filter_map(|&entity| allocations.get(entity).ok().map(|item| (entity, item)))
}

/// Keeps freshly built commands; a probe's stay stale so the next frame builds them again.
fn store(
    batch: &mut ChunkIndirectBatch,
    (commands, drawn): (
        Vec<DrawIndexedIndirectArgs>,
        Vec<(Entity, FrameAllocationIdentity)>,
    ),
    probing: bool,
) {
    batch.commands = commands;
    batch.drawn_allocations = drawn;
    batch.stale = probing;
}

fn append_indirect_batch(
    batch: &mut ChunkIndirectBatch,
    all_commands: &mut Vec<DrawIndexedIndirectArgs>,
) {
    batch.indirect_offset = all_commands.len() as u64 * INDEXED_INDIRECT_BYTES;
    let Ok(command_count) = u32::try_from(batch.commands.len()) else {
        batch.command_count = 0;
        return;
    };
    batch.command_count = command_count;
    all_commands.extend_from_slice(&batch.commands);
}

#[cfg(test)]
mod tests;
