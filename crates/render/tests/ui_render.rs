#[path = "../src/ui.rs"]
pub mod ui;
#[path = "../src/ui_render.rs"]
pub mod ui_render;

use std::sync::Arc;

use bevy::{
    app::SubApp,
    asset::Assets,
    core_pipeline::core_3d::Transparent3d,
    ecs::{schedule::Schedule, system::RunSystemOnce},
    prelude::{App, Shader},
    render::{
        ExtractSchedule, Render, RenderApp, RenderStartup,
        render_phase::DrawFunctions,
        render_resource::BlendFactor,
        renderer::{RenderDevice, RenderQueue, WgpuWrapper},
    },
};
use ui::{
    MAX_UI_INDICES, UiRenderBatch, UiRenderInput, UiRenderRejectReason, UiRenderScene,
    UiRenderStats, UiRenderTextureArray, UiRenderVertex, UiScissor,
};
use ui_render::{
    UiRenderHarness, UiRenderPlugin, prepare_ui_resources, ui_bind_group_layout,
    ui_pipeline_descriptor,
};

#[test]
fn repeated_draw_lists_reuse_shared_gpu_resources_and_preserve_batch_order() {
    let mut harness = UiRenderHarness::new();
    harness.publish(fixture_draw_list(1)).unwrap();
    let first = harness.prepare().unwrap();
    harness.publish(fixture_draw_list(2)).unwrap();
    let second = harness.prepare().unwrap();

    assert_eq!(first.pipeline_id, second.pipeline_id);
    assert_eq!(first.bind_group_family_id, second.bind_group_family_id);
    assert_eq!(first.vertex_arena_id, second.vertex_arena_id);
    assert_eq!(first.index_arena_id, second.index_arena_id);
    assert_eq!(second.per_node_gpu_allocations, 0);
    assert_eq!(second.draw_order(), &[0, 1, 2]);
    assert_eq!(second.scissors()[1], UiScissor::new(4, 5, 20, 21));
}

#[test]
fn shader_parses_and_declares_premultiplied_texture_sampling() {
    let source = include_str!("../src/ui.wgsl");
    let module = naga::front::wgsl::parse_str(source).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    assert!(source.contains("textureSample"));
    assert!(source.contains("sample.rgb * sample.a"));
    assert!(source.contains("viewport_size"));
}

#[test]
fn pipeline_is_one_depth_free_premultiplied_overlay_family() {
    let layout = ui_bind_group_layout();
    assert_eq!(layout.entries.len(), 3);
    let descriptor = ui_pipeline_descriptor(layout);
    assert!(descriptor.depth_stencil.is_none());
    let blend = descriptor.fragment.unwrap().targets[0]
        .as_ref()
        .unwrap()
        .blend
        .unwrap();
    assert_eq!(blend.color.src_factor, BlendFactor::One);
    assert_eq!(blend.color.dst_factor, BlendFactor::OneMinusSrcAlpha);
    assert_eq!(blend.alpha.src_factor, BlendFactor::One);
    assert_eq!(blend.alpha.dst_factor, BlendFactor::OneMinusSrcAlpha);
}

#[test]
fn oversized_or_invalid_publication_retains_last_valid_scene_with_attribution() {
    let mut harness = UiRenderHarness::new();
    harness.publish(fixture_draw_list(7)).unwrap();
    let accepted = harness.prepare().unwrap();

    let mut oversized = fixture_draw_list(8);
    oversized.indices = vec![0; MAX_UI_INDICES + 1].into();
    let rejection = harness.publish(oversized).unwrap_err();
    assert_eq!(
        rejection.reason,
        UiRenderRejectReason::IndexLimitExceeded {
            actual: MAX_UI_INDICES + 1,
            limit: MAX_UI_INDICES,
        }
    );
    assert_eq!(harness.scene().revision, 7);
    assert_eq!(harness.stats().rejected_revision, Some(8));
    assert_eq!(harness.stats().rejected_reason, Some(rejection.reason));
    assert_eq!(harness.prepare().unwrap(), accepted);

    let mut invalid = fixture_draw_list(9);
    let mut batches = invalid.batches.to_vec();
    batches[1].index_count = 13;
    invalid.batches = batches.into();
    let rejection = harness.publish(invalid).unwrap_err();
    assert_eq!(
        rejection.reason,
        UiRenderRejectReason::BatchIndexRangeInvalid { batch: 1 }
    );
    assert_eq!(harness.scene().revision, 7);
    assert_eq!(harness.prepare().unwrap(), accepted);
}

#[test]
fn empty_scene_and_zero_area_batches_do_not_allocate_or_draw() {
    let mut harness = UiRenderHarness::new();
    let mut input = fixture_draw_list(1);
    input.vertices = Arc::from([]);
    input.indices = Arc::from([]);
    input.batches = Arc::from([]);
    harness.publish(input).unwrap();

    let prepared = harness.prepare().unwrap();
    assert_eq!(prepared.draw_order(), &[]);
    assert_eq!(harness.stats().draw_calls, 0);
    assert_eq!(harness.stats().uploaded_vertices, 0);
    assert_eq!(harness.stats().uploaded_indices, 0);
    assert_eq!(prepared.per_node_gpu_allocations, 0);
}

#[test]
fn same_revision_is_an_identical_noop_and_conflicting_content_fails_closed() {
    let mut harness = UiRenderHarness::new();
    let input = fixture_draw_list(12);
    harness.publish(input.clone()).unwrap();
    let prepared = harness.prepare().unwrap();
    let stats = harness.stats();

    harness.publish(input).unwrap();
    assert_eq!(harness.prepare().unwrap(), prepared);
    assert_eq!(harness.stats(), stats);

    let mut conflicting = fixture_draw_list(12);
    let mut vertices = conflicting.vertices.to_vec();
    vertices[0].position = [63.0, 63.0];
    conflicting.vertices = vertices.into();
    let rejection = harness.publish(conflicting).unwrap_err();
    assert_eq!(
        rejection.reason,
        UiRenderRejectReason::RevisionConflict { revision: 12 }
    );
    assert_eq!(harness.scene().revision, 12);
    assert_eq!(harness.prepare().unwrap(), prepared);
}

#[test]
fn later_revision_cannot_reuse_texture_identity_for_different_content() {
    let mut harness = UiRenderHarness::new();
    harness.publish(fixture_draw_list(30)).unwrap();
    let accepted = harness.prepare().unwrap();

    let mut conflicting = fixture_draw_list(31);
    let mut texture = (*conflicting.textures).clone();
    texture.rgba8 = vec![0; texture.rgba8.len()].into();
    conflicting.textures = Arc::new(texture);

    let rejection = harness.publish(conflicting).unwrap_err();
    assert_eq!(
        rejection.reason,
        UiRenderRejectReason::TextureIdentityConflict { identity: [3; 32] }
    );
    assert_eq!(harness.scene().revision, 30);
    assert_eq!(harness.prepare().unwrap(), accepted);

    let mut extent_conflicting = fixture_draw_list(32);
    let mut texture = (*extent_conflicting.textures).clone();
    texture.width = 2;
    texture.rgba8 = vec![255; 16].into();
    extent_conflicting.textures = Arc::new(texture);
    let rejection = harness.publish(extent_conflicting).unwrap_err();
    assert_eq!(
        rejection.reason,
        UiRenderRejectReason::TextureIdentityConflict { identity: [3; 32] }
    );
    assert_eq!(harness.scene().revision, 30);
    assert_eq!(harness.prepare().unwrap(), accepted);
}

#[test]
fn render_preparation_updates_main_world_observable_stats() {
    let mut app = app_with_noop_render_sub_app();
    app.add_plugins(UiRenderPlugin);
    app.finish();
    let stats = app.world().resource::<UiRenderStats>().clone();
    app.world_mut()
        .resource_mut::<UiRenderScene>()
        .publish(fixture_draw_list(21), &stats)
        .unwrap();
    let scene = app.world().resource::<UiRenderScene>().clone();

    let render_app = app.sub_app_mut(RenderApp);
    render_app.world_mut().insert_resource(scene);
    render_app.world_mut().run_schedule(RenderStartup);
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();

    let observed = app.world().resource::<UiRenderStats>().snapshot();
    assert_eq!(observed.accepted_revision, Some(21));
    assert_eq!(observed.uploaded_vertices, 12);
    assert_eq!(observed.uploaded_indices, 18);
    assert_eq!(observed.draw_calls, 3);
    assert!(observed.retained_gpu_bytes > 0);
}

fn fixture_draw_list(revision: u64) -> UiRenderInput {
    let vertices = (0..12)
        .map(|index| UiRenderVertex {
            position: [index as f32, index as f32 + 0.5],
            uv: [index as u16, index as u16],
            color: [255, 128, 64, 192],
            style_flags: 0,
        })
        .collect::<Vec<_>>()
        .into();
    let indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7, 8, 9, 10, 8, 10, 11].into();
    let batches = vec![
        UiRenderBatch::new(0, UiScissor::new(0, 0, 64, 64), 0, 6),
        UiRenderBatch::new(0, UiScissor::new(4, 5, 20, 21), 6, 6),
        UiRenderBatch::new(1, UiScissor::new(0, 0, 64, 64), 12, 6),
    ]
    .into();
    UiRenderInput {
        revision,
        viewport_size: [64, 64],
        safe_area: [0, 0, 0, 0],
        vertices,
        indices,
        batches,
        textures: Arc::new(UiRenderTextureArray {
            identity: [3; 32],
            width: 1,
            height: 1,
            layers: 2,
            rgba8: vec![255; 8].into(),
        }),
    }
}

fn app_with_noop_render_sub_app() -> App {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut render_app = SubApp::new();
    render_app
        .insert_resource(RenderDevice::from(device))
        .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))))
        .insert_resource(DrawFunctions::<Transparent3d>::default())
        .add_schedule(Schedule::new(RenderStartup))
        .add_schedule(Render::base_schedule())
        .add_schedule(Schedule::new(ExtractSchedule));
    let mut app = App::new();
    app.insert_resource(Assets::<Shader>::default())
        .insert_sub_app(RenderApp, render_app);
    app
}
