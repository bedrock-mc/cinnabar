//! Rebuilds derived terrain state after an allocation failure, keeping allocated storage.

use super::*;
use crate::queue_writes::ProfiledQueue as _;

/// Backoff belongs to the failed generation, independent of its entities.
struct Retry {
    wait: u32,
    release_revision: u64,
    budget: render_scene::ChunkUploadBudget,
}

impl Retry {
    /// Starts the next generation when backoff expires or its memory conditions improve.
    fn ready(&mut self, release_revision: u64, budget: render_scene::ChunkUploadBudget) -> bool {
        if release_revision != self.release_revision || budget != self.budget {
            self.wait = 0;
        }
        if self.wait == 0 {
            return true;
        }
        self.wait -= 1;
        false
    }
}

#[derive(Resource)]
pub(in crate::chunk) struct TerrainGpuRecovery {
    failures: crate::gpu_allocation::FailureFlag,
    retry: Option<Retry>,
    attempts: u32,
    generation: u64,
    /// Keeps growth minimal until the entire rebuilt generation is ready.
    pub(in crate::chunk) recovering: bool,
}

impl TerrainGpuRecovery {
    /// Observes live terrain failures; invisible candidates have their own failure scope.
    pub(in crate::chunk) fn new(device: &RenderDevice) -> Self {
        Self {
            failures: crate::gpu_allocation::FailureFlag::new(device),
            retry: None,
            attempts: 0,
            generation: 0,
            recovering: false,
        }
    }

    /// Captures failure conditions before the next frame can change its upload budget.
    fn backoff(&mut self, release_revision: u64, budget: render_scene::ChunkUploadBudget) {
        if self.retry.is_some() {
            return;
        }
        self.attempts = self.attempts.saturating_add(1);
        self.retry = Some(Retry {
            wait: crate::gpu_allocation::retry_frames(self.attempts),
            release_revision,
            budget,
        });
    }

    /// Keeps uploads paused until the failed generation's retry becomes due.
    pub(in crate::chunk) fn waiting(&self) -> bool {
        self.failures.failed()
    }
}

/// Forgets every derived publication together; CPU instances remain the source of truth.
pub(in crate::chunk) fn begin_recovery(world: &mut World) {
    if !world.resource::<TerrainGpuRecovery>().failures.failed() {
        return;
    }
    let arena = world.resource::<ChunkGpuArena>();
    let release_revision = arena.release_revision;
    let budget = *world.resource::<render_scene::ChunkUploadBudget>();
    {
        let mut recovery = world.resource_mut::<TerrainGpuRecovery>();
        recovery.backoff(release_revision, budget);
        if !recovery
            .retry
            .as_mut()
            .unwrap()
            .ready(release_revision, budget)
        {
            return;
        }
        recovery.retry = None;
        recovery.failures.take();
        recovery.recovering = true;
        recovery.generation += 1;
        bevy::log::debug!(
            generation = recovery.generation,
            "rebuilding terrain GPU generation"
        );
    }
    let profiler = world
        .get_resource::<render_scene::RuntimeStageProfiler>()
        .cloned();
    let _timer = profiler
        .as_ref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::GpuPreparation));
    let entities = world
        .query_filtered::<Entity, With<GpuChunkAllocation>>()
        .iter(world)
        .collect::<Vec<_>>();
    for entity in entities {
        world.entity_mut(entity).remove::<GpuChunkAllocation>();
    }
    world
        .resource_mut::<ChunkGpuArena>()
        .invalidate_publication();
    *world.resource_mut::<GpuUpdateFairness>() = GpuUpdateFairness::default();
    *world.resource_mut::<coverage::TerrainCoverage>() = coverage::TerrainCoverage::default();
    {
        let mut retained = world.resource_mut::<pipeline::batches::RetainedTerrainBatches>();
        let stats = retained.stats;
        *retained = default();
        retained.stats = stats;
    }
    *world.resource_mut::<pipeline::solid::ChunkSolidIndirectBatches>() = default();
    *world.resource_mut::<ChunkIndirectBatches>() = default();
    *world.resource_mut::<ChunkModelIndirectBatches>() = default();
    *world.resource_mut::<ChunkDepthLiquidIndirectBatches>() = default();
    world
        .resource_mut::<TransparentSortRuntime>()
        .reset_for_view(None);
    let next_generation = world
        .resource::<TransparentModelSortRuntime>()
        .next_generation;
    *world.resource_mut::<TransparentModelSortRuntime>() = default();
    world
        .resource_mut::<TransparentModelSortRuntime>()
        .next_generation = next_generation;
    *world.resource_mut::<TransparentRetirementFence>() = default();
    *world.resource_mut::<TransparentPresentationFence>() = default();
    if let Some(mut units) = world.get_resource_mut::<pipeline::units::TerrainUnitBatches>() {
        units.invalidate_publication();
    }
    gpu_cull::invalidate_publication(world);
    if let Some(mut far) = world.get_resource_mut::<crate::far_terrain::FarTerrainGpu>() {
        far.invalidate_publication();
    }
    #[cfg(feature = "shaderpacks")]
    if let Some(mut geometry) =
        world.get_resource_mut::<crate::shaderpack::geometry::ShaderpackGeometry>()
    {
        geometry.invalidate_publication();
    }
    let device = world.resource::<RenderDevice>();
    let queue = world.resource::<RenderQueue>();
    let result = crate::gpu_allocation::terrain_write(
        device,
        "terrain recovery biome upload",
        std::mem::size_of_val(&FALLBACK_BIOME_RECORD) as u64,
        || {
            queue.profiled_write_buffer(
                &world.resource::<ChunkGpuArena>().biome_buffer,
                0,
                bytemuck::cast_slice(&FALLBACK_BIOME_RECORD),
            )
        },
    );
    if result.is_err() {
        bevy::log::debug!("terrain generation fallback upload failed");
    }
}

/// Prevents a partially prepared frame from drawing or witnessing terrain. Recovery retries
/// the entire generation after backoff, regardless of which owner observed the failure.
pub(in crate::chunk) fn finish_publication(world: &mut World) {
    let pending = world.resource::<ChunkGpuUploadStats>().candidates_deferred != 0;
    let release_revision = world.resource::<ChunkGpuArena>().release_revision;
    let budget = *world.resource::<render_scene::ChunkUploadBudget>();
    let mut recovery = world.resource_mut::<TerrainGpuRecovery>();
    if !recovery.failures.failed() && (!recovery.recovering || !pending) {
        recovery.recovering = false;
        recovery.attempts = 0;
        return;
    }
    if recovery.failures.failed() {
        recovery.backoff(release_revision, budget);
    }
    let mut arena = world.resource_mut::<ChunkGpuArena>();
    arena.bind_group = None;
    arena.transparent_bind_group = None;
    gpu_cull::suppress_publication(world);
    crate::far_terrain::suppress_publication(world);
    #[cfg(feature = "shaderpacks")]
    if let Some(mut geometry) =
        world.get_resource_mut::<crate::shaderpack::geometry::ShaderpackGeometry>()
    {
        geometry.invalidate_publication();
    }
}

impl ChunkGpuArena {
    /// Reuses physical buffers after all previous frame commands have been submitted.
    fn invalidate_publication(&mut self) {
        if !self.allocations.is_empty() || self.migration.is_some() {
            self.growth_blocked = false;
        }
        self.allocations.clear();
        self.transparent_liquids = default();
        self.retired_allocations.clear();
        self.retirement_budget.items = 0;
        self.retirement_budget.bytes = 0;
        self.migration = None;
        self.quad_len = 0;
        self.geometry_stream_len = 0;
        self.origin_len = 0;
        self.biome_len = FALLBACK_BIOME_WORDS;
        self.free_quads.clear();
        self.free_geometry_stream_words.clear();
        self.free_origins.clear();
        self.free_biomes.clear();
        self.shared_biome_ranges = default();
        self.uploaded_indirect_bytes.clear();
        self.bind_group = None;
        self.transparent_bind_group = None;
        self.bind_group_buffers = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A release or budget change shortens one wait without erasing the next failure's backoff.
    #[test]
    fn changed_conditions_shorten_only_the_failed_generation() {
        let budget = render_scene::ChunkUploadBudget::new(0, 0);
        for (revision, changed) in [(1, budget), (0, render_scene::ChunkUploadBudget::default())] {
            let mut retry = Retry {
                wait: crate::gpu_allocation::retry_frames(u32::MAX),
                release_revision: 0,
                budget,
            };
            assert!(!retry.ready(0, budget));
            assert!(retry.ready(revision, changed));
            let mut next = Retry {
                wait: 4,
                release_revision: revision,
                budget: changed,
            };
            assert!(!next.ready(revision, changed));
        }
    }
}
