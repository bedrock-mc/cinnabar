use super::{CameraMotionBlur, graph::*, history::*};
use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    prelude::*,
    render::{
        RenderApp,
        render_graph::{EmptyNode, InternedRenderLabel, RenderGraph, RenderLabel},
    },
};

fn settings() -> CameraMotionBlur {
    CameraMotionBlur {
        exposure_seconds: 0.01,
        samples: 7,
        reset_epoch: 0,
        delta_seconds: 0.02,
    }
}
fn history(position: Vec3, yaw: f32, epoch: u64) -> CameraHistory {
    let pose = Mat4::from_rotation_translation(Quat::from_rotation_y(yaw), position);
    CameraHistory::new(
        Mat4::perspective_infinite_reverse_rh(1.2, 2.0, 0.1),
        pose,
        UVec4::new(0, 0, 640, 320),
        epoch,
    )
}

#[test]
fn camera_reanchors_and_invalid_frames_have_zero_strength() {
    let mut previous = history(Vec3::ZERO, 0.0, 0);
    assert_eq!(previous.advance(previous, settings()).strength.x, 0.0);
    assert!(
        previous
            .advance(history(Vec3::X, 0.1, 0), settings())
            .strength
            .x
            > 0.0
    );
    assert_eq!(
        previous
            .advance(history(Vec3::X, 0.2, 1), settings())
            .strength
            .x,
        0.0
    );
    assert!(
        previous
            .advance(history(Vec3::X, 0.3, 1), settings())
            .strength
            .x
            > 0.0
    );
    assert_eq!(
        previous
            .advance(history(Vec3::X * 100.0, 0.3, 1), settings())
            .strength
            .x,
        0.0
    );
    assert_eq!(
        previous
            .advance(history(Vec3::ZERO, 2.0, 1), settings())
            .strength
            .x,
        0.0
    );
    for dt in [0.0, -1.0, f32::NAN, f32::INFINITY, 0.3] {
        let mut previous = history(Vec3::ZERO, 0.0, 0);
        assert_eq!(
            previous
                .advance(
                    history(Vec3::ZERO, 0.1, 0),
                    CameraMotionBlur {
                        delta_seconds: dt,
                        ..settings()
                    }
                )
                .strength
                .x,
            0.0
        );
    }
}

#[test]
fn exposure_uses_real_elapsed_time_and_history_allocates_nothing() {
    let projected_velocity = |dt| {
        let mut previous = history(Vec3::ZERO, 0.0, 0);
        let uniform = previous.advance(
            history(Vec3::X * dt, 0.0, 0),
            CameraMotionBlur {
                delta_seconds: dt,
                ..settings()
            },
        );
        let previous_pixel = uniform
            .previous_clip_from_clip
            .project_point3(Vec3::new(0.0, 0.0, 0.05));
        previous_pixel.x * uniform.strength.x
    };
    assert!((projected_velocity(1.0 / 60.0) - projected_velocity(1.0 / 240.0)).abs() < 0.00001);
    let mut previous = history(Vec3::ZERO, 0.0, 0);
    let current = history(Vec3::X, 0.1, 0);
    let start = crate::alloc_count::thread_allocations();
    std::hint::black_box(previous.advance(current, settings()));
    assert_eq!(crate::alloc_count::thread_allocations(), start);
}

#[test]
fn motion_blur_reprojection_is_independent_of_the_world_origin() {
    let reproject = |origin: Vec3, translation: Vec3| {
        let mut previous = history(origin, 0.1, 0);
        previous
            .advance(history(origin + translation, 0.101, 0), settings())
            .previous_clip_from_clip
            .project_point3(Vec3::new(0.0, 0.0, 0.1))
    };
    for translation in [Vec3::ZERO, Vec3::new(0.125, 0.25, -0.125)] {
        let expected = reproject(Vec3::ZERO, translation);
        for offset in [1_000.0, 10_000.0, 100_000.0, 1_000_000.0, -1_000_000.0] {
            let actual = reproject(Vec3::new(offset, 64.0, offset), translation);
            assert!(
                actual.abs_diff_eq(expected, 0.000001),
                "{offset}: {actual:?} != {expected:?}"
            );
        }
    }
}

fn reachable(graph: &RenderGraph, from: InternedRenderLabel, to: InternedRenderLabel) -> bool {
    from == to
        || graph
            .iter_node_outputs(from)
            .unwrap()
            .any(|(_, node)| reachable(graph, node.label, to))
}

#[test]
fn off_has_no_graph_pass_and_on_precedes_both_hands_nametags_and_ui() {
    use bevy::{
        app::SubApp,
        ecs::schedule::Schedule,
        render::{
            ExtractSchedule, Render, RenderStartup,
            renderer::{RenderDevice, RenderQueue, WgpuWrapper},
        },
    };
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut core = RenderGraph::default();
    for label in [
        Node3d::MainTransparentPass,
        Node3d::EndMainPass,
        Node3d::EndMainPassPostProcessing,
        Node3d::Upscaling,
    ] {
        core.add_node(label, EmptyNode);
    }
    core.add_node_edges((
        Node3d::MainTransparentPass,
        Node3d::EndMainPass,
        Node3d::EndMainPassPostProcessing,
        Node3d::Upscaling,
    ));
    let mut graphs = RenderGraph::default();
    graphs.add_sub_graph(Core3d, core);
    let mut render_app = SubApp::new();
    render_app
        .insert_resource(RenderDevice::from(device))
        .insert_resource(RenderQueue(std::sync::Arc::new(WgpuWrapper::new(queue))))
        .insert_resource(graphs)
        .add_schedule(Schedule::new(RenderStartup))
        .add_schedule(Render::base_schedule())
        .add_schedule(Schedule::new(ExtractSchedule));
    let mut app = App::new();
    app.init_resource::<Assets<Shader>>()
        .insert_sub_app(RenderApp, render_app);
    app.add_plugins((
        crate::UiRenderPlugin,
        crate::HandRigRenderPlugin,
        crate::ViewmodelRenderPlugin,
    ));
    app.finish();
    let world = app.sub_app_mut(RenderApp).world_mut();
    configure_graph(world, false);
    let allocations = crate::alloc_count::thread_allocations();
    configure_graph(world, false);
    assert_eq!(allocations, crate::alloc_count::thread_allocations());
    assert!(
        world
            .resource::<RenderGraph>()
            .get_sub_graph(Core3d)
            .unwrap()
            .get_node_state(MotionBlurLabel)
            .is_err()
    );
    configure_graph(world, true);
    let graph = world
        .resource::<RenderGraph>()
        .get_sub_graph(Core3d)
        .unwrap();
    for sharp in [
        SharpNametagsLabel.intern(),
        crate::ui_render::UiWorldLabel.intern(),
        crate::viewmodel_render::HandLabel.intern(),
        crate::hand_rig_render::HandRigLabel.intern(),
        crate::ui_render::UiOverlayLabel.intern(),
    ] {
        assert!(reachable(graph, MotionBlurLabel.intern(), sharp));
        assert!(!reachable(graph, sharp, MotionBlurLabel.intern()));
    }
    configure_graph(world, false);
    let graph = world
        .resource::<RenderGraph>()
        .get_sub_graph(Core3d)
        .unwrap();
    assert!(graph.get_node_state(MotionBlurLabel).is_err());
    assert!(graph.get_node_state(SharpNametagsLabel).is_err());
    assert!(reachable(
        graph,
        Node3d::MainTransparentPass.intern(),
        crate::viewmodel_render::HandLabel.intern()
    ));
}
