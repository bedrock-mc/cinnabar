//! Builds an isolated GPU arena; no visible allocation changes until the atlas is published.
use crate::chunk::*;
use bevy::ecs::system::RunSystemOnce;

pub(in crate::chunk) struct PreparedResourceGeometry {
    arena: ChunkGpuArena,
    liquids: TransparentSortRuntime,
    models: TransparentModelSortRuntime,
}

impl PreparedResourceGeometry {
    /// Uses the ordinary validated uploader against an invisible, independently owned arena.
    pub(super) fn build(
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

    /// Rebinds all staged allocations to resident render entities before any draw preparation.
    pub(in crate::chunk) fn publish(
        mut self,
        commands: &mut Commands,
        instances: &Query<(Entity, &ChunkRenderInstance)>,
        active: &mut ChunkGpuArena,
    ) -> bool {
        let keys: HashMap<_, _> = instances
            .iter()
            .map(|(entity, instance)| (instance.key, entity))
            .collect();
        if self
            .arena
            .allocations
            .values()
            .any(|allocation| !keys.contains_key(&allocation.gpu.key))
            || self.models.committed.as_ref().is_some_and(|committed| {
                committed
                    .address
                    .allocations
                    .iter()
                    .any(|allocation| !keys.contains_key(&allocation.key))
            })
        {
            return false;
        }
        let mut by_key: HashMap<_, _> = self
            .arena
            .allocations
            .drain()
            .map(|(_, allocation)| (allocation.gpu.key, allocation))
            .collect();
        for (entity, instance) in instances {
            if let Some(allocation) = by_key.remove(&instance.key) {
                commands.entity(entity).insert(allocation.gpu.clone());
                self.arena.allocations.insert(entity, allocation);
            } else {
                commands.entity(entity).remove::<GpuChunkAllocation>();
            }
        }
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
        commands.insert_resource(self.liquids);
        commands.insert_resource(self.models);
        commands.insert_resource(GpuUpdateFairness::default());
        true
    }
}

#[cfg(test)]
#[path = "resource_geometry_tests.rs"]
mod tests;
