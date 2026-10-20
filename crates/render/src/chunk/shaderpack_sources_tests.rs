use super::*;
use bevy::ecs::system::RunSystemOnce;
use meshing::{ChunkBiomeTintIdentity, PackedBiomeRecord};
use render_scene::ChunkUploadPriority;
use std::sync::Arc;

/// One empty packed section and the allocation uploaded from it.
fn section(tint: ChunkBiomeTintIdentity) -> (ChunkRenderInstance, GpuChunkAllocation) {
    let key = world::SubChunkKey::new(0, 1, 2, 3);
    let instance = ChunkRenderInstance {
        cube_layout: CubeQuadLayout::default(),
        key,
        cube_quads: Arc::from([]),
        cube_lighting: Arc::from([]),
        model_refs: Arc::from([]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
        transparent_model_draw_refs: Arc::from([]),
        liquid_quads: Arc::from([]),
        liquid_lighting: Arc::from([]),
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: tint,
        generation: 6,
        priority: ChunkUploadPriority::new(0.0),
        token: None,
        publication_permit: None,
        origin: [16, 32, 48],
    };
    let allocation = GpuChunkAllocation {
        cube_layout: instance.cube_layout,
        key,
        generation: instance.generation,
        tint_identity: tint,
        quad_range: 0..0,
        cube_lighting_range: None,
        model_range: None,
        model_lighting_range: None,
        model_draw_range: None,
        transparent_model_draw_range: None,
        liquid_range: None,
        liquid_lighting_range: None,
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_range: None,
        order_independent_liquid: false,
        metadata_index: 7,
    };
    (instance, allocation)
}

#[test]
fn expansion_requires_matching_instance_allocation_and_active_tint_tables() {
    let old = ChunkBiomeTintIdentity::new(4, 5);
    let new = ChunkBiomeTintIdentity::new(4, 6);
    for (instance_tint, allocation_tint, active, accepted) in [
        (old, old, old, true),
        (old, old, new, false),
        (old, new, new, false),
        (new, old, new, false),
        (new, new, old, false),
        (new, new, new, true),
    ] {
        let mut world = World::new();
        let (instance, mut allocation) = section(instance_tint);
        allocation.tint_identity = allocation_tint;
        let entity = world.spawn((instance, allocation)).id();
        world
            .run_system_once(move |sections: ResidentSections| {
                assert_eq!(
                    sections.get(entity, active).is_some(),
                    accepted,
                    "instance {instance_tint:?}, allocation {allocation_tint:?}, active {active:?}"
                );
            })
            .unwrap();
    }
}
