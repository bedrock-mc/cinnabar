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
    player_skins_resident,
};

#[path = "pipeline_prewarm_tests.rs"]
mod pipeline_prewarm_tests;

#[path = "pipeline_material_gpu_tests.rs"]
mod pipeline_material_gpu_tests;

#[path = "frame_order_tests.rs"]
mod frame_order_tests;

/// A residency holding one standard-raster skin in class 0, layer 0.
fn one_resident_skin() -> Arc<crate::actor::ActorSkinResidency> {
    let skin = render_api::SkinRgba8::from(vec![255; render_model::STANDARD_SKIN_BYTES]);
    let mut residency = crate::actor::ActorSkinResidency::default();
    residency.classes[0] = Arc::from([Some(crate::actor::ResidentSkin {
        texels: Arc::clone(skin.pixels()),
        skin,
        admission: 1,
    })]);
    Arc::new(residency)
}

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
    frame.skins = one_resident_skin();

    assert!(player_skins_resident(&frame));
}

#[test]
fn dragon_dissolve_depth_and_color_passes_keep_their_distinct_depth_contracts() {
    use bevy::prelude::Msaa;
    use bevy::render::render_resource::{ColorWrites, CompareFunction, Specializer};
    for material in [
        assets::EntityRenderMaterial::DissolveDepth,
        assets::EntityRenderMaterial::DissolveColor,
    ] {
        let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
        ActorPipelineSpecializer
            .specialize(
                ActorPipelineKey {
                    msaa: Msaa::Off,
                    hdr: false,
                    enhanced: false,
                    material: material as u32,
                },
                &mut descriptor,
            )
            .unwrap();
        let depth = descriptor.depth_stencil.unwrap();
        let target = descriptor.fragment.unwrap().targets[0].clone().unwrap();
        assert_eq!(depth.depth_write_enabled, Some(true));
        assert_eq!(target.blend, None);
        if material == assets::EntityRenderMaterial::DissolveDepth {
            assert_eq!(target.write_mask, ColorWrites::empty());
            assert_eq!(depth.depth_compare, Some(CompareFunction::GreaterEqual));
        } else {
            assert_eq!(target.write_mask, ColorWrites::ALL);
            assert_eq!(depth.depth_compare, Some(CompareFunction::Equal));
        }
    }
}

#[test]
fn actor_material_states_specialize_culling_blending_and_depth_write_independently() {
    use bevy::prelude::Msaa;
    use bevy::render::render_resource::{BlendFactor, Face, Specializer};
    for (cull, blend, depth_write) in [
        (true, false, true),
        (false, true, true),
        (true, true, false),
    ] {
        let material = crate::ActorMaterial {
            state: Some(assets::EntityRenderMaterialState {
                alpha_test: true,
                cull,
                blend,
                depth_write,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
        ActorPipelineSpecializer
            .specialize(
                ActorPipelineKey {
                    msaa: Msaa::Off,
                    hdr: false,
                    enhanced: false,
                    material: material.gpu_word(),
                },
                &mut descriptor,
            )
            .unwrap();
        assert_eq!(descriptor.primitive.cull_mode, cull.then_some(Face::Back));
        assert_eq!(
            descriptor.depth_stencil.unwrap().depth_write_enabled,
            Some(depth_write)
        );
        let actual = descriptor.fragment.unwrap().targets[0]
            .as_ref()
            .unwrap()
            .blend;
        assert_eq!(actual.is_some(), blend);
        if let Some(actual) = actual {
            assert_eq!(actual.color.src_factor, BlendFactor::SrcAlpha);
            assert_eq!(actual.color.dst_factor, BlendFactor::OneMinusSrcAlpha);
        }
    }
}

#[test]
fn skin_preparation_rejects_slots_that_are_not_resident() {
    let mut frame = crate::actor::ActorRenderFrame::default();
    frame.rig.instances = Arc::from([crate::actor::ActorGpuInstance {
        texture_layer: 0,
        ..Default::default()
    }]);
    assert!(!player_skins_resident(&frame));

    frame.skins = one_resident_skin();
    assert!(player_skins_resident(&frame));
    for slot in [1, crate::actor::pack_skin_slot(1, 0)] {
        Arc::make_mut(&mut frame.rig.instances)[0].texture_layer = slot;
        assert!(!player_skins_resident(&frame));
    }
}

#[test]
fn generic_only_frames_do_not_require_or_reinterpret_player_skins() {
    let mut frame = crate::actor::ActorRenderFrame::default();
    frame.rig.instances = Arc::from([crate::actor::ActorGpuInstance {
        texture_layer: 17,
        ..Default::default()
    }]);
    frame.instance_pages = Arc::from([1]);
    assert!(
        player_skins_resident(&frame),
        "generic layer is not a player-skin layer"
    );
    frame.instance_pages = Arc::from([0]);
    assert!(!player_skins_resident(&frame));
}

#[test]
fn first_generic_only_frame_prepares_after_an_empty_skin_revision() {
    use crate::actor::{ActorDrawManifestEntry, ActorRenderIdentity, ActorRigRoute};
    use bevy::ecs::system::RunSystemOnce;
    use render_model::{ActorRigVertex, EntityRigId};
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
            surface: Default::default(),
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
        color_mask: false,
        multitexture: false,
        rgba8: vec![255; 1024].into(),
    }]);
    frame.artwork = Arc::new(artwork);
    world.insert_resource(frame);
    world
        .run_system_once(super::prepare_actor_resources)
        .unwrap();
    let gpu = world.resource::<ActorGpu>();
    assert_eq!(gpu.instance_count, 1);
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

fn standalone_actor_shader_source() -> String {
    let shader = crate::shader_safety::from_actor_wgsl(
        ACTOR_SHADER_SOURCE,
        "actor.wgsl",
        crate::actor::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wgsl(source) = shader.source else {
        panic!("actor source is WGSL");
    };
    shader_source::standalone(&source, &[])
}

#[test]
fn actor_shader_parses_as_wgsl() {
    let module = naga::front::wgsl::parse_str(&standalone_actor_shader_source())
        .expect("actor shader parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("shared fog and native multitexture varyings validate together");
}

// A binding the fragment stage reads must be visible to it, or pipeline creation fails validation.
#[test]
fn fragment_view_reads_are_visible_to_the_fragment_stage() {
    use bevy::render::render_resource::ShaderStages;
    assert!(crate::shader_test_support::fragment_reads_binding(
        &standalone_actor_shader_source(),
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
    use bevy::render::render_resource::Specializer;

    let layout = actor_bind_group_layout();
    crate::shader_test_support::assert_binding_visibility(
        &standalone_actor_shader_source(),
        0,
        &layout,
    );

    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        let mut descriptor = actor_pipeline_descriptor(layout.clone());
        ActorPipelineSpecializer
            .specialize(
                ActorPipelineKey {
                    material: 0,
                    msaa,
                    hdr: true,
                    enhanced: false,
                },
                &mut descriptor,
            )
            .expect("actor pipeline specializes");
        assert_eq!(descriptor.multisample.count, msaa.samples());
        assert_eq!(
            descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap()
                .format,
            crate::SCENE_HDR_FORMAT
        );
    }

    let app = app_with_noop_render_sub_app();
    let render_device = app.sub_app(RenderApp).world().resource::<RenderDevice>();
    render_device.create_bind_group_layout("actor layout validation", &layout.entries);
}

#[test]
fn rig_vertex_shader_stride_includes_both_uvs_without_changing_player_alpha() {
    assert_eq!(
        std::mem::size_of::<render_model::ActorRigVertex>(),
        render_model::ACTOR_RIG_VERTEX_WORDS * 4
    );
    assert_eq!(
        std::mem::offset_of!(render_model::ActorRigVertex, bone_index),
        40
    );
    assert!(ACTOR_SHADER_SOURCE.contains("instance_index * ACTOR_GPU_INSTANCE_WORDS"));
    assert!(ACTOR_SHADER_SOURCE.contains("vertex_words[vertex_base + 10u]"));
    assert!(ACTOR_SHADER_SOURCE.contains("material_class.x == 0u && color.a < 0.1"));
    // The one-sided plane sentinel lies below the shader's discard threshold.
    assert!(ACTOR_SHADER_SOURCE.contains("input.back_uv.x < -1.0e8"));
    const { assert!(render_model::ONE_SIDED_BACK_UV[0] < -1.0e8) };
    assert!(ACTOR_SHADER_SOURCE.contains("material_class.x == 1u && color.a == 0.0"));
}

#[test]
fn native_color_mask_alpha_controls_dye_not_opacity() {
    assert!(ACTOR_SHADER_SOURCE.contains("let color_mask_material = material_class.y != 0u;"));
    assert!(ACTOR_SHADER_SOURCE.contains("!color_mask_material && !multitexture_material &&"));
    assert!(ACTOR_SHADER_SOURCE.contains("mix(color.rgb, color.rgb * dye, color.a)"));
    assert!(ACTOR_SHADER_SOURCE.contains("color.a * change_color.a"));
    let descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    assert!(
        descriptor.fragment.unwrap().targets[0]
            .as_ref()
            .unwrap()
            .blend
            .is_none()
    );
    assert_eq!(
        descriptor.depth_stencil.unwrap().depth_write_enabled,
        Some(true)
    );
}

#[test]
fn native_multitexture_mixes_rgb_once_without_using_base_alpha_as_coverage() {
    assert!(
        ACTOR_SHADER_SOURCE.contains("material_class.z != 0u && all(input.multitexture_layers")
    );
    assert!(
        ACTOR_SHADER_SOURCE.contains("mix(mix(color.rgb, tex1.rgb, tex1.a), tex2.rgb, tex2.a)")
    );
    assert!(ACTOR_SHADER_SOURCE.contains("!color_mask_material && !multitexture_material &&"));
    assert_eq!(
        std::mem::offset_of!(crate::actor::ActorGpuInstance, multitexture_layers) / 4,
        std::mem::offset_of!(crate::actor::ActorGpuInstance, material) / 4 - 2
    );
}

fn diagnostic_body(runtime_id: u64, texture_layer: u32) -> crate::actor::ActorRigSubmission {
    use crate::actor::{ActorRenderIdentity, ActorRigRenderInput, ActorRigRoute};
    let bone = render_model::RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [0.0, 0.0, 0.0, 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    crate::actor::ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: ActorRigRenderInput {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id,
                spawn_revision: 1,
                ingress_sequence: 1,
                source_tick: Some(1),
                movement_revision: 1,
                pose_generation: 1,
                layer: 0,
            },
            rig: render_model::EntityRigId(u32::MAX),
            previous_bones: Arc::from([bone; 6]),
            current_bones: Arc::from([bone; 6]),
            completed_tick: 1,
            reset_generation: 1,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 64.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        texture_layer,
        route: ActorRigRoute::Diagnostic,
        tint: 0,
        uv_anim: crate::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    }
}

/// A standard raster upscaled from a `side`-texel skin of one varying colour per texel.
fn upscaled_skin(seed: u8, side: usize) -> render_api::SkinRgba8 {
    let standard = render_model::STANDARD_SKIN_SIDE;
    let scale = standard / side;
    let mut out = vec![0; render_model::STANDARD_SKIN_BYTES];
    for (index, texel) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (index % standard / scale, index / standard / scale);
        texel.copy_from_slice(&[seed, x as u8, y as u8, 255]);
    }
    out.into()
}

/// Publishes the players in `shown` (runtime id, skin) and syncs the GPU arrays to that frame.
fn publish_and_sync(
    scene: &mut crate::actor::ActorRenderScene,
    gpu: &mut super::GpuSkinArrays,
    device: &RenderDevice,
    queue: &RenderQueue,
    shown: &[(u64, &render_api::SkinRgba8)],
) -> Vec<u32> {
    let skins: Vec<_> = shown.iter().map(|(_, skin)| (*skin).clone()).collect();
    let frame = scene.update_rigs(
        0.5,
        None,
        shown
            .iter()
            .enumerate()
            .map(|(index, (runtime_id, _))| diagnostic_body(*runtime_id, index as u32)),
        &skins,
    );
    assert!(player_skins_resident(frame));
    gpu.sync(&frame.skins, device, queue);
    assert!(gpu.is_synced(&frame.skins));
    frame
        .rig
        .instances
        .iter()
        .map(|instance| instance.texture_layer)
        .collect()
}

#[test]
fn skin_arrays_upload_only_newly_admitted_skins() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let mut gpu = super::GpuSkinArrays::new(&device);
    let mut scene = crate::actor::ActorRenderScene::default();
    let skins: Vec<_> = (0..6).map(|seed| upscaled_skin(seed, 64)).collect();
    let classic = 64 * 64 * 4;

    let first: Vec<_> = (0..4)
        .map(|index| (index + 1, &skins[index as usize]))
        .collect();
    let layers = publish_and_sync(&mut scene, &mut gpu, &device, &queue, &first);
    assert_eq!(gpu.uploaded_bytes, 4 * classic);

    // Players leaving and re-entering view sample their existing layers.
    let subset = [first[3], first[1]];
    let mut moved = publish_and_sync(&mut scene, &mut gpu, &device, &queue, &subset);
    moved.sort_unstable();
    let mut expected = [layers[1], layers[3]];
    expected.sort_unstable();
    assert_eq!(moved, expected);
    publish_and_sync(&mut scene, &mut gpu, &device, &queue, &first);
    assert_eq!(gpu.uploaded_bytes, 4 * classic);

    // HD skins upload only their native layer; growing the array copies resident layers on
    // the GPU instead of re-uploading them.
    let hd = [upscaled_skin(9, 256), upscaled_skin(10, 256)];
    let mut shown = first.clone();
    shown.push((5, &hd[0]));
    let one = publish_and_sync(&mut scene, &mut gpu, &device, &queue, &shown);
    assert_eq!(gpu.uploaded_bytes, 4 * classic + 256 * 256 * 4);
    shown.push((6, &hd[1]));
    let two = publish_and_sync(&mut scene, &mut gpu, &device, &queue, &shown);
    assert_eq!(scene.frame().skins.classes[3].len(), 2);
    assert_eq!(two[..5], one[..]);
    assert_eq!(gpu.uploaded_bytes, 4 * classic + 2 * 256 * 256 * 4);
}
