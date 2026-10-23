//! Builds an isolated GPU arena; no visible allocation changes until the atlas is published.

use crate::chunk::*;
use bevy::ecs::system::RunSystemOnce;

use render_scene::{ChunkRenderInstance, ChunkTextureAssets, ChunkUploadAcknowledgements};

pub(in crate::chunk) struct PreparedResourceGeometry {
    arena: ChunkGpuArena,
    liquids: TransparentSortRuntime,
    models: TransparentModelSortRuntime,
}

impl PreparedResourceGeometry {
    /// Uses the ordinary validated uploader against an invisible, independently owned arena.
    pub(in crate::chunk) fn build(
        instances: &[ChunkRenderInstance],
        assets: ChunkTextureAssets,
        device: RenderDevice,
        queue: RenderQueue,
        view: Option<super::resource_sorts::ResourceView>,
    ) -> Option<Self> {
        crate::gpu_allocation::isolated_terrain(|| {
            Self::build_generation(instances, assets, device, queue, view)
        })
    }

    /// Rejects the whole candidate when any allocation, geometry upload or sort upload fails.
    fn build_generation(
        instances: &[ChunkRenderInstance],
        assets: ChunkTextureAssets,
        device: RenderDevice,
        queue: RenderQueue,
        view: Option<super::resource_sorts::ResourceView>,
    ) -> Option<Self> {
        let mut app = App::new();
        let mut tints = ChunkBiomeTints::default();
        if let Some(instance) = instances.first() {
            tints.identity = instance.tint_identity;
        }
        app.insert_resource(ChunkGpuArena::new(&device))
            .insert_resource(device)
            .insert_resource(queue)
            .insert_resource(assets)
            .insert_resource(ChunkUploadBudget::new(usize::MAX, u64::MAX))
            .init_resource::<ChunkGpuUploadStats>()
            .insert_resource(tints)
            .init_resource::<ChunkUploadAcknowledgements>()
            .init_resource::<ChunkGpuRemovalQueue>()
            .init_resource::<TransparentRetirementFence>()
            .init_resource::<GpuUpdateFairness>();
        for instance in instances {
            let mut instance = instance.clone();
            instance.token = None;
            instance.publication_permit = None;
            app.world_mut().spawn(instance);
        }
        let expected = instances
            .iter()
            .filter(|instance| {
                !instance.cube_quads.is_empty()
                    || !instance.model_refs.is_empty()
                    || !instance.liquid_quads.is_empty()
            })
            .count();
        loop {
            let before = {
                let arena = app.world().resource::<ChunkGpuArena>();
                (
                    arena.allocations.len(),
                    arena.migration.as_ref().map(|m| m.copied_bytes),
                )
            };
            app.world_mut().run_system_once(prepare_gpu_chunks).ok()?;
            let arena = app.world().resource::<ChunkGpuArena>();
            if arena.allocations.len() == expected {
                break;
            }
            let after = (
                arena.allocations.len(),
                arena.migration.as_ref().map(|m| m.copied_bytes),
            );
            if before == after {
                return None;
            }
        }
        let (liquids, models) = super::resource_sorts::prepare(&mut app, view)?;
        Some(Self {
            liquids,
            models,
            arena: app.world_mut().remove_resource::<ChunkGpuArena>()?,
        })
    }

    /// Checks the live entity set and physical retirement without consuming staged resources.
    pub(in crate::chunk) fn ready(
        &self,
        instances: &Query<(Entity, &ChunkRenderInstance)>,
        active: &ChunkGpuArena,
    ) -> bool {
        if !active.retired_allocations.is_empty() {
            return false;
        }
        let keys: HashSet<_> = instances.iter().map(|(_, instance)| instance.key).collect();
        let staged: HashSet<_> = self
            .arena
            .allocations
            .values()
            .map(|allocation| allocation.gpu.key)
            .collect();
        staged.iter().all(|key| keys.contains(key))
            && active.allocations.iter().all(|(entity, allocation)| {
                (!active.pending_removals.contains(entity) && keys.contains(&allocation.gpu.key))
                    || staged.contains(&allocation.gpu.key)
            })
            && self.models.committed.as_ref().is_none_or(|committed| {
                committed
                    .address
                    .allocations
                    .iter()
                    .all(|allocation| keys.contains(&allocation.key))
            })
    }

    /// Rebinds a complete swap, returning the intact candidate when retirement still blocks it.
    #[allow(clippy::result_large_err)]
    pub(in crate::chunk) fn publish(
        mut self,
        commands: &mut Commands,
        instances: &Query<(Entity, &ChunkRenderInstance)>,
        active: &mut ChunkGpuArena,
        mut coverage: Option<&mut TerrainCoverage>,
    ) -> Result<(), Self> {
        if !self.ready(instances, active) {
            return Err(self);
        }
        let mut by_key: HashMap<_, _> = self
            .arena
            .allocations
            .drain()
            .map(|(_, allocation)| (allocation.gpu.key, allocation))
            .collect();
        for (entity, instance) in instances {
            if let Some(allocation) = by_key.remove(&instance.key) {
                if active.pending_removals.contains(&entity) {
                    self.arena.pending_removals.insert(entity);
                    commands.entity(entity).remove::<GpuChunkAllocation>();
                } else {
                    commands.entity(entity).insert(allocation.gpu.clone());
                    if let Some(coverage) = coverage.as_deref_mut() {
                        coverage.publish_full(allocation.gpu.key, allocation.gpu.generation);
                    }
                }
                self.arena.allocations.insert(entity, allocation);
            } else {
                commands.entity(entity).remove::<GpuChunkAllocation>();
            }
        }
        self.arena.transparent_liquids.rebuild(
            self.arena
                .allocations
                .iter()
                .map(|(&entity, allocation)| (entity, &allocation.gpu)),
        );
        let keys: HashMap<_, _> = instances
            .iter()
            .map(|(entity, instance)| (instance.key, entity))
            .collect();
        self.models.draw_orders.remap_entities(&keys);
        if let Some(committed) = self.models.committed.as_mut() {
            let mut allocations = committed.address.allocations.to_vec();
            for allocation in &mut allocations {
                allocation.entity = keys[&allocation.key];
            }
            committed.address.allocations = allocations.into();
        }
        *active = self.arena;
        if let Some(coverage) = coverage {
            coverage.invalidate();
        }
        commands.insert_resource(self.liquids);
        commands.insert_resource(self.models);
        commands.insert_resource(GpuUpdateFairness::default());
        Ok(())
    }
}

#[cfg(test)]
#[path = "resource_geometry_tests.rs"]
mod tests;
