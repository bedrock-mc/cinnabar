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
fn always_depth_prewarm_covers_blend_and_depth_write_combinations() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        for msaa in [Msaa::Off, Msaa::Sample4] {
            for hdr in [false, true] {
                pipeline.prewarm(cache, msaa, hdr, false).unwrap();
                for kind in [
                    EntityRenderMaterial::Default,
                    EntityRenderMaterial::DissolveDepth,
                    EntityRenderMaterial::DissolveColor,
                ] {
                    for bits in 0..32 {
                        let state = EntityRenderMaterialState {
                            cull: bits & 1 != 0,
                            blend: bits & 2 != 0,
                            depth_write: bits & 4 != 0,
                            additive: bits & 8 != 0,
                            additive_alpha: bits & 16 != 0,
                            depth_always: true,
                            ..Default::default()
                        };
                        let material = kind.word(Some(state));
                        assert!(
                            pipeline.draw_variant(msaa, hdr, false, material).is_some(),
                            "always-depth material must have a draw pipeline: {kind:?}, {bits}"
                        );
                        if kind == EntityRenderMaterial::DissolveColor {
                            let paired = kind.word(Some(EntityRenderMaterialState {
                                depth_always: false,
                                ..state
                            })) | crate::actor::material::LATE_DISSOLVE_COLOR;
                            assert!(
                                pipeline.draw_variant(msaa, hdr, false, paired).is_some(),
                                "paired color must retain its depth contract: {bits}"
                            );
                        }
                    }
                }
            }
        }
    });
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
            28,
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
            52,
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
    let without_overlay = EntityRenderMaterial::Default.word(Some(EntityRenderMaterialState {
        disable_overlay: true,
        ..state
    }));
    assert_ne!(normal, without_overlay);
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
        assert_eq!(
            pipeline.draw_variant(msaa, hdr, false, normal),
            pipeline.draw_variant(msaa, hdr, false, without_overlay),
            "overlay admission reuses the raster pipeline"
        );
    });
}

#[test]
fn actor_coverage_variants_are_prewarmed_without_changing_blend_emissive_or_dissolve() {
    use super::super::{
        ActorPipelineKey, ActorPipelineSpecializer, actor_bind_group_layout,
        actor_pipeline_descriptor,
    };
    use bevy::render::render_resource::Specializer;

    let (mut app, _) = crate::queue_review_support::app();
    let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
    app.world_mut().init_resource::<ActorPipeline>();
    let mut pipeline = app.world_mut().remove_resource::<ActorPipeline>().unwrap();
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        pipeline.prewarm(&cache, msaa, false, false).unwrap();
        for kind in [
            EntityRenderMaterial::Default,
            EntityRenderMaterial::Glint,
            EntityRenderMaterial::Dragon,
            EntityRenderMaterial::DissolveDepth,
            EntityRenderMaterial::DissolveColor,
        ] {
            for alpha_test in [false, true] {
                for blend in [false, true] {
                    for emissive in [false, true] {
                        let material = kind.word(Some(EntityRenderMaterialState {
                            alpha_test,
                            blend,
                            emissive,
                            ..Default::default()
                        }));
                        let id = pipeline
                            .draw_variant(msaa, false, false, material)
                            .expect("coverage variants are ready before actors exist");
                        let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
                        ActorPipelineSpecializer
                            .specialize(
                                ActorPipelineKey {
                                    msaa,
                                    hdr: false,
                                    enhanced: false,
                                    material,
                                },
                                &mut descriptor,
                            )
                            .unwrap();
                        let enabled = msaa.samples() > 1
                            && alpha_test
                            && !blend
                            && !emissive
                            && matches!(
                                kind,
                                EntityRenderMaterial::Default | EntityRenderMaterial::Glint
                            );
                        assert_eq!(descriptor.multisample.alpha_to_coverage_enabled, enabled);
                        assert_eq!(
                            descriptor
                                .fragment
                                .as_ref()
                                .unwrap()
                                .shader_defs
                                .contains(&crate::alpha_coverage::SHADER_DEF.into()),
                            enabled
                        );
                        let warmed = crate::queue_review_support::queued_descriptor(&mut cache, id);
                        assert_eq!(warmed.multisample.alpha_to_coverage_enabled, enabled);
                    }
                }
            }
        }
    }
}
