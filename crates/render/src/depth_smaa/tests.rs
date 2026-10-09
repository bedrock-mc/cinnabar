use super::*;
use bevy::{ecs::system::RunSystemOnce, render::render_graph::EmptyNode};

#[path = "raster_tests.rs"]
mod raster_tests;

/// Follows graph dependencies without depending on node insertion order.
fn reaches(graph: &RenderGraph, from: impl RenderLabel, to: impl RenderLabel) -> bool {
    let target = to.intern();
    let mut pending = vec![from.intern()];
    let mut seen = std::collections::HashSet::new();
    while let Some(label) = pending.pop() {
        if label == target {
            return true;
        }
        if seen.insert(label) {
            pending.extend(
                graph
                    .get_node_state(label)
                    .unwrap()
                    .edges
                    .output_edges()
                    .iter()
                    .map(|edge| edge.get_input_node()),
            );
        }
    }
    false
}

/// Builds the world overlay dependencies without a graphics device.
fn graph_world() -> World {
    let mut core = RenderGraph::default();
    for label in [
        Node3d::MainTransparentPass.intern(),
        crate::ui_render::UiWorldLabel.intern(),
        crate::viewmodel_render::HandLabel.intern(),
        crate::hand_rig_render::HandRigLabel.intern(),
        Node3d::EndMainPass.intern(),
        crate::ui_render::UiOverlayLabel.intern(),
    ] {
        core.add_node(label, EmptyNode);
    }
    core.add_node_edges((
        crate::ui_render::UiWorldLabel,
        crate::viewmodel_render::HandLabel,
        crate::hand_rig_render::HandRigLabel,
        Node3d::EndMainPass,
        crate::ui_render::UiOverlayLabel,
    ));
    let mut graphs = RenderGraph::default();
    graphs.add_sub_graph(Core3d, core);
    let mut world = World::new();
    world.insert_resource(graphs);
    world
}

#[test]
fn depth_smaa_orders_before_text_hands_and_ui_composite() {
    let mut world = graph_world();
    world.run_system_once(sync_graph).unwrap();
    assert!(
        world
            .resource::<RenderGraph>()
            .get_sub_graph(Core3d)
            .unwrap()
            .get_node_state(nametags::NametagsAfterSmaaLabel)
            .is_err()
    );
    assert!(
        world
            .resource::<RenderGraph>()
            .get_sub_graph(Core3d)
            .unwrap()
            .get_node_state(Node3d::Smaa)
            .is_err()
    );
    let camera = world.spawn((Camera3d::default(), Smaa::default())).id();
    world.run_system_once(sync_graph).unwrap();
    let graph = world
        .resource::<RenderGraph>()
        .get_sub_graph(Core3d)
        .unwrap();
    assert!(reaches(graph, Node3d::MainTransparentPass, Node3d::Smaa));
    assert!(reaches(
        graph,
        Node3d::Smaa,
        nametags::NametagsAfterSmaaLabel
    ));
    assert!(reaches(
        graph,
        nametags::NametagsAfterSmaaLabel,
        crate::ui_render::UiWorldLabel
    ));
    assert!(!reaches(
        graph,
        nametags::NametagsAfterSmaaLabel,
        Node3d::Smaa
    ));
    for label in [
        crate::ui_render::UiWorldLabel.intern(),
        crate::viewmodel_render::HandLabel.intern(),
        crate::hand_rig_render::HandRigLabel.intern(),
        crate::ui_render::UiOverlayLabel.intern(),
    ] {
        assert!(reaches(graph, Node3d::Smaa, label));
        assert!(!reaches(graph, label, Node3d::Smaa));
    }
    world.entity_mut(camera).remove::<Smaa>();
    world.run_system_once(sync_graph).unwrap();
    assert!(
        world
            .resource::<RenderGraph>()
            .get_sub_graph(Core3d)
            .unwrap()
            .get_node_state(nametags::NametagsAfterSmaaLabel)
            .is_err()
    );
    assert!(
        world
            .resource::<RenderGraph>()
            .get_sub_graph(Core3d)
            .unwrap()
            .get_node_state(Node3d::Smaa)
            .is_err()
    );
    // The view query cannot run any pass without the enabled camera component.
    let disabled = world.spawn_empty().id();
    let enabled = world.spawn(Smaa::default()).id();
    let mut admitted = world.query_filtered::<Entity, With<Smaa>>();
    assert!(!admitted.iter(&world).any(|entity| entity == disabled));
    assert!(admitted.iter(&world).any(|entity| entity == enabled));
}

#[test]
fn depth_smaa_unchanged_graph_sync_allocates_nothing() {
    for enabled in [false, true] {
        let mut world = graph_world();
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
fn depth_smaa_orders_before_motion_blur_across_independent_toggles() {
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
        let mut world = graph_world();
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
            let graph = world
                .resource::<RenderGraph>()
                .get_sub_graph(Core3d)
                .unwrap();
            let blur_label = crate::motion_blur::graph::MotionBlurLabel;
            assert_eq!(graph.get_node_state(Node3d::Smaa).is_ok(), smaa);
            assert_eq!(graph.get_node_state(blur_label.clone()).is_ok(), blur);
            assert_eq!(
                graph
                    .get_node_state(nametags::NametagsAfterSmaaLabel)
                    .is_ok(),
                smaa
            );
            if smaa && blur {
                assert!(reaches(graph, Node3d::Smaa, blur_label.clone()));
                assert!(reaches(
                    graph,
                    blur_label.clone(),
                    nametags::NametagsAfterSmaaLabel
                ));
                assert!(!reaches(graph, blur_label.clone(), Node3d::Smaa));
            }
            for effect in [
                smaa.then(|| Node3d::Smaa.intern()),
                blur.then(|| blur_label.intern()),
            ]
            .into_iter()
            .flatten()
            {
                for sharp in [
                    crate::ui_render::UiWorldLabel.intern(),
                    crate::viewmodel_render::HandLabel.intern(),
                    crate::hand_rig_render::HandRigLabel.intern(),
                    crate::ui_render::UiOverlayLabel.intern(),
                ] {
                    assert!(reaches(graph, effect, sharp));
                    assert!(!reaches(graph, sharp, effect));
                }
            }
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
