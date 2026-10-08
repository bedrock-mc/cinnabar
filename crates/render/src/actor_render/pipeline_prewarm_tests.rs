use std::collections::HashSet;

use assets::{EntityRenderMaterial, EntityRenderMaterialState};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::{Msaa, Mut},
    render::{render_resource::PipelineCache, view::ExtractedView},
};

use super::super::ActorPipeline;

#[test]
fn actor_pipeline_prewarm_empty_view_requests_pipelines_without_publishing_pending_work() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.init_resource::<crate::ActorPipelineReadiness>();
    world
        .run_system_once(super::super::prepare_actor_pipelines)
        .unwrap();
    let (&msaa, view) = world
        .query::<(&Msaa, &ExtractedView)>()
        .single(world)
        .unwrap();
    let pipeline = world.resource::<ActorPipeline>();
    let cache = world.resource::<PipelineCache>();
    assert!(
        pipeline
            .draw_variant(msaa, view.hdr, false, EntityRenderMaterial::Default as u32)
            .is_some()
    );
    assert!(!pipeline.ready(cache, msaa, view.hdr, false));
    assert!(!world.resource::<crate::ActorPipelineReadiness>().is_ready());
}

#[test]
fn actor_pipeline_prewarm_empty_frame_covers_all_authored_raster_states() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.init_resource::<crate::ActorRenderFrame>();
    assert!(
        world
            .resource::<crate::ActorRenderFrame>()
            .rig
            .instances
            .is_empty()
    );
    let (&msaa, view) = world
        .query::<(&Msaa, &ExtractedView)>()
        .single(world)
        .unwrap();
    let hdr = view.hdr;
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        pipeline
            .prewarm(cache, msaa, hdr, false)
            .expect("an empty view prepares actor pipelines");
        let mut descriptors = HashSet::new();
        let kinds = [
            EntityRenderMaterial::Default,
            EntityRenderMaterial::Glint,
            EntityRenderMaterial::Dragon,
            EntityRenderMaterial::DissolveDepth,
            EntityRenderMaterial::DissolveColor,
        ];
        for kind in kinds {
            for cull in [false, true] {
                for blend in [false, true] {
                    for depth_write in [false, true] {
                        for alpha_test in [false, true] {
                            let material = kind.word(Some(EntityRenderMaterialState {
                                alpha_test,
                                cull,
                                blend,
                                depth_write,
                                ..Default::default()
                            }));
                            descriptors.insert(
                                pipeline.draw_variant(msaa, hdr, false, material).expect(
                                    "authored raster states are requested before an actor exists",
                                ),
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(
            descriptors.len(),
            24,
            "only distinct raster/depth contracts compile"
        );
        for kind in kinds {
            for cull in [false, true] {
                for depth_write in [false, true] {
                    for additive_alpha in [false, true] {
                        let material = kind.word(Some(EntityRenderMaterialState {
                            cull,
                            depth_write,
                            blend: true,
                            additive: true,
                            additive_alpha,
                            ..Default::default()
                        }));
                        descriptors.insert(
                            pipeline
                                .draw_variant(msaa, hdr, false, material)
                                .expect("each additive kind is prepared before an actor exists"),
                        );
                    }
                }
            }
        }
        assert_eq!(
            descriptors.len(),
            48,
            "additive source factors add eight contracts per raster kind"
        );
    });
}

#[test]
fn actor_pipeline_prewarm_reuses_descriptors_for_shader_only_flags() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    let (&msaa, view) = world
        .query::<(&Msaa, &ExtractedView)>()
        .single(world)
        .unwrap();
    let hdr = view.hdr;
    let state = EntityRenderMaterialState::default();
    let normal = EntityRenderMaterial::Default.word(Some(state));
    let dragon = EntityRenderMaterial::Dragon.word(Some(EntityRenderMaterialState {
        alpha_test: true,
        ..state
    }));
    let emissive = EntityRenderMaterial::Default.word(Some(EntityRenderMaterialState {
        emissive: true,
        ..state
    }));
    assert_ne!(
        normal, dragon,
        "shader instances retain their full material words"
    );
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        pipeline.prewarm(cache, msaa, hdr, false).unwrap();
        assert_eq!(
            pipeline.draw_variant(msaa, hdr, false, normal).unwrap(),
            pipeline.draw_variant(msaa, hdr, false, dragon).unwrap(),
            "identical raster contracts share one compiled pipeline",
        );
        assert_eq!(
            pipeline.draw_variant(msaa, hdr, false, normal),
            pipeline.draw_variant(msaa, hdr, false, emissive),
            "emissive shader flags reuse the raster pipeline"
        );
    });
}
