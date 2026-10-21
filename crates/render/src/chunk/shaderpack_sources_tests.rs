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

/// Builds two model planes with distinct centroids and sidedness, without local carriers.
fn model_assets() -> ChunkTextureAssets {
    let mut visual = assets::BlockVisual::diagnostic(
        assets::BlockFlags::empty(),
        assets::ContributorRole::Primary,
    );
    visual.kind = assets::VisualKind::Model;
    visual.support = assets::VisualSupport::Exact;
    visual.model_template = 0;
    let overlay = assets::BlockOverlay {
        visuals: vec![visual],
        light_properties: vec![assets::LightProperties::default()],
        materials: vec![assets::Material {
            texture: assets::TextureRef::DIAGNOSTIC,
            animation: assets::NO_ANIMATION,
            ..assets::Material::unvaried()
        }],
        model_templates: vec![assets::ModelTemplate {
            quad_start: 0,
            quad_count: 2,
            flags: 0,
        }],
        model_quads: [0, 256]
            .into_iter()
            .map(|y| assets::ModelQuad {
                positions: [[0, y, 0], [256, y, 0], [256, y, 256], [0, y, 256]],
                uvs: [[0; 2]; 4],
                material: 0,
                flags: if y == 0 {
                    0
                } else {
                    assets::MODEL_QUAD_FLAG_TWO_SIDED
                },
            })
            .collect(),
        ..Default::default()
    };
    let assets = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &overlay)
        .unwrap();
    ChunkTextureAssets::with_revision(Arc::new(assets), 1)
}

/// Reordering uploaded draw refs cannot change expansion slots or their sorting centroids.
#[test]
fn expansion_model_jobs_do_not_depend_on_uploaded_draw_order() {
    use crate::chunk::transparent::face_metric::FaceOrderClass;
    use crate::shaderpack::geometry::jobs::{
        JOB_REVERSE_FACE, JobKind, MaterialTables, plan_section,
    };
    let tint = ChunkBiomeTintIdentity::new(4, 5);
    let (mut instance, mut allocation) = section(tint);
    instance.model_refs = Arc::from([PackedModelRef::new(0, 0, 0, 3)]);
    instance.model_lighting = Arc::from([meshing::PackedQuadLighting::default(); 2]);
    instance.transparent_model_draw_refs =
        Arc::from([PackedModelDrawRef::new(0, 0), PackedModelDrawRef::new(0, 1)]);
    allocation.model_range = Some(100..104);
    allocation.transparent_model_draw_range = Some(200..204);
    let mut world = World::new();
    let entity = world.spawn((instance, allocation.clone())).id();
    let identity = TransparentModelAllocationIdentity {
        entity,
        key: allocation.key,
        generation: allocation.generation,
        model_range: allocation.model_range.clone().unwrap(),
        draw_range: allocation.transparent_model_draw_range.clone().unwrap(),
    };
    let assets = model_assets();
    let address = TransparentModelAddressIdentity {
        asset_identity: assets.identity(),
        allocations: Arc::from([identity]),
        revision: 1,
        _retained_assets: Some(Arc::clone(assets.assets())),
    };
    let mut runtime = TransparentModelSortRuntime::default();
    runtime.draw_orders.publish(
        &address,
        TransparentModelSortBatch {
            draw_range: 200..204,
            class: FaceOrderClass::Far([0, 1, 0]),
            words: Box::new([[25, 1], [25, 0]]),
        },
    );
    world.insert_resource(runtime);
    world
        .run_system_once(move |sections: ResidentSections| {
            let section = sections.get(entity, tint).unwrap();
            assert_eq!(
                section
                    .geometry
                    .transparent_model_draw_refs
                    .first()
                    .unwrap()
                    .words(),
                [0, 0]
            );
            let plan = plan_section(
                &section.geometry,
                &MaterialTables {
                    materials: assets.assets().materials(),
                    model_templates: assets.assets().model_templates(),
                    model_quads: assets.assets().model_quads(),
                },
            );
            let jobs = &plan.jobs[JobKind::Model as usize];
            assert_eq!(jobs[0], [25, 7, 0, 0]);
            assert_eq!(jobs[1], [25, 7, 1, JOB_REVERSE_FACE | 1]);
            let faces = section.translucent_faces(&plan, &assets);
            assert_eq!(faces.len(), 3);
            assert_eq!(
                faces.iter().map(|face| face.stable).collect::<Vec<_>>(),
                [[0, 0], [0, 1], [0, 1]]
            );
            assert_eq!(faces[1].centroid, faces[2].centroid);
            assert_eq!(faces[1].centroid.y, faces[0].centroid.y + 1.0);
        })
        .unwrap();
}
