#[path = "../src/alloc_count.rs"]
mod alloc_count;

#[path = "../src/material_shader.rs"]
#[allow(dead_code, reason = "shared checked shader constructor dependencies")]
mod material_shader;
#[path = "../src/scene_target.rs"]
#[allow(
    dead_code,
    reason = "shared world attachment contract for projected UI"
)]
mod scene_target;
#[path = "../src/pipeline_warmup.rs"]
#[allow(dead_code, reason = "shared pipeline warmup")]
mod pipeline_warmup;
#[path = "../src/shader_safety.rs"]
#[allow(dead_code, reason = "shared checked shader constructors")]
mod shader_safety;
#[path = "../src/ui_render.rs"]
pub mod ui_render;

// This standalone UI fixture installs no camera-effect scene. Production's
// post-hand camera pass is exercised by the render library and live client.
mod screen_overlay_render {
    pub(crate) fn draw_before_hud(
        _: bevy::prelude::Entity,
        _: &bevy::render::view::ViewTarget,
        _: &bevy::render::camera::ExtractedCamera,
        _: Option<&bevy::camera::MainPassResolutionOverride>,
        _: &mut bevy::render::renderer::RenderContext,
        _: &bevy::prelude::World,
    ) {
    }
}

mod gpu_timing {
    /// The isolated UI fixture installs no GPU timing query ring.
    pub(crate) fn render_pass_timestamps(
        _: &bevy::prelude::World,
        _: render::RuntimeStage,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        None
    }
}

use render::{EnhancedRendering, RuntimeStage};
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
use render_model::{
    MAX_UI_INDICES, UiRenderBatch, UiRenderInput, UiRenderRejectReason, UiRenderScene,
    UiRenderStats, UiRenderTextureArray, UiRenderVertex, UiScissor, UiTexturePage,
};
use ui_render::harness::UiRenderHarness;
use ui_render::pipeline::{ui_bind_group_layout, ui_pipeline_descriptor};
use ui_render::resources::prepare_ui_resources;
use ui_render::{UiRenderPlugin, UiRenderSceneResource, UiRenderStatsResource};

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
    let source = ui_render::shader::source(include_str!("../src/ui.wgsl"));
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    assert!(source.contains("textureSample"));
    assert!(source.contains("model_rgb * sample.a * straight_color.a"));
    assert!(source.contains("viewport_size"));
    assert!(source.contains(&format!(
        "const STYLE_ALPHA_TEST: u32 = {}u;",
        render_model::UI_STYLE_ALPHA_TEST
    )));
    assert!(source.contains("sample.a < 0.5"));
    assert!(source.find("discard;").unwrap() < source.find("let alpha =").unwrap());
    let (_, viewport) = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some("UiViewport"))
        .unwrap();
    let naga::TypeInner::Struct { members, span } = &viewport.inner else {
        panic!("UI viewport must remain a uniform struct");
    };
    assert_eq!(*span, 16);
    assert_eq!(members[2].name.as_deref(), Some("glint_strength"));
    assert_eq!(members[2].offset, 12);
}

// The UI layer composites over the scene in sRGB-encoded values.
#[test]
fn composite_shader_blends_in_gamma_space() {
    let source = include_str!("../src/ui_composite.wgsl");
    let module = naga::front::wgsl::parse_str(source).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    assert!(source.contains("ui.rgb + linear_to_srgb(under.rgb) * (1.0 - ui.a)"));
}

#[test]
fn pipeline_is_one_depth_neutral_premultiplied_overlay_family() {
    let layout = ui_bind_group_layout();
    // Viewport, pages, the nearest and `bilinear` samplers, and the page format.
    assert_eq!(layout.entries.len(), 5);
    let descriptor = ui_pipeline_descriptor(layout);
    assert!(
        descriptor.depth_stencil.is_none(),
        "dedicated HUD pass has no depth"
    );
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
fn homogeneous_world_vertices_validate_without_dividing_at_the_camera() {
    let mut input = fixture_draw_list(1);
    let mut vertices = input.vertices.to_vec();
    vertices[0].clip_w = 0.0;
    vertices[1].clip_w = -1.0;
    vertices[2].clip_z = 0.25;
    input.vertices = vertices.clone().into();
    let mut batches = input.batches.to_vec();
    batches[0] = batches[0].with_depth_test(true);
    batches[1] = batches[1].with_world_projection(true);
    input.batches = batches.into();
    input.validate().unwrap();
    vertices[0].clip_w = f32::NAN;
    input.vertices = vertices.into();
    assert_eq!(input.validate(), Err(UiRenderRejectReason::NonFiniteVertex));
}

#[test]
fn ui_model_depth_scopes_are_ordered_private_and_cannot_reenter_after_overlay() {
    let mut input = fixture_draw_list(1);
    let mut batches = input.batches.to_vec();
    batches[0] = batches[0]
        .with_depth_test(true)
        .with_depth_write(true)
        .with_isolated_depth_scope(Some(7));
    batches[1] = batches[1]
        .with_depth_test(true)
        .with_isolated_depth_scope(Some(7));
    input.batches = batches.clone().into();
    input.validate().unwrap();
    assert_eq!(input.batches[0].world_projection, 0);
    batches[0].world_projection = 1;
    input.batches = batches.clone().into();
    assert_eq!(
        input.validate(),
        Err(UiRenderRejectReason::UnsupportedWorldProjection { batch: 0 })
    );
    batches[0].world_projection = 0;
    batches[1] = batches[1]
        .with_depth_test(false)
        .with_world_projection(false)
        .with_isolated_depth_scope(None);
    batches[2] = batches[2].with_isolated_depth_scope(Some(7));
    input.batches = batches.into();
    assert_eq!(
        input.validate(),
        Err(UiRenderRejectReason::InvalidIsolatedDepthScope { batch: 2 })
    );
}

#[test]
fn ui_model_material_cutoff_is_finite_and_independent_of_glyph_half_alpha() {
    let mut input = fixture_draw_list(1);
    let mut vertices = input.vertices.to_vec();
    vertices[0].alpha_cutoff = 0.1;
    input.vertices = vertices.clone().into();
    input.validate().unwrap();
    vertices[0].alpha_cutoff = f32::NAN;
    input.vertices = vertices.into();
    assert_eq!(input.validate(), Err(UiRenderRejectReason::NonFiniteVertex));
    let source = ui_render::shader::source(include_str!("../src/ui.wgsl"));
    assert!(source.contains("if input.alpha_cutoff >= 0.0"));
    assert!(source.contains("sample.a < input.alpha_cutoff"));
    assert!(source.contains("else if (input.style_flags & STYLE_ALPHA_TEST)"));
}

#[test]
fn ui_model_light_preserves_float_vertex_values_and_rejects_invalid_multipliers() {
    let mut input = fixture_draw_list(1);
    let mut vertices = input.vertices.to_vec();
    vertices[0].model_light = 0.718_629;
    input.vertices = vertices.clone().into();
    input.validate().unwrap();
    assert_eq!(
        input.vertices[0].model_light.to_bits(),
        0.718_629_f32.to_bits()
    );
    for value in [f32::NAN, f32::INFINITY, -0.1] {
        vertices[0].model_light = value;
        input.vertices = vertices.clone().into();
        assert_eq!(input.validate(), Err(UiRenderRejectReason::NonFiniteVertex));
    }
    let source = ui_render::shader::source(include_str!("../src/ui.wgsl"));
    assert!(source.contains("@location(5) model_light: f32"));
    assert!(source.contains("straight_color.a * input.model_light"));
}

#[test]
fn ui_model_fire_overlay_is_float_and_rejects_non_finite_channels() {
    let mut input = fixture_draw_list(1);
    let mut vertices = input.vertices.to_vec();
    vertices[0].overlay_color = [0.8, 0.248_176, 0.0, 0.7];
    input.vertices = vertices.clone().into();
    input.validate().unwrap();
    assert_eq!(input.vertices[0].overlay_color, vertices[0].overlay_color);
    for channel in 0..4 {
        vertices[0].overlay_color[channel] = f32::NAN;
        input.vertices = vertices.clone().into();
        assert_eq!(input.validate(), Err(UiRenderRejectReason::NonFiniteVertex));
        vertices[0].overlay_color[channel] = 0.0;
    }
}

#[test]
fn ui_model_uv_keeps_native_side_pixel_centers_without_rounding_or_half_texel_shift() {
    let mut input = fixture_draw_list(1);
    let mut vertices = input.vertices.to_vec();
    vertices[0].uv = [8.5, 4.5];
    input.vertices = vertices.clone().into();
    input.validate().unwrap();
    assert_eq!(input.vertices[0].uv, [8.5, 4.5]);
    vertices[0].uv[1] = f32::NAN;
    input.vertices = vertices.into();
    assert_eq!(input.validate(), Err(UiRenderRejectReason::NonFiniteVertex));
    let source = ui_render::shader::source(include_str!("../src/ui.wgsl"));
    assert!(source.contains("@location(1) uv: vec2<f32>"));
    assert!(source.contains("output.uv = uv;"));
    assert!(source.contains("let normalized_uv = input.uv / dimensions;"));
}

#[test]
fn world_depth_and_projection_batch_modes_are_validated_separately() {
    let mut input = fixture_draw_list(1);
    let mut batches = input.batches.to_vec();
    batches[0].depth_test = 2;
    input.batches = batches.clone().into();
    assert_eq!(
        input.validate(),
        Err(UiRenderRejectReason::UnsupportedDepthTest { batch: 0 })
    );
    batches[0].depth_test = 1;
    input.batches = batches.clone().into();
    assert_eq!(
        input.validate(),
        Err(UiRenderRejectReason::UnsupportedWorldProjection { batch: 0 })
    );
    batches[0].world_projection = 1;
    batches[1] = batches[1].with_depth_write(true);
    input.batches = batches.clone().into();
    input.validate().unwrap();
    batches[1].depth_write = 2;
    input.batches = batches.clone().into();
    assert_eq!(
        input.validate(),
        Err(UiRenderRejectReason::UnsupportedDepthWrite { batch: 1 })
    );
    batches[1].depth_write = 1;
    batches[1].world_projection = 0;
    input.batches = batches.clone().into();
    assert_eq!(
        input.validate(),
        Err(UiRenderRejectReason::UnsupportedWorldProjection { batch: 1 })
    );
    batches[1].world_projection = 1;
    input.batches = batches.into();
    input.validate().unwrap();
}

#[test]
fn oversized_or_invalid_publication_withholds_scene_with_attribution() {
    let mut harness = UiRenderHarness::new();
    harness.publish(fixture_draw_list(7)).unwrap();
    harness.prepare().unwrap();

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
    assert!(harness.prepare().is_err());

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
    assert!(harness.prepare().is_err());
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
    assert_eq!(prepared.draw_order(), &[] as &[usize]);
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
    assert!(harness.prepare().is_err());
    assert!(
        harness.publish(fixture_draw_list(12)).is_err(),
        "rejected revision cannot regrant an old accepted draw"
    );
    harness.publish(fixture_draw_list(13)).unwrap();
    assert_eq!(harness.prepare().unwrap().revision, 13);
}

#[test]
fn later_revision_cannot_replace_static_catalog_or_dimensions() {
    let mut harness = UiRenderHarness::new();
    harness.publish(fixture_draw_list(30)).unwrap();
    harness.prepare().unwrap();
    let mut conflicting = fixture_draw_list(31);
    conflicting.textures = Arc::new(
        UiRenderTextureArray::new(
            vec![UiTexturePage::owned([1, 1], vec![0; 4].into()).unwrap(); 2],
            2,
        )
        .unwrap(),
    );
    let identity = conflicting.textures.identity();
    assert_eq!(
        harness.publish(conflicting).unwrap_err().reason,
        UiRenderRejectReason::TextureIdentityConflict { identity }
    );
    assert_eq!(harness.scene().revision, 30);
    assert!(harness.prepare().is_err());
    // Restoring an original valid candidate does not permit shape migration.
    harness.publish(fixture_draw_list(32)).unwrap();
    let mut changed_shape = fixture_draw_list(33);
    changed_shape.textures = Arc::new(
        UiRenderTextureArray::new(
            vec![UiTexturePage::owned([2, 1], vec![255; 8].into()).unwrap(); 2],
            2,
        )
        .unwrap(),
    );
    assert!(harness.publish(changed_shape).is_err());
    assert!(harness.prepare().is_err());
}

#[test]
fn render_preparation_updates_main_world_observable_stats() {
    let mut app = app_with_noop_render_sub_app();
    app.add_plugins(UiRenderPlugin);
    app.finish();
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    app.world_mut()
        .resource_mut::<UiRenderSceneResource>()
        .publish(fixture_draw_list(21), &stats)
        .unwrap();
    let scene = app.world().resource::<UiRenderSceneResource>().clone();

    let render_app = app.sub_app_mut(RenderApp);
    render_app.world_mut().insert_resource(scene);
    render_app.world_mut().run_schedule(RenderStartup);
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();

    let observed = app.world().resource::<UiRenderStatsResource>().snapshot();
    assert_eq!(observed.accepted_revision, Some(21));
    assert_eq!(observed.uploaded_vertices, 12);
    assert_eq!(observed.uploaded_indices, 18);
    assert_eq!(observed.draw_calls, 3);
    assert!(observed.retained_gpu_bytes > 0);
}

#[test]
fn current_device_loss_or_invalid_scene_withholds_old_prepared_draws() {
    let mut app = app_with_noop_render_sub_app();
    app.add_plugins(UiRenderPlugin);
    app.finish();
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    let render_app = app.sub_app_mut(RenderApp);
    render_app.world_mut().run_schedule(RenderStartup);
    let mut scene = UiRenderScene::default();
    scene.publish(fixture_draw_list(1), &stats).unwrap();
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene.clone()));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(stats.snapshot().accepted_revision, Some(1));
    let mut invalid = fixture_draw_list(1);
    invalid.indices = vec![u32::MAX].into();
    scene.input = Some(Arc::new(invalid));
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(stats.snapshot().accepted_revision, None);
    assert_eq!(stats.snapshot().draw_calls, 0);
    let mut scene = UiRenderScene::default();
    scene.publish(fixture_draw_list(3), &stats).unwrap();
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(stats.snapshot().accepted_revision, Some(3));
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    render_app
        .world_mut()
        .insert_resource(RenderDevice::from(device));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(stats.snapshot().accepted_revision, None);
    assert_eq!(stats.snapshot().draw_calls, 0);
    for _ in 0..3 {
        render_app
            .world_mut()
            .run_system_once(prepare_ui_resources)
            .unwrap();
        assert_eq!(stats.snapshot().accepted_revision, None);
    }
    render_app
        .world_mut()
        .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))));
    render_app.world_mut().run_schedule(RenderStartup);
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(
        stats.snapshot().accepted_revision,
        Some(3),
        "paired device/queue and real renderer startup may recover"
    );
}

#[test]
fn cloned_device_resource_replacement_on_empty_frame_stays_invalid_until_startup() {
    let mut app = app_with_noop_render_sub_app();
    app.add_plugins(UiRenderPlugin);
    app.finish();
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    let render_app = app.sub_app_mut(RenderApp);
    render_app.world_mut().run_schedule(RenderStartup);
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource::default());
    let device = render_app.world().resource::<RenderDevice>().clone();
    render_app.world_mut().increment_change_tick();
    render_app.world_mut().insert_resource(device);
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    let mut scene = UiRenderScene::default();
    scene.publish(fixture_draw_list(1), &stats).unwrap();
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene));
    for _ in 0..10 {
        render_app
            .world_mut()
            .run_system_once(prepare_ui_resources)
            .unwrap();
        assert_eq!(stats.snapshot().accepted_revision, None);
        assert_eq!(stats.snapshot().draw_calls, 0);
    }
    render_app.world_mut().run_schedule(RenderStartup);
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(stats.snapshot().accepted_revision, Some(1));
}

#[test]
fn same_revision_requires_exact_accepted_publication_not_equivalent_catalog() {
    let mut app = app_with_noop_render_sub_app();
    app.add_plugins(UiRenderPlugin);
    app.finish();
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    let render_app = app.sub_app_mut(RenderApp);
    render_app.world_mut().run_schedule(RenderStartup);
    let mut scene = UiRenderScene::default();
    scene.publish(fixture_draw_list(1), &stats).unwrap();
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene.clone()));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    let accepted = stats.snapshot();
    for _ in 0..10 {
        let publication = Arc::clone(scene.input.as_ref().unwrap());
        scene.publish(fixture_draw_list(1), &stats).unwrap();
        assert!(Arc::ptr_eq(scene.input.as_ref().unwrap(), &publication));
        render_app
            .world_mut()
            .insert_resource(UiRenderSceneResource(scene.clone()));
        render_app
            .world_mut()
            .run_system_once(prepare_ui_resources)
            .unwrap();
        assert_eq!(
            stats.snapshot(),
            accepted,
            "same immutable publication is a no-op"
        );
    }
    let original = Arc::clone(scene.input.as_ref().unwrap());
    let mut malformed = original.as_ref().clone();
    malformed.indices = vec![u32::MAX].into();
    scene.input = Some(Arc::new(malformed));
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene.clone()));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(stats.snapshot().accepted_revision, None);
    let mut conflicting = original.as_ref().clone();
    conflicting.viewport_size = [65, 64];
    conflicting.validate().unwrap();
    scene.input = Some(Arc::new(conflicting));
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene.clone()));
    for _ in 0..10 {
        render_app
            .world_mut()
            .run_system_once(prepare_ui_resources)
            .unwrap();
        assert_eq!(stats.snapshot().accepted_revision, None);
        assert_eq!(stats.snapshot().draw_calls, 0);
        assert_eq!(
            stats.snapshot().rejected_reason,
            Some(UiRenderRejectReason::RevisionConflict { revision: 1 })
        );
    }
    let conflict = Arc::clone(scene.input.as_ref().unwrap());
    scene.input = Some(Arc::clone(&original));
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene.clone()));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(
        stats.snapshot().accepted_revision,
        Some(1),
        "exact admitted publication may recover after transient refusal"
    );
    let expired = Arc::downgrade(&original);
    drop(original);
    scene.input = Some(conflict);
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene.clone()));
    assert!(
        expired.upgrade().is_none(),
        "no pixel publication history is retained by renderer"
    );
    for _ in 0..10 {
        render_app
            .world_mut()
            .run_system_once(prepare_ui_resources)
            .unwrap();
        assert_eq!(stats.snapshot().accepted_revision, None);
        assert_eq!(stats.snapshot().draw_calls, 0);
    }
    scene.input = None;
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene.clone()));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    scene.input = Some(Arc::new(fixture_draw_list(0)));
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene.clone()));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(stats.snapshot().accepted_revision, None);
    assert_eq!(
        stats.snapshot().rejected_reason,
        Some(UiRenderRejectReason::StaleRevision {
            current: 1,
            rejected: 0
        })
    );
    scene.publish(fixture_draw_list(2), &stats).unwrap();
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene.clone()));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(
        stats.snapshot().accepted_revision,
        Some(2),
        "fresh revision legitimately recovers"
    );
    assert_eq!(stats.snapshot().draw_calls, 3);
    render_app.world_mut().run_schedule(RenderStartup);
    let mut fresh_scene = UiRenderScene::default();
    fresh_scene.publish(fixture_draw_list(1), &stats).unwrap();
    render_app
        .world_mut()
        .insert_resource(UiRenderSceneResource(fresh_scene.clone()));
    render_app
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    assert_eq!(
        stats.snapshot().accepted_revision,
        Some(1),
        "only actual renderer and publisher recreation starts a fresh revision lifetime"
    );
}

fn fixture_draw_list(revision: u64) -> UiRenderInput {
    let vertices = (0..12)
        .map(|index| UiRenderVertex {
            position: [index as f32, index as f32 + 0.5],
            clip_z: 0.0,
            clip_w: 1.0,
            uv: [index as f32, index as f32],
            color: [255, 128, 64, 192],
            style_flags: 0,
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        })
        .collect::<Vec<_>>()
        .into();
    let indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7, 8, 9, 10, 8, 10, 11].into();
    let batches = vec![
        UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 64, 64),
            0,
            6,
            render_model::UI_BLEND_ALPHA,
        ),
        UiRenderBatch::new(
            0,
            UiScissor::new(4, 5, 20, 21),
            6,
            6,
            render_model::UI_BLEND_INVERT,
        ),
        UiRenderBatch::new(
            1,
            UiScissor::new(0, 0, 64, 64),
            12,
            6,
            render_model::UI_BLEND_ALPHA,
        ),
    ]
    .into();
    UiRenderInput {
        revision,
        viewport_size: [64, 64],
        safe_area: [0, 0, 0, 0],
        vertices,
        indices,
        batches,
        textures: Arc::new(
            UiRenderTextureArray::new(
                vec![UiTexturePage::owned([1, 1], vec![255; 4].into()).unwrap(); 2],
                2,
            )
            .unwrap(),
        ),
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

#[test]
fn ui_only_plugin_never_registers_a_duplicate_transparent_draw() {
    use bevy::{
        core_pipeline::core_3d::graph::{Core3d, Node3d},
        render::render_graph::{EmptyNode, RenderGraph},
    };
    let mut app = app_with_noop_render_sub_app();
    let mut core = RenderGraph::default();
    core.add_node(Node3d::MainTransparentPass, EmptyNode);
    core.add_node(Node3d::EndMainPass, EmptyNode);
    let mut graphs = RenderGraph::default();
    graphs.add_sub_graph(Core3d, core);
    app.sub_app_mut(RenderApp).insert_resource(graphs);
    app.add_plugins(UiRenderPlugin);
    app.finish();
    // Compare the first assigned ID against an independently empty registry,
    // rather than querying for a type that could never have been registered.
    let empty = DrawFunctions::<Transparent3d>::default();
    let expected = empty.write().add(TestTransparentDraw);
    let actual = app
        .sub_app_mut(RenderApp)
        .world_mut()
        .resource::<DrawFunctions<Transparent3d>>()
        .write()
        .add(TestTransparentDraw);
    assert_eq!(actual, expected);
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<RenderGraph>()
            .get_sub_graph(Core3d)
            .unwrap()
            .get_node_state(ui_render::UiOverlayLabel)
            .is_ok()
    );
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<RenderGraph>()
            .get_sub_graph(Core3d)
            .unwrap()
            .get_node_state(ui_render::UiWorldLabel)
            .is_ok()
    );
}

struct TestTransparentDraw;
#[test]
fn unchanged_view_pipeline_pairs_update_in_place_and_departed_views_are_removed() {
    use bevy::{prelude::Entity, render::render_resource::CachedRenderPipelineId};
    use std::collections::BTreeMap;
    use ui_render::overlay::{cache_view_pipeline_pair, retain_view_pipeline_entries};
    let live = Entity::from_raw_u32(0).unwrap();
    let departed = Entity::from_raw_u32(1).unwrap();
    let pair = (
        CachedRenderPipelineId::INVALID,
        CachedRenderPipelineId::INVALID,
    );
    let mut entries = BTreeMap::new();
    cache_view_pipeline_pair(&mut entries, live, pair);
    cache_view_pipeline_pair(&mut entries, departed, pair);
    let retained_address = entries.get(&live).unwrap() as *const _;
    for _ in 0..16 {
        retain_view_pipeline_entries(&mut entries, |view| view == live || view == departed);
        cache_view_pipeline_pair(&mut entries, live, pair);
        assert_eq!(entries.get(&live).unwrap() as *const _, retained_address);
        assert_eq!(entries.len(), 2);
    }
    retain_view_pipeline_entries(&mut entries, |view| view == live);
    assert_eq!(entries.len(), 1);
    assert!(!entries.contains_key(&departed));
    assert_eq!(*entries.get(&live).unwrap(), pair);
}
#[test]
fn ui_only_overlay_preserves_partial_camera_viewport_and_resolution_override() {
    use bevy::{
        camera::{MainPassResolutionOverride, Viewport},
        prelude::UVec2,
    };
    use ui_render::overlay::overlay_viewport;
    let viewport = Viewport {
        physical_position: UVec2::new(13, 27),
        physical_size: UVec2::new(300, 200),
        depth: 0.2..0.8,
    };
    let copied = overlay_viewport(Some(&viewport), None).unwrap();
    assert_eq!(copied.physical_position, viewport.physical_position);
    assert_eq!(copied.physical_size, viewport.physical_size);
    assert_eq!(copied.depth, viewport.depth);
    let override_size = MainPassResolutionOverride(UVec2::new(150, 100));
    let smaller = overlay_viewport(Some(&viewport), Some(&override_size)).unwrap();
    assert_eq!(smaller.physical_position, viewport.physical_position);
    assert_eq!(smaller.physical_size, override_size.0);
    assert_eq!(smaller.depth, viewport.depth);
    let whole_override = overlay_viewport(None, Some(&override_size)).unwrap();
    assert_eq!(whole_override.physical_position, UVec2::ZERO);
    assert_eq!(whole_override.physical_size, override_size.0);
    assert!(overlay_viewport(None, None).is_none());
}
#[test]
fn empty_overlay_never_selects_retained_pipeline_after_target_change() {
    use bevy::{
        camera::{MainPassResolutionOverride, Viewport},
        prelude::{Entity, UVec2},
    };
    use std::collections::BTreeMap;
    use ui_render::overlay::{overlay_pipeline_pair, overlay_viewport};
    let view = Entity::from_raw_u32(0).unwrap();
    let batch = UiRenderBatch::new(
        0,
        UiScissor::new(0, 0, 64, 64),
        0,
        6,
        render_model::UI_BLEND_ALPHA,
    );
    let mut entries = BTreeMap::new();
    // Target specialization keys represent the retained pipeline pair's actual
    // compatibility class. This tests selection, not GPU compilation.
    entries.insert(view, (false, 1u32));
    assert_eq!(
        overlay_pipeline_pair(&[batch], &entries, view),
        Some(&(false, 1))
    );
    let changed_target = (true, 4u32);
    assert_ne!(*entries.get(&view).unwrap(), changed_target);
    assert!(overlay_pipeline_pair(&[], &entries, view).is_none());
    assert_eq!(entries.len(), 1);
    let viewport = Viewport {
        physical_position: UVec2::new(4, 8),
        physical_size: UVec2::new(64, 64),
        ..Default::default()
    };
    let override_size = MainPassResolutionOverride(UVec2::new(32, 32));
    let effective = overlay_viewport(Some(&viewport), Some(&override_size)).unwrap();
    assert_eq!(effective.physical_position, viewport.physical_position);
    assert_eq!(effective.physical_size, override_size.0);
    // Nonempty preparation must supply the new specialization before selection.
    *entries.get_mut(&view).unwrap() = changed_target;
    assert_eq!(
        overlay_pipeline_pair(&[batch], &entries, view),
        Some(&changed_target)
    );
    assert_eq!(
        overlay_viewport(Some(&viewport), Some(&override_size))
            .unwrap()
            .depth,
        effective.depth
    );
}
#[test]
fn current_hand_coverage_omits_only_its_quad_and_missing_stale_coverage_keeps_cpu() {
    use bevy::prelude::Entity;
    use ui_render::{UiHandCoverage, overlay::retained_batch_ranges};
    let view = Entity::from_raw_u32(0).unwrap();
    let main = Entity::from_raw_u32(1).unwrap();
    let coverage = UiHandCoverage::default();
    let batch = UiRenderBatch::new(
        1,
        UiScissor::new(2, 3, 4, 5),
        0,
        18,
        render_model::UI_BLEND_ALPHA,
    );
    assert!(coverage.range(view, main, Some(7), &[batch], 18).is_none());
    coverage.clear();
    coverage.record(view, main, 7, 6, 1);
    let range = coverage.range(view, main, Some(7), &[batch], 18).unwrap();
    assert_eq!(
        retained_batch_ranges(&batch, Some(&range)),
        [Some(0..6), Some(12..18)]
    );
    assert_eq!(retained_batch_ranges(&batch, None), [Some(0..18), None]);
    assert!(coverage.range(main, main, Some(7), &[batch], 18).is_none());
    assert!(coverage.range(view, view, Some(7), &[batch], 18).is_none());
    assert!(coverage.range(view, main, Some(8), &[batch], 18).is_none());
    assert!(
        coverage
            .range(view, main, Some(7), &[batch, batch], 18)
            .is_none()
    );
    assert!(coverage.range(view, main, Some(7), &[batch], 11).is_none());
    coverage.clear();
    assert!(coverage.range(view, main, Some(7), &[batch], 18).is_none());
    // An unchanged UI revision/view cannot reuse last render-frame coverage.
    assert_eq!(retained_batch_ranges(&batch, None), [Some(0..18), None]);
    assert_eq!(batch.texture_page, 1);
    assert_eq!(batch.scissor, UiScissor::new(2, 3, 4, 5));
    assert_eq!(batch.blend_mode, render_model::UI_BLEND_ALPHA);
}

#[test]
fn actual_prepare_clears_hand_coverage_on_unchanged_empty_and_rejected_input() {
    use bevy::prelude::Entity;
    use ui_render::UiHandCoverage;
    let mut app = app_with_noop_render_sub_app();
    app.add_plugins(UiRenderPlugin);
    app.finish();
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    let render = app.sub_app_mut(RenderApp);
    render.world_mut().run_schedule(RenderStartup);
    let mut scene = UiRenderScene::default();
    let input = fixture_draw_list(1);
    scene.publish(input.clone(), &stats).unwrap();
    render
        .world_mut()
        .insert_resource(UiRenderSceneResource(scene.clone()));
    render
        .world_mut()
        .run_system_once(prepare_ui_resources)
        .unwrap();
    let view = Entity::from_raw_u32(0).unwrap();
    let main = Entity::from_raw_u32(1).unwrap();
    for state in 0..3 {
        let coverage = render.world().resource::<UiHandCoverage>();
        coverage.record(view, main, 1, 12, 1);
        assert_eq!(
            coverage.range(view, main, Some(1), &input.batches, 18),
            Some(12..18)
        );
        if state == 1 {
            scene.input = None;
        }
        if state == 2 {
            let mut invalid = input.clone();
            invalid.indices = Arc::from([u32::MAX]);
            scene.input = Some(Arc::new(invalid));
        }
        render
            .world_mut()
            .insert_resource(UiRenderSceneResource(scene.clone()));
        render
            .world_mut()
            .run_system_once(prepare_ui_resources)
            .unwrap();
        assert!(
            render
                .world()
                .resource::<UiHandCoverage>()
                .range(view, main, Some(1), &input.batches, 18)
                .is_none()
        );
        if state == 0 {
            assert_eq!(stats.snapshot().accepted_revision, Some(1));
        } else {
            assert_eq!(stats.snapshot().accepted_revision, None);
        }
    }
}
impl bevy::render::render_phase::Draw<Transparent3d> for TestTransparentDraw {
    fn draw<'w>(
        &mut self,
        _world: &'w bevy::prelude::World,
        _pass: &mut bevy::render::render_phase::TrackedRenderPass<'w>,
        _view: bevy::prelude::Entity,
        _item: &Transparent3d,
    ) -> Result<(), bevy::render::render_phase::DrawError> {
        Ok(())
    }
}

#[test]
fn review_render_rejection_does_not_forget_static_texture_identity() {
    let mut scene = UiRenderScene::default();
    let stats = UiRenderStats::default();
    scene.publish(fixture_draw_list(30), &stats).unwrap();
    let mut conflicting = fixture_draw_list(31);
    conflicting.textures = Arc::new(
        UiRenderTextureArray::new(
            vec![UiTexturePage::owned([1, 1], vec![0; 4].into()).unwrap(); 2],
            2,
        )
        .unwrap(),
    );
    assert!(scene.publish(conflicting.clone(), &stats).is_err());
    assert!(scene.publish(conflicting.clone(), &stats).is_err());
    conflicting.revision += 1;
    assert!(scene.publish(conflicting, &stats).is_err());
    scene.publish(fixture_draw_list(33), &stats).unwrap();
}
