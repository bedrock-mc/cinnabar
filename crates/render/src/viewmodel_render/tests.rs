use super::*;

fn empty_hand_world() -> World {
    use bevy::ecs::system::RunSystemOnce;
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions {
                enable: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut context = Context::from_waker(Waker::noop());
    let Poll::Ready(Ok(adapter)) =
        pin!(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).poll(&mut context)
    else {
        panic!("noop adapter must be immediate");
    };
    let Poll::Ready(Ok((device, queue))) =
        pin!(adapter.request_device(&wgpu::DeviceDescriptor::default())).poll(&mut context)
    else {
        panic!("noop device must be immediate");
    };
    let device = RenderDevice::from(device);
    let adapter = RenderAdapter::new(adapter);
    let mut world = World::new();
    world.insert_resource(PipelineCache::new(device.clone(), true));
    world.insert_resource(device);
    world.insert_resource(adapter);
    world.insert_resource(RenderQueue::new(queue));
    world.init_resource::<ViewmodelCompletionGate>();
    world.init_resource::<ViewmodelScene>();
    world.init_resource::<HandDrawn>();
    world.init_resource::<crate::ui_render::UiHandCoverage>();
    world.run_system_once(init_gpu).unwrap();
    world
}

#[test]
fn every_empty_prepare_observes_device_replacement_and_refuses_until_actual_startup() {
    use bevy::ecs::system::RunSystemOnce;
    for clone_replacement in [false, true] {
        let mut world = empty_hand_world();
        world.resource_mut::<HandGpu>().pipeline_variants[0] =
            Some(CachedRenderPipelineId::INVALID);
        world.run_system_once(prepare).unwrap();
        assert!(
            world.resource::<HandGpu>().pipeline_variants[0].is_some(),
            "unchanged empty preparation preserves bounded cache"
        );
        let old_device = world.resource::<RenderDevice>().clone();
        let replacement = if clone_replacement {
            old_device
        } else {
            RenderDevice::from(wgpu::Device::noop(&wgpu::DeviceDescriptor::default()).0)
        };
        world.increment_change_tick();
        world.insert_resource(replacement);
        world.run_system_once(prepare).unwrap();
        assert!(
            world
                .resource::<HandGpu>()
                .pipeline_variants
                .iter()
                .all(Option::is_none)
        );
        for _ in 0..3 {
            world.resource_mut::<HandGpu>().pipeline_variants[0] =
                Some(CachedRenderPipelineId::INVALID);
            world.run_system_once(prepare).unwrap();
            let gpu = world.resource::<HandGpu>();
            assert!(gpu.pipeline_variants.iter().all(Option::is_none));
            assert!(
                gpu.token.is_none()
                    && gpu.vertices.is_none()
                    && gpu.skin.is_none()
                    && gpu.depth.is_none()
                    && gpu.bind_group.is_none()
                    && gpu.pipeline.is_none()
            );
        }
        world.run_system_once(init_gpu).unwrap();
        world.resource_mut::<HandGpu>().pipeline_variants[0] =
            Some(CachedRenderPipelineId::INVALID);
        world.run_system_once(prepare).unwrap();
        assert!(
            world.resource::<HandGpu>().pipeline_variants[0].is_some(),
            "only actual startup resets the observed refusal"
        );
    }
}

#[test]
fn shared_hand_device_observation_handles_wrap_and_fail_closed_unknown_gap() {
    use bevy::ecs::change_detection::{MAX_CHANGE_AGE, Tick};
    let mut observation = DeviceObservation::new(Tick::new(u32::MAX - 2));
    assert!(observation.observe(Tick::new(u32::MAX - 3), Tick::new(1), true));
    assert!(observation.observe(Tick::new(u32::MAX - 3), Tick::new(2), true));
    assert!(!observation.observe(Tick::new(3), Tick::new(4), true));
    assert!(!observation.observe(Tick::new(3), Tick::new(5), true));
    let mut observation = DeviceObservation::new(Tick::new(10));
    assert!(!observation.observe(Tick::new(0), Tick::new(10 + MAX_CHANGE_AGE), true));
    assert!(!observation.observe(Tick::new(0), Tick::new(11 + MAX_CHANGE_AGE), true));
}

#[test]
fn neutral_shader_validates_and_has_private_projection_abi() {
    let module = naga::front::wgsl::parse_str(include_str!("../viewmodel.wgsl")).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    assert_eq!(size_of::<HandVertex>(), 20);
    let descriptor = hand_pipeline_descriptor(hand_layout());
    assert_eq!(
        descriptor.depth_stencil.unwrap().depth_compare,
        Some(CompareFunction::GreaterEqual)
    );
    assert_eq!(descriptor.vertex.buffers[0].array_stride, 20);
    assert!(
        descriptor.fragment.unwrap().targets[0]
            .as_ref()
            .unwrap()
            .blend
            .is_none()
    );
}

#[test]
fn hand_attachment_specialization_matches_hdr_and_msaa_without_world_depth() {
    for samples in [1, 2, 4, 8] {
        for hdr in [false, true] {
            let pipeline = specialized_hand_pipeline(hand_layout(), samples, hdr);
            assert_eq!(pipeline.multisample.count, samples);
            assert_eq!(
                pipeline.fragment.unwrap().targets[0]
                    .as_ref()
                    .unwrap()
                    .format,
                if hdr {
                    crate::SCENE_HDR_FORMAT
                } else {
                    crate::SCENE_COLOR_FORMAT
                }
            );
            let depth = pipeline.depth_stencil.unwrap();
            assert_eq!(depth.format, TextureFormat::Depth32Float);
            assert_eq!(depth.depth_write_enabled, Some(true));
        }
    }
}

#[test]
fn hand_pipeline_variants_are_bounded_across_toggles_and_disabled_frames() {
    let mut entries = [None; 8];
    let mut creations = 0u32;
    let mut expected = [[0u32; 4]; 2];
    for cycle in 0..16 {
        for (hdr_index, hdr) in [false, true].into_iter().enumerate() {
            for (sample_index, samples) in [1, 2, 4, 8].into_iter().enumerate() {
                let id = memoized_hand_pipeline(&mut entries, samples, hdr, || {
                    creations += 1;
                    creations
                })
                .unwrap();
                if cycle == 0 {
                    expected[hdr_index][sample_index] = id;
                }
                assert_eq!(id, expected[hdr_index][sample_index]);
            }
        }
    }
    assert_eq!(creations, 8);
    assert_eq!(entries.iter().filter(|entry| entry.is_some()).count(), 8);
    for unsupported in [0, 3, 16, u32::MAX] {
        assert!(
            memoized_hand_pipeline(&mut entries, unsupported, false, || {
                panic!("unsupported key created pipeline")
            })
            .is_none()
        );
    }
    assert_eq!(creations, 8);
}

#[test]
fn actual_empty_hand_deactivation_preserves_variants_for_reenable() {
    use bevy::ecs::system::RunSystemOnce;
    let (device, _queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.init_resource::<ViewmodelCompletionGate>();
    world.init_resource::<HandDrawn>();
    world.run_system_once(init_gpu).unwrap();
    let mut gpu = world.resource_mut::<HandGpu>();
    let mut creations = 0;
    for _ in 0..8 {
        for (samples, hdr) in [(1, false), (4, true), (1, false)] {
            let selected = memoized_hand_pipeline(&mut gpu.pipeline_variants, samples, hdr, || {
                creations += 1;
                CachedRenderPipelineId::INVALID
            });
            assert!(selected.is_some());
            gpu.pipeline = selected;
            let before = gpu.pipeline_variants;
            deactivate_hand(&mut gpu);
            assert!(gpu.token.is_none());
            assert!(gpu.depth.is_none());
            assert_eq!(gpu.pipeline_variants, before);
        }
    }
    assert_eq!(creations, 2);
    assert_eq!(
        gpu.pipeline_variants
            .iter()
            .filter(|entry| entry.is_some())
            .count(),
        2
    );
}

#[test]
fn actual_missing_current_view_coverage_revokes_prior_completion() {
    use bevy::ecs::system::RunSystemOnce;
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue::new(queue));
    world.init_resource::<HandDrawn>();
    let gate = ViewmodelCompletionGate::default();
    let token = ViewmodelToken {
        session: 1,
        actor_session: 2,
        dimension: 0,
        runtime: 3,
        spawn: 4,
        owner: Entity::from_raw_u32(0).unwrap(),
        viewport: [640, 480],
        samples: 1,
        hdr: false,
        skin: [5; 32],
        geometry: [6; 32],
        revision: 1,
    };
    gate.select(Some(token));
    assert!(gate.complete(gate.reserve(token).unwrap()));
    world.insert_resource(gate.clone());
    let coverage = crate::ui_render::UiHandCoverage::default();
    coverage.clear();
    let batch = render_model::UiRenderBatch::new(
        0,
        render_model::UiScissor::new(0, 0, 640, 480),
        0,
        6,
        render_model::UI_BLEND_ALPHA,
    );
    let render_view = Entity::from_raw_u32(1).unwrap();
    gate.select(None);
    gate.select(Some(token));
    node::record_overlay_coverage(&coverage, &gate, token, render_view, token.owner, (7, 0, 0));
    assert!(
        coverage
            .range(render_view, token.owner, Some(7), &[batch], 6)
            .is_none()
    );
    assert!(gate.complete(gate.reserve(token).unwrap()));
    node::record_overlay_coverage(&coverage, &gate, token, render_view, token.owner, (7, 0, 0));
    assert_eq!(
        coverage.range(render_view, token.owner, Some(7), &[batch], 6),
        Some(0..6)
    );
    coverage.clear();
    assert!(
        coverage
            .range(render_view, token.owner, Some(7), &[batch], 6)
            .is_none()
    );
    world.run_system_once(init_gpu).unwrap();
    // Renderer recreation intentionally removed the earlier completion. This
    // witness then selects a fresh lifetime before testing missing node coverage.
    gate.select(Some(token));
    assert!(gate.complete(gate.reserve(token).unwrap()));
    world.resource_mut::<HandGpu>().token = Some(token);
    // No node recorded this intended view this frame: readiness and yesterday's
    // completion are insufficient, including a pipeline/resource-loss frame.
    world.run_system_once(submit_completion).unwrap();
    assert!(!gate.completed(token));
    assert_eq!(gate.rejection_count(), 1);
}

#[test]
fn hand_completion_follows_the_frame_without_a_submit_of_its_own() {
    use bevy::{ecs::system::RunSystemOnce, render::render_resource::PollType};
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let queue = RenderQueue::new(queue);
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(queue.clone());
    world.init_resource::<HandDrawn>();
    let gate = ViewmodelCompletionGate::default();
    world.insert_resource(gate.clone());
    world.run_system_once(init_gpu).unwrap();
    let token = ViewmodelToken {
        session: 1,
        actor_session: 2,
        dimension: 0,
        runtime: 3,
        spawn: 4,
        owner: Entity::from_raw_u32(0).unwrap(),
        viewport: [640, 480],
        samples: 1,
        hdr: false,
        skin: [5; 32],
        geometry: [6; 32],
        revision: 1,
    };
    gate.select(Some(token));
    world.resource_mut::<HandGpu>().token = Some(token);
    *world.resource::<HandDrawn>().0.lock().unwrap() = Some(token);

    let baseline = crate::device_poll::submission_marker(&queue);
    queue.on_submitted_work_done(|| {});
    let callback_delta = crate::device_poll::submission_marker(&queue) - baseline;
    let before = crate::device_poll::submission_marker(&queue);
    world.run_system_once(submit_completion).unwrap();
    assert_eq!(
        crate::device_poll::submission_marker(&queue) - before,
        callback_delta,
        "hand completion only registers the frame callback"
    );
    world
        .resource::<RenderDevice>()
        .poll(PollType::wait_indefinitely())
        .unwrap();
    assert!(
        gate.completed(token),
        "the frame's completion publishes the hand"
    );
}

#[test]
fn an_unprepared_hand_submits_nothing_after_repeated_installation() {
    let mut world = crate::render_test_support::empty_render_world();
    crate::ui_render::install_overlay_graph(&mut world);
    world.insert_resource(Installed);
    install_hand_graph(&mut world);
    install_hand_graph(&mut world);
    crate::ui_render::install_overlay_graph(&mut world);
    crate::render_test_support::assert_empty_render(&mut world);
}

#[test]
fn both_plugin_orders_submit_nothing_without_a_prepared_view() {
    use bevy::{app::SubApp, ecs::schedule::Schedule, render::ExtractSchedule};
    for hand_first in [false, true] {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let mut render_app = SubApp::new();
        render_app
            .insert_resource(RenderDevice::from(device))
            .insert_resource(RenderQueue::new(queue))
            .add_schedule(Core3d::base_schedule())
            .add_schedule(Schedule::new(RenderStartup))
            .add_schedule(Render::base_schedule())
            .add_schedule(Schedule::new(ExtractSchedule));
        let mut app = App::new();
        app.insert_resource(Assets::<Shader>::default())
            .insert_sub_app(RenderApp, render_app);
        if hand_first {
            app.add_plugins((
                ViewmodelRenderPlugin,
                crate::HandRigRenderPlugin,
                crate::ui_render::UiRenderPlugin,
            ));
        } else {
            app.add_plugins((
                crate::ui_render::UiRenderPlugin,
                crate::HandRigRenderPlugin,
                ViewmodelRenderPlugin,
            ));
        }
        app.finish();
        install_hand_graph(app.sub_app_mut(RenderApp).world_mut());
        let world = app.sub_app_mut(RenderApp).world_mut();
        world.init_resource::<bevy::render::renderer::PendingCommandBuffers>();
        crate::render_test_support::assert_empty_render(world);
    }
}
