//! Publishes near admission and the far mask before any terrain draw lists are built.

use super::*;
use far_terrain::{ColumnPos, NearMask, TerrainPublication};
use render_scene::FarTerrainScene;

#[cfg(test)]
mod tests;

#[derive(Resource, Default)]
pub(crate) struct TerrainCoverage {
    pub(crate) state: TerrainPublication,
    world: Option<render_scene::FarTerrainResidency>,
    source: Option<render_scene::terrain_publication::TerrainPublications>,
    source_revision: Option<u64>,
    enabled: bool,
    mask: Option<Arc<NearMask>>,
    applied_target: Option<Arc<far_terrain::TerrainTarget>>,
    far_revision: u64,
    dirty: bool,
    /// Set by a publication against a far target, which may hide allocations; cleared by
    /// the first walk without one, which admits them all again.
    hiding: bool,
}

impl TerrainCoverage {
    /// Marks actual GPU publication changes without rebuilding unchanged per-frame snapshots.
    pub(crate) fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Invalidates empty proofs only after full geometry has actually published on the GPU.
    pub(crate) fn publish_full(&mut self, key: SubChunkKey, generation: u64) {
        if let Some(source) = &self.source {
            source.record(key, generation, false);
        }
        self.invalidate();
    }

    /// The mask uploaded by the same publication that admitted allocation components.
    #[cfg(feature = "publication-test-support")]
    pub(crate) fn mask(&self) -> Option<&NearMask> {
        self.mask.as_deref()
    }
}

/// Starts a separate publication epoch before this world's GPU completions are accepted.
pub(super) fn begin_terrain_coverage(
    scene: Option<Res<FarTerrainScene>>,
    mut coverage: ResMut<TerrainCoverage>,
    arena: Res<ChunkGpuArena>,
    gpu: Option<ResMut<crate::far_terrain::FarTerrainGpu>>,
) {
    let enabled = scene.as_ref().filter(|scene| scene.target.is_some());
    let source = scene.as_ref().and_then(|scene| scene.publications.as_ref());
    let same_world = match (source, coverage.source.as_ref()) {
        (Some(source), Some(previous)) => source.same_source(previous),
        (None, None) => match (enabled, coverage.world.as_ref()) {
            (Some(scene), Some(world)) => world.same_world(&scene.residency),
            (None, None) => true,
            _ => false,
        },
        _ => false,
    };
    if !same_world {
        coverage.state = TerrainPublication::default();
        coverage.source = source.cloned();
        coverage.source_revision = None;
    }
    if !same_world || coverage.enabled != enabled.is_some() {
        coverage.state.reset_draws();
        coverage.mask = None;
        coverage.applied_target = None;
        coverage.dirty = true;
    }
    coverage.enabled = enabled.is_some();
    coverage.world = enabled.map(|scene| scene.residency.clone());
    if let Some(mut gpu) = gpu {
        let protecting = coverage.state.far_columns().next().is_some();
        gpu.protect_coverage(
            arena
                .allocations
                .values()
                .take(if protecting { usize::MAX } else { 0 })
                .map(|allocation| allocation.gpu.key.chunk())
                .filter(|column| {
                    enabled.is_some_and(|scene| {
                        scene
                            .target
                            .as_ref()
                            .is_some_and(|target| target.contains(*column))
                    }) && !coverage.state.draws_near(*column)
                })
                .chain(coverage.state.far_columns().filter(|column| {
                    enabled.is_some_and(|scene| {
                        scene.target.as_ref().is_some_and(|target| {
                            target.contains(*column) && target.near.contains_key(column)
                        })
                    })
                }))
                .map(|column| ColumnPos::new(column.x, column.z)),
        );
    }
}

/// Admits or hides every section of a column and uploads its matching far mask together.
pub(super) fn publish_terrain_coverage(
    mut commands: Commands,
    scene: Option<Res<FarTerrainScene>>,
    mut coverage: ResMut<TerrainCoverage>,
    arena: Res<ChunkGpuArena>,
    allocations: Query<(), With<GpuChunkAllocation>>,
    gpu: Option<ResMut<crate::far_terrain::FarTerrainGpu>>,
    (device, queue): (Res<RenderDevice>, Res<RenderQueue>),
) {
    let target = scene.as_ref().and_then(|scene| scene.target.as_deref());
    let same_target = match (
        scene.as_ref().and_then(|scene| scene.target.as_ref()),
        coverage.applied_target.as_ref(),
    ) {
        (Some(target), Some(applied)) => Arc::ptr_eq(target, applied),
        (None, None) => true,
        _ => false,
    };
    if let Some((revision, empty)) = coverage.source.as_ref().and_then(|source| {
        source.changed_empty(if same_target {
            coverage.source_revision
        } else {
            None
        })
    }) {
        coverage.state.replace_empty(empty);
        coverage.source_revision = Some(revision);
        coverage.dirty = true;
    }
    let far_revision = gpu.as_ref().map_or(0, |gpu| gpu.publication_revision());
    if !coverage.dirty && same_target && coverage.far_revision == far_revision {
        return;
    }
    if let (Some(scene), Some(target), Some(mut gpu)) = (scene.as_ref(), target, gpu) {
        let live = arena
            .allocations
            .iter()
            .filter(|(entity, _)| !arena.pending_removals.contains(entity))
            .map(|(_, allocation)| (allocation.gpu.key, allocation.gpu.generation))
            .collect();
        let changed = coverage.state.publish(target, &live, |column| {
            gpu.covers_column(ColumnPos::new(column.x, column.z))
        });
        let view_changed = coverage.applied_target.as_ref().is_none_or(|applied| {
            applied.centre != target.centre || applied.horizon != target.horizon
        });
        if changed || view_changed || coverage.mask.is_none() {
            let mask = NearMask::from_columns(
                [target.centre.x, target.centre.z],
                target.horizon,
                coverage
                    .state
                    .near_columns()
                    .filter(|column| column.dimension == target.centre.dimension)
                    .map(|column| ColumnPos::new(column.x, column.z)),
            );
            if coverage.mask.as_deref() != Some(&mask) {
                let mask = Arc::new(mask);
                let _ = gpu.publish_mask(&device, &queue, Arc::clone(&mask));
                coverage.mask = Some(mask);
            }
            scene.residency.publish_near(
                coverage
                    .state
                    .near_columns()
                    .filter(|column| column.dimension == target.centre.dimension)
                    .map(|column| ColumnPos::new(column.x, column.z)),
            );
        }
    }
    // Allocation components are the common input of opaque, unit, indirect, GPU-cull and
    // transparent queues. Physical arena ownership and retirement remain unchanged. Upload
    // gives every new allocation its component, so without a far target, once a walk has
    // admitted whatever an earlier target hid, nothing needs admitting: the walk is skipped.
    if target.is_some() || coverage.hiding {
        for (&entity, allocation) in &arena.allocations {
            if arena.pending_removals.contains(&entity) {
                continue;
            }
            let admitted =
                target.is_none() || coverage.state.draws_near(allocation.gpu.key.chunk());
            if admitted != allocations.contains(entity)
                && let Ok(mut entity) = commands.get_entity(entity)
            {
                if admitted {
                    entity.insert(allocation.gpu.clone());
                } else {
                    entity.remove::<GpuChunkAllocation>();
                }
            }
        }
    }
    coverage.hiding = target.is_some();
    coverage.dirty = false;
    coverage.far_revision = far_revision;
    coverage.applied_target = scene.as_ref().and_then(|scene| scene.target.clone());
}
