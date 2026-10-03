use std::sync::Arc;

use crate::shader_source;

use bevy::{
    app::SubApp,
    asset::Assets,
    core_pipeline::core_3d::{Opaque3d, Transparent3d},
    ecs::schedule::Schedule,
    prelude::{App, Shader},
    render::{
        ExtractSchedule, Render, RenderApp, RenderStartup,
        render_phase::DrawFunctions,
        renderer::{RenderDevice, RenderQueue, WgpuWrapper},
    },
};

use super::{
    ACTOR_SHADER_SOURCE, ActorGpu, ActorPipelineKey, ActorPipelineSpecializer,
    ActorRenderInstalled, ActorRenderPlugin, actor_bind_group_layout, actor_pipeline_descriptor,
    actor_skin_upload_plan,
};

#[test]
fn shared_skin_layer_prepares_one_texture_layer_for_multiple_actors() {
    let mut frame = crate::actor::ActorRenderFrame::default();
    frame.rig.instances = Arc::from([
        crate::actor::ActorGpuInstance {
            texture_layer: 0,
            ..Default::default()
        },
        crate::actor::ActorGpuInstance {
            texture_layer: 0,
            ..Default::default()
        },
    ]);
    frame.skins_rgba8 = vec![255; crate::actor::STANDARD_SKIN_BYTES].into();

    let plan =
        actor_skin_upload_plan(&frame).expect("a shared normalized skin family remains drawable");

    assert_eq!(plan.layer_count, 1);
}

#[test]
fn skin_upload_preparation_rejects_misaligned_bytes_and_out_of_range_layers() {
    let mut frame = crate::actor::ActorRenderFrame::default();
    frame.rig.instances = Arc::from([crate::actor::ActorGpuInstance {
        texture_layer: 0,
        ..Default::default()
    }]);
    frame.skins_rgba8 = vec![255; crate::actor::STANDARD_SKIN_BYTES - 1].into();
    assert!(actor_skin_upload_plan(&frame).is_none());

    frame.skins_rgba8 = vec![255; crate::actor::STANDARD_SKIN_BYTES].into();
    Arc::make_mut(&mut frame.rig.instances)[0].texture_layer = 1;
    assert!(actor_skin_upload_plan(&frame).is_none());
}

#[test]
fn generic_only_frames_do_not_require_or_reinterpret_player_skin_bytes() {
    let mut frame = crate::actor::ActorRenderFrame::default();
    frame.rig.instances = Arc::from([crate::actor::ActorGpuInstance {
        texture_layer: 17,
        ..Default::default()
    }]);
    frame.instance_pages = Arc::from([1]);
    let plan = actor_skin_upload_plan(&frame).expect("generic layer is not a player-skin layer");
    assert_eq!(plan.layer_count, 0);
    frame.instance_pages = Arc::from([0]);
    assert!(actor_skin_upload_plan(&frame).is_none());
}

#[test]
fn first_generic_only_frame_prepares_after_an_empty_skin_revision() {
    use crate::actor::{
        ActorDrawManifestEntry, ActorRenderIdentity, ActorRigRoute, ActorRigVertex, EntityRigId,
    };
    use bevy::ecs::system::RunSystemOnce;
    let mut app = app_with_noop_render_sub_app();
    app.add_plugins(ActorRenderPlugin);
    app.finish();
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.run_schedule(RenderStartup);
    world.resource_mut::<ActorGpu>().skin_revision = 0;
    let mut frame = crate::actor::ActorRenderFrame::default();
    frame.rig.frame_generation = 1;
    frame.rig.geometry_revision = 1;
    frame.rig.maximum_vertex_count = 3;
    frame.rig.instances = Arc::from([crate::actor::ActorGpuInstance {
        texture_layer: 0,
        ..Default::default()
    }]);
    frame.instance_pages = Arc::from([1]);
    frame.rig.previous_bones = Arc::from([[[0.0; 4]; 3]]);
    frame.rig.current_bones = Arc::clone(&frame.rig.previous_bones);
    frame.rig.manifest = Arc::from([ActorDrawManifestEntry {
        identity: ActorRenderIdentity {
            session_id: 1,
            dimension: 0,
            runtime_id: 2,
            spawn_revision: 3,
            ingress_sequence: 4,
            source_tick: None,
            movement_revision: 0,
            pose_generation: 1,
            layer: 0,
        },
        rig: EntityRigId(0),
        completed_tick: 1,
        reset_generation: 1,
        route: ActorRigRoute::Compiled,
        instance_index: 0,
        previous_bone_base: 0,
        current_bone_base: 0,
        bone_count: 1,
    }]);
    frame.rig.geometry_vertices = crate::actor::ActorRigVertexSegments::from_vertices(
        [ActorRigVertex {
            position: [0.0; 3],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0; 2],
            back_uv: [0.0; 2],
            bone_index: 0,
        }; 3],
    );
    frame.rig.geometry_spans = Arc::from([crate::actor::ActorRigGeometrySpan {
        first_vertex: 0,
        vertex_count: 3,
    }]);
    let mut artwork = crate::actor::ActorArtworkPages::default();
    artwork.identity = [1; 32];
    artwork.entity_identity = [2; 32];
    artwork.pages = Arc::from([crate::actor::ActorTexturePage {
        width: 16,
        height: 16,
        layers: 1,
        rgba8: vec![255; 1024].into(),
    }]);
    frame.artwork = Arc::new(artwork);
    world.insert_resource(frame);
    world
        .run_system_once(super::prepare_actor_resources)
        .unwrap();
    let gpu = world.resource::<ActorGpu>();
    assert_eq!(gpu.instance_count, 1);
    assert!(gpu.skin_view.is_some());
    assert_eq!(gpu.artwork.pages.len(), 1);
    let draw = crate::actor::ActorDrawFrame {
        artwork_identity: gpu.artwork_identity,
        skin_revision: gpu.skin_revision,
        geometry_revision: gpu.geometry_revision,
        frame_generation: gpu.frame_generation,
        draw_generation: 1,
        manifest: Arc::clone(&gpu.manifest),
    };
    let gate = world
        .resource::<crate::actor::ActorPresentationGate>()
        .clone();
    let old = gate.try_reserve_callback(draw).unwrap();
    let mut replacement = world
        .resource::<crate::actor::ActorRenderFrame>()
        .artwork
        .as_ref()
        .clone();
    replacement.identity = [3; 32];
    replacement.entity_identity = [4; 32];
    world
        .resource_mut::<crate::actor::ActorRenderFrame>()
        .artwork = Arc::new(replacement);
    world
        .run_system_once(super::prepare_actor_resources)
        .unwrap();
    // A session pack's artwork replaces the old generation instead of hiding neutral pages.
    let gpu = world.resource::<ActorGpu>();
    assert!(gpu.artwork_current);
    assert_eq!(gpu.artwork_identity, [3; 32]);
    assert_eq!(gpu.artwork.pages.len(), 1);
    assert!(gpu.artwork.pages[0].bind_group.is_none());
    let now = std::time::Instant::now();
    assert!(!gate.publish_reserved(old, now, now));
    assert!(gate.drain().is_empty());

    let mut next = world
        .resource::<crate::actor::ActorRenderFrame>()
        .artwork
        .as_ref()
        .clone();
    next.entity_identity = [5; 32];
    world
        .resource_mut::<crate::actor::ActorRenderFrame>()
        .artwork = Arc::new(next);
    world
        .run_system_once(super::prepare_actor_resources)
        .unwrap();
    assert!(world.resource::<ActorGpu>().artwork_current);
}

fn app_with_noop_render_sub_app() -> App {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut render_app = SubApp::new();
    render_app
        .insert_resource(RenderDevice::from(device))
        .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))))
        .insert_resource(DrawFunctions::<Opaque3d>::default())
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
fn actor_shader_parses_as_wgsl() {
    let source = shader_source::standalone(ACTOR_SHADER_SOURCE, &[]);
    naga::front::wgsl::parse_str(&source).expect("actor shader parses");
}

// A binding the fragment stage reads must be visible to it, or pipeline creation fails validation.
#[test]
fn fragment_view_reads_are_visible_to_the_fragment_stage() {
    use bevy::render::render_resource::ShaderStages;
    assert!(crate::shader_test_support::fragment_reads_binding(
        &shader_source::standalone(ACTOR_SHADER_SOURCE, &[]),
        0,
        0
    ));
    assert!(
        actor_bind_group_layout().entries[0]
            .visibility
            .contains(ShaderStages::FRAGMENT)
    );
}

#[test]
fn plugin_install_is_idempotent_and_starts_one_shared_gpu_state() {
    let mut app = app_with_noop_render_sub_app();
    app.add_plugins(ActorRenderPlugin);
    app.finish();

    let render_app = app.sub_app_mut(RenderApp);
    assert!(
        render_app
            .world()
            .contains_resource::<ActorRenderInstalled>()
    );
    render_app.world_mut().run_schedule(RenderStartup);
    assert!(render_app.world().contains_resource::<ActorGpu>());
}

#[test]
fn pipeline_descriptor_specializes_and_noop_backend_accepts_the_binding_layout() {
    use bevy::prelude::Msaa;
    use bevy::render::{render_resource::Specializer, view::ViewTarget};

    let layout = actor_bind_group_layout();
    crate::shader_test_support::assert_binding_visibility(
        &shader_source::standalone(ACTOR_SHADER_SOURCE, &[]),
        0,
        &layout,
    );

    let mut descriptor = actor_pipeline_descriptor(layout.clone());
    ActorPipelineSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Sample4,
                hdr: true,
            },
            &mut descriptor,
        )
        .expect("actor pipeline specializes");
    assert_eq!(descriptor.multisample.count, 4);
    assert_eq!(
        descriptor.fragment.as_ref().unwrap().targets[0]
            .as_ref()
            .unwrap()
            .format,
        ViewTarget::TEXTURE_FORMAT_HDR
    );

    let app = app_with_noop_render_sub_app();
    let render_device = app.sub_app(RenderApp).world().resource::<RenderDevice>();
    render_device.create_bind_group_layout("actor layout validation", &layout.entries);
}

#[test]
fn rig_vertex_shader_stride_includes_both_uvs_without_changing_player_alpha() {
    assert_eq!(std::mem::size_of::<crate::actor::ActorRigVertex>(), 44);
    assert_eq!(
        std::mem::offset_of!(crate::actor::ActorRigVertex, bone_index),
        40
    );
    assert!(ACTOR_SHADER_SOURCE.contains(&format!(
        "instance_index * {}u",
        crate::actor::ACTOR_GPU_INSTANCE_WORDS
    )));
    assert!(ACTOR_SHADER_SOURCE.contains("(span.first_vertex + vertex_index) * 11u"));
    assert!(ACTOR_SHADER_SOURCE.contains("vertex_words[vertex_base + 10u]"));
    assert!(ACTOR_SHADER_SOURCE.contains("material_class.x == 0u && color.a < 0.1"));
    assert!(ACTOR_SHADER_SOURCE.contains("(input.light & 0x80000000u) != 0u"));
    // The one-sided plane sentinel lies below the shader's discard threshold.
    assert!(ACTOR_SHADER_SOURCE.contains("input.back_uv.x < -1.0e8"));
    const { assert!(crate::actor::ONE_SIDED_BACK_UV[0] < -1.0e8) };
    assert!(ACTOR_SHADER_SOURCE.contains("material_class.x == 1u && color.a == 0.0"));
}
