use super::*;
use bevy::ecs::system::RunSystemOnce;

#[path = "raster_tests.rs"]
mod raster_tests;

#[test]
fn depth_smaa_unchanged_filter_sync_allocates_nothing() {
    for enabled in [false, true] {
        let mut world = crate::render_test_support::empty_render_world();
        let camera = world.spawn(Camera3d::default()).id();
        if enabled {
            world.entity_mut(camera).insert(Smaa::default());
        }
        let mut sync = IntoSystem::into_system(sync_graph);
        sync.initialize(&mut world);
        sync.run((), &mut world).unwrap();
        sync.run((), &mut world).unwrap();
        let before = crate::alloc_count::thread_allocations();
        sync.run((), &mut world).unwrap();
        assert_eq!(crate::alloc_count::thread_allocations(), before);
    }
}

#[test]
fn unprepared_views_encode_nothing_across_independent_filter_toggles() {
    for states in [
        [
            (false, false),
            (true, false),
            (true, true),
            (false, true),
            (false, false),
        ],
        [
            (false, false),
            (false, true),
            (true, true),
            (true, false),
            (false, false),
        ],
    ] {
        let mut world = crate::render_test_support::empty_render_world();
        let camera = world.spawn(Camera3d::default()).id();
        let mut schedule = bevy::ecs::schedule::Schedule::default();
        schedule.add_systems((
            sync_graph.after(crate::motion_blur::graph::sync_graph),
            crate::motion_blur::graph::sync_graph,
        ));
        for (smaa, blur) in states {
            if smaa {
                world.entity_mut(camera).insert(Smaa::default());
            } else {
                world.entity_mut(camera).remove::<Smaa>();
            }
            if blur {
                world
                    .entity_mut(camera)
                    .insert(crate::motion_blur::CameraMotionBlur {
                        exposure_seconds: 0.01,
                        delta_seconds: 0.01,
                        samples: 7,
                        reset_epoch: 0,
                    });
            } else {
                world
                    .entity_mut(camera)
                    .remove::<crate::motion_blur::CameraMotionBlur>();
            }
            schedule.run(&mut world);
            schedule.run(&mut world);
            crate::render_test_support::assert_empty_render(&mut world);
        }
    }
}

#[test]
fn depth_smaa_edge_shader_reads_only_depth_for_both_sample_modes() {
    for definitions in [vec![], vec!["MULTISAMPLED"]] {
        let shader = crate::shader_source::preprocess(include_str!("edge.wgsl"), &definitions);
        assert!(crate::shader_test_support::fragment_reads_binding(
            &shader, 0, 0
        ));
        let module = naga::front::wgsl::parse_str(&shader).unwrap();
        assert_eq!(module.global_variables.len(), 1);
        let variable = &module.global_variables.iter().next().unwrap().1;
        assert!(matches!(
            module.types[variable.ty].inner,
            naga::TypeInner::Image {
                class: naga::ImageClass::Depth { .. },
                ..
            }
        ));
    }
}

#[test]
fn depth_smaa_prewarm_queues_and_reuses_every_sample_variant() {
    use crate::pipeline_warmup::{PrewarmPipelines, WarmView};
    let (mut app, _) = crate::queue_review_support::app();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
    app.init_asset::<Shader>();
    app.world_mut().run_system_once(pipelines::init).unwrap();
    let world = app.world_mut();
    world.resource_scope(|world, mut pipelines: Mut<DepthSmaaPipelines>| {
        let cache = world.resource::<PipelineCache>();
        for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
            for hdr in [false, true] {
                let view = WarmView {
                    msaa,
                    hdr,
                    enhanced: false,
                    output: None,
                };
                let mut before = Vec::new();
                pipelines.prewarm(cache, view, &mut before).unwrap();
                assert_eq!(before.len(), 4);
                let mut after = Vec::new();
                pipelines.prewarm(cache, view, &mut after).unwrap();
                assert_eq!(before, after);
            }
        }
        assert_eq!(pipelines.variants_len(), 8);
    });
}
