//! Keeps device geometry policy independent of the actor scene installed at startup.

use bevy::prelude::{Resource, World};
use render_scene::ActorRenderScene;

#[derive(Clone, Copy, Resource)]
struct ActorGeometryBudget {
    memory_tier: u32,
    binding_bytes: u64,
}

impl ActorGeometryBudget {
    /// Records the device policy and configures any scene installed before device startup.
    fn install(self, world: &mut World) {
        if let Some(mut scene) = world.get_resource_mut::<ActorRenderScene>() {
            scene.set_geometry_budget(self.memory_tier, self.binding_bytes);
        }
        world.insert_resource(self);
    }
}

/// Records the memory-tier budget within one storage binding of the initialized GPU.
pub(crate) fn initialize_actor_geometry_budget(world: &mut World) {
    let Some(device) = world.get_resource::<bevy::render::renderer::RenderDevice>() else {
        return;
    };
    let budget = ActorGeometryBudget {
        memory_tier: resource_pack::subpacks::device_memory_tier(
            world
                .get_resource::<crate::host_memory::HostMemory>()
                .map_or_else(
                    || crate::host_memory::HostMemory::default().0,
                    |memory| memory.0,
                ),
        ),
        binding_bytes: render::actor_storage_binding_bytes(device),
    };
    budget.install(world);
}

/// Installs an eager or deferred scene using the device policy when it is already available.
pub(crate) fn install_actor_scene(world: &mut World, mut scene: ActorRenderScene) {
    if let Some(budget) = world.get_resource::<ActorGeometryBudget>() {
        scene.set_geometry_budget(budget.memory_tier, budget.binding_bytes);
    }
    world.insert_resource(scene);
}

#[cfg(test)]
mod tests {
    use super::*;
    use render_model::{ActorRigGeometry, ActorRigVertex};

    /// Builds distinct geometry pages whose combined storage exceeds the baseline budget.
    fn large_catalog() -> Vec<ActorRigGeometry> {
        let count = render_model::MAX_ACTOR_CATALOG_VERTICES / 3 * 3 + 3;
        // Geometry validation requires whole triangles.
        let page_vertices = render_model::MAX_ACTOR_RIG_VERTICES / 3 * 3;
        (0..count)
            .step_by(page_vertices)
            .enumerate()
            .map(|(slot, start)| {
                ActorRigGeometry::new(
                    render_model::pack_rig_id(slot as u32),
                    vec![
                        ActorRigVertex {
                            position: [slot as f32 + 1.0; 3],
                            ..Default::default()
                        };
                        (count - start).min(page_vertices)
                    ],
                    vec![[0.0; 3]],
                )
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn eager_and_deferred_scenes_keep_the_large_device_geometry_budget() {
        let geometries = large_catalog();
        let mut baseline = ActorRenderScene::default();
        baseline
            .replace_pack_entity_geometries(geometries.clone())
            .unwrap();
        assert!(
            geometries
                .iter()
                .any(|rig| !baseline.contains_geometry(rig.id))
        );

        for deferred in [false, true] {
            let mut world = World::new();
            install_actor_scene(&mut world, ActorRenderScene::default());
            let memory_tier = resource_pack::subpacks::device_memory_tier(16 << 30);
            ActorGeometryBudget {
                memory_tier,
                binding_bytes: render_model::actor_catalog_budget_bytes(memory_tier) as u64,
            }
            .install(&mut world);
            if deferred {
                let mut queue = bevy::ecs::world::CommandQueue::default();
                queue.push(|world: &mut World| {
                    install_actor_scene(world, ActorRenderScene::default());
                });
                queue.apply(&mut world);
            }
            let mut scene = world.resource_mut::<ActorRenderScene>();
            scene
                .replace_pack_entity_geometries(geometries.clone())
                .unwrap();
            assert!(
                geometries.iter().all(|rig| scene.contains_geometry(rig.id)),
                "a fitting catalog must stay resident after scene installation (deferred={deferred})"
            );
        }
    }
}
