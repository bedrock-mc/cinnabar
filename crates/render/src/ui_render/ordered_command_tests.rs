use super::pipeline::{
    UiPipeline, UiPipelineKey, UiPipelineSpecializer, ui_bind_group_layout, ui_pipeline_descriptor,
};
use super::resources::{init_ui_gpu, prepare_ui_resources};
use super::*;
use render_model::UiScissor;

#[test]
fn world_projection_specializes_native_test_and_write_modes_at_each_msaa_sample_count() {
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        for (depth_test, depth_write) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let mut descriptor = ui_pipeline_descriptor(ui_bind_group_layout());
            UiPipelineSpecializer
                .specialize(
                    UiPipelineKey {
                        msaa,
                        hdr: true,
                        invert_blend: false,
                        layer: false,
                        depth_test,
                        depth_write,
                        isolated_depth: false,
                    },
                    &mut descriptor,
                )
                .unwrap();
            assert_eq!(descriptor.multisample.count, msaa.samples());
            if depth_test || depth_write {
                let depth = descriptor.depth_stencil.unwrap();
                assert_eq!(depth.depth_write_enabled, depth_write);
                assert_eq!(
                    depth.depth_compare,
                    if depth_test {
                        CompareFunction::GreaterEqual
                    } else {
                        CompareFunction::Always
                    }
                );
                assert_eq!(depth.format, CORE_3D_DEPTH_FORMAT);
                assert_eq!(
                    depth.bias.constant,
                    if depth_test && depth_write {
                        -pipeline::NATIVE_ENVIRONMENTAL_TEXT_DEPTH_BIAS
                    } else {
                        0
                    }
                );
                assert_eq!(depth.bias.slope_scale, 0.0);
                assert_eq!(depth.bias.clamp, 0.0);
                if depth_test && depth_write {
                    assert!(
                        depth.bias.constant > 0,
                        "reverse-Z toward-eye bias must be positive"
                    );
                }
            } else {
                assert!(descriptor.depth_stencil.is_none());
            }
        }
    }
    let descriptor = ui_pipeline_descriptor(ui_bind_group_layout());
    assert_eq!(
        descriptor.vertex.buffers[0].array_stride,
        std::mem::size_of::<UiRenderVertex>() as u64
    );
    assert_eq!(
        descriptor.vertex.buffers[0].attributes[0].format,
        VertexFormat::Float32x4
    );
    assert_eq!(
        descriptor.vertex.buffers[0].attributes[1].format,
        VertexFormat::Float32x2
    );
    assert_eq!(
        descriptor.vertex.buffers[0].attributes[1].offset,
        std::mem::offset_of!(UiRenderVertex, uv) as u64
    );
    assert_eq!(
        descriptor.vertex.buffers[0].attributes[2].offset,
        std::mem::offset_of!(UiRenderVertex, color) as u64
    );
    assert_eq!(
        std::mem::offset_of!(UiRenderVertex, clip_z),
        std::mem::size_of::<[f32; 2]>()
    );
    assert_eq!(
        std::mem::offset_of!(UiRenderVertex, clip_w),
        std::mem::size_of::<[f32; 3]>()
    );
}

pub(super) fn binding_world() -> World {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::render::renderer::{RenderAdapter, WgpuWrapper};
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..Default::default()
        },
        ..Default::default()
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
    let adapter = RenderAdapter(Arc::new(WgpuWrapper::new(adapter)));
    let mut world = World::new();
    world.insert_resource(PipelineCache::new(device.clone(), adapter, true));
    world.insert_resource(device);
    world.insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))));
    world.init_resource::<UiPipeline>();
    world.init_resource::<UiRenderStatsResource>();
    world.init_resource::<UiRenderSceneResource>();
    world.run_system_once(init_ui_gpu).unwrap();
    world
}

#[test]
fn actual_rejected_and_empty_preparation_cannot_bind_withheld_texture_resources() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = binding_world();
    let input = UiRenderInput {
        revision: 1,
        viewport_size: [64, 64],
        safe_area: [0; 4],
        vertices: Arc::from([]),
        indices: Arc::from([]),
        batches: Arc::from([]),
        textures: Arc::new(
            render_model::UiTextureCatalog::new(
                vec![render_model::UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    };
    let mut scene = UiRenderScene::default();
    scene
        .publish(input.clone(), world.resource::<UiRenderStatsResource>())
        .unwrap();
    world.insert_resource(UiRenderSceneResource(scene.clone()));
    world.run_system_once(prepare_ui_resources).unwrap();
    assert_eq!(world.resource::<UiGpu>().textures.buckets.len(), 1);
    let mut invalid = input.clone();
    invalid.indices = Arc::from([u32::MAX]);
    scene.input = Some(Arc::new(invalid));
    world.insert_resource(UiRenderSceneResource(scene.clone()));
    world.run_system_once(prepare_ui_resources).unwrap();
    world.run_system_once(prepare_ui_bind_group).unwrap();
    assert!(
        world.resource::<UiGpu>().textures.buckets[0]
            .bind_group
            .is_none()
    );
    assert!(world.resource::<UiGpu>().accepted_revision.is_none());
    scene.input = None;
    world.insert_resource(UiRenderSceneResource(scene));
    world.run_system_once(prepare_ui_resources).unwrap();
    world.run_system_once(prepare_ui_bind_group).unwrap();
    assert!(
        world.resource::<UiGpu>().textures.buckets[0]
            .bind_group
            .is_none()
    );
    // Actual initialization drops withheld resident resources and resets the
    // publication lifetime. Re-admit through the real publication path.
    world.run_system_once(init_ui_gpu).unwrap();
    let mut recovered = UiRenderScene::default();
    recovered
        .publish(input, world.resource::<UiRenderStatsResource>())
        .unwrap();
    world.insert_resource(UiRenderSceneResource(recovered));
    world.run_system_once(prepare_ui_resources).unwrap();
    world.run_system_once(prepare_ui_bind_group).unwrap();
    assert_eq!(world.resource::<UiGpu>().accepted_revision, Some(1));
    assert!(
        world.resource::<UiGpu>().textures.buckets[0]
            .bind_group
            .is_some()
    );
}

#[test]
fn actual_preparation_uploads_changed_spans_and_refills_reallocated_arenas() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = binding_world();
    let vertex = UiRenderVertex {
        position: [0.0; 2],
        clip_z: 0.0,
        clip_w: 1.0,
        uv: [0.0; 2],
        color: [255; 4],
        style_flags: 0,
        alpha_cutoff: -1.0,
        model_light: 1.0,
        overlay_color: [0.0; 4],
    };
    let mut input = UiRenderInput {
        revision: 1,
        viewport_size: [64; 2],
        safe_area: [0; 4],
        vertices: Arc::from([vertex; 4]),
        indices: Arc::from([0, 1, 2, 0, 2, 3]),
        batches: Arc::from([UiRenderBatch::new(0, UiScissor::new(0, 0, 64, 64), 0, 6, 0)]),
        textures: Arc::new(
            render_model::UiTextureCatalog::new(
                vec![render_model::UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    };
    let mut scene = UiRenderScene::default();
    for revision in 1..=4 {
        input.revision = revision;
        match revision {
            2 => Arc::make_mut(&mut input.vertices)[1].position[0] = 1.0,
            3 => input.viewport_size = [128; 2],
            4 => input.vertices = vec![vertex; 4_000].into(),
            _ => {}
        }
        scene
            .publish(input.clone(), world.resource::<UiRenderStatsResource>())
            .unwrap();
        world.insert_resource(UiRenderSceneResource(scene.clone()));
        world.run_system_once(prepare_ui_resources).unwrap();
        let stats = world.resource::<UiRenderStatsResource>().snapshot();
        assert_eq!(stats.accepted_revision, Some(revision));
        assert_eq!(
            stats.uploaded_vertices,
            [4, 1, 0, 4_000][revision as usize - 1]
        );
        assert_eq!(stats.uploaded_indices, if revision == 1 { 6 } else { 0 });
        assert_eq!(world.resource::<UiGpu>().viewport_size, input.viewport_size);
    }
}

#[test]
fn resolved_commands_keep_bucket_layer_blend_scissor_and_index_order() {
    let plan =
        render_model::UiTexturePlan::new(&[[1024, 1024], [2048, 2048], [256, 256], [2048, 2048]])
            .unwrap();
    let batches = [2, 0, 3, 1, 2]
        .into_iter()
        .enumerate()
        .map(|(index, page)| {
            UiRenderBatch::new(
                page,
                UiScissor::new(index as u32, 2, 30, 40),
                index as u32 * 6,
                6,
                if index == 2 { UI_BLEND_INVERT } else { 0 },
            )
        })
        .collect::<Vec<_>>();
    let trace = resolved_batches(Some(7), &batches, plan.locations(), plan.buckets())
        .unwrap()
        .map(|(index, batch, location)| {
            (
                index,
                location.bucket,
                location.layer,
                batch.blend_mode,
                batch.scissor,
                batch.first_index..batch.first_index + batch.index_count,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        trace,
        vec![
            (0, 2, 0, 0, UiScissor::new(0, 2, 30, 40), 0..6),
            (1, 0, 0, 0, UiScissor::new(1, 2, 30, 40), 6..12),
            (
                2,
                1,
                1,
                UI_BLEND_INVERT,
                UiScissor::new(2, 2, 30, 40),
                12..18
            ),
            (3, 1, 0, 0, UiScissor::new(3, 2, 30, 40), 18..24),
            (4, 2, 0, 0, UiScissor::new(4, 2, 30, 40), 24..30),
        ]
    );
    assert!(
        resolved_batches(None, &batches, plan.locations(), plan.buckets()).is_none(),
        "rejected frame emits no commands"
    );
    let mut malformed = batches.clone();
    malformed.last_mut().unwrap().texture_page = 99;
    assert!(
        resolved_batches(Some(7), &malformed, plan.locations(), plan.buckets()).is_none(),
        "invalid late mapping must not emit a partial prefix"
    );
    let mut locations = plan.locations().to_vec();
    locations.push(render_model::UiTextureLocation {
        bucket: 2,
        layer: 1,
    });
    malformed = batches;
    malformed.last_mut().unwrap().texture_page = 4;
    assert!(
        resolved_batches(Some(7), &malformed, &locations, plan.buckets()).is_none(),
        "late layer outside the actual one-layer bucket emits no prefix"
    );
}
