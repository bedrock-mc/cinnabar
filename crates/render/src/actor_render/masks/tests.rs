use super::*;
use crate::actor_render::tests::allocation_fault_tests::{draw_frame, fixture, publication};

#[test]
fn actor_only_renderer_draws_without_a_mod_scene() {
    let mut app = fixture();
    assert!(
        !app.sub_app(RenderApp)
            .world()
            .contains_resource::<render_scene::ModRenderScene>()
    );
    let frame = publication(1, &[20], 1);
    assert_eq!(draw_frame(&mut app, &frame).len(), 1);
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<MaskGpu>()
            .targets
            .is_empty()
    );
}

#[test]
fn mask_selection_suppresses_failed_actor_uploads_and_recovers() {
    let mut app = fixture();
    app.sub_app_mut(RenderApp)
        .world_mut()
        .init_resource::<render_scene::ModRenderScene>();
    let control = publication(1, &[20], 1);
    assert_eq!(draw_frame(&mut app, &control).len(), 1);
    let identity = &control.rig.manifest[0].identity;
    let mut mask = mod_render::masks::Mask {
        handle: DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        },
        session: mod_api::drawing::DrawingSession {
            connection: identity.session_id,
            dimension: identity.dimension,
            local: true,
            permissions: mod_api::drawing::DrawingPermissions::ALL,
        },
        depth: MaskDepth::Test,
        entities: vec![MaskEntity {
            connection: identity.session_id,
            dimension: identity.dimension,
            runtime_id: identity.runtime_id,
            spawn_revision: identity.spawn_revision,
        }],
    };
    assert!(mask.authorized());
    assert_eq!(
        selection(&mask, app.sub_app(RenderApp).world().resource::<ActorGpu>())
            .unwrap()
            .len(),
        1
    );

    let mut next = control;
    next.rig.frame_generation += 1;
    let fault = gpu_allocation::inject(|owner, _| owner == "actor instance and bone upload");
    assert!(draw_frame(&mut app, &next).is_empty());
    let gpu = app.sub_app(RenderApp).world().resource::<ActorGpu>();
    assert_eq!(gpu.instance_count, 0);
    assert!(!gpu.spans.is_empty() && !gpu.manifest.is_empty());
    assert!(gpu.artwork_current && gpu.artwork.pages[0].bind_group.is_some());
    for depth in [MaskDepth::Test, MaskDepth::Always] {
        mask.depth = depth;
        assert!(selection(&mask, gpu).is_none(), "{depth:?}");
    }

    drop(fault);
    next.rig.frame_generation += 1;
    assert_eq!(draw_frame(&mut app, &next).len(), 1);
    assert_eq!(
        selection(&mask, app.sub_app(RenderApp).world().resource::<ActorGpu>())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn mask_pipeline_cannot_inherit_an_actors_depth_bypass_or_write_world_depth() {
    for msaa in [Msaa::Off, Msaa::Sample4] {
        for depth in [MaskDepth::Test, MaskDepth::Always] {
            let material = assets::EntityRenderMaterial::Default.word(Some(
                assets::EntityRenderMaterialState {
                    alpha_test: true,
                    cull: true,
                    depth_always: true,
                    depth_write: true,
                    ..Default::default()
                },
            ));
            let key = Key {
                actor: ActorPipelineKey {
                    msaa,
                    hdr: false,
                    enhanced: false,
                    material,
                },
                depth,
            };
            let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
            MaskSpecializer.specialize(key, &mut descriptor).unwrap();
            let state = descriptor.depth_stencil.unwrap();
            assert_eq!(state.depth_write_enabled, Some(false));
            assert_eq!(
                state.depth_compare,
                Some(if depth == MaskDepth::Test {
                    CompareFunction::GreaterEqual
                } else {
                    CompareFunction::Always
                })
            );
            assert_eq!(descriptor.multisample.count, msaa.samples());
            assert_eq!(
                descriptor.multisample.alpha_to_coverage_enabled,
                msaa.samples() > 1
            );
            assert_eq!(descriptor.primitive.cull_mode, Some(wgpu::Face::Back));
            assert_eq!(
                descriptor.fragment.unwrap().targets[0]
                    .as_ref()
                    .unwrap()
                    .format,
                FORMAT
            );
        }
    }
}

#[test]
fn production_mask_shaders_validate_for_opaque_blended_and_msaa_cutout_coverage() {
    let source = crate::material_shader::source(include_str!("../../actor.wesl"))
        .replace(
            "ACTOR_GPU_INSTANCE_WORDS",
            &render_model::ACTOR_GPU_INSTANCE_WORDS.to_string(),
        )
        .replace(
            "ACTOR_RIG_VERTEX_WORDS",
            &render_model::ACTOR_RIG_VERTEX_WORDS.to_string(),
        );
    for defines in [vec![], vec!["MASK_BLEND"], vec!["ALPHA_TO_COVERAGE"]] {
        let source = crate::shader_source::standalone_prepared(&source, &defines);
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[test]
fn actor_mask_pixels_preserve_cutouts_and_blended_coverage() {
    use crate::{
        actor_raster,
        gpu_snapshot::{Gpu, RasterState, SNAPSHOT_SIDE},
    };
    let Some(gpu) = Gpu::for_fixture_with_limits(
        "actor mask coverage",
        wgpu::Features::empty(),
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: crate::required_vertex_storage_buffers(),
            ..Default::default()
        },
    ) else {
        return;
    };
    let vertices = actor_raster::cube([16, 16, 16], true, false);
    for samples in [1, 4] {
        let cutout = samples > 1;
        let pixels = actor_raster::raster_material_with_state(
            &gpu,
            &vertices,
            render_model::ActorMaterial {
                kind: assets::EntityRenderMaterial::Default,
                state: Some(assets::EntityRenderMaterialState {
                    alpha_test: true,
                    depth_write: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            false,
            [[255, 255, 255, 0]; 2],
            false,
            false,
            render_model::pack_actor_light(15, 15),
            0,
            0,
            if cutout { &["ALPHA_TO_COVERAGE"] } else { &[] },
            RasterState {
                actor_fragment: Some("actor_mask_fragment"),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    alpha_to_coverage_enabled: cutout,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert!(
            !pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] == 255),
            "transparent texels must not become silhouette pixels"
        );
    }
    let pixels = actor_raster::raster_material_with_state(
        &gpu,
        &vertices,
        render_model::ActorMaterial {
            kind: assets::EntityRenderMaterial::Default,
            state: Some(assets::EntityRenderMaterialState {
                blend: true,
                ..Default::default()
            }),
            ..Default::default()
        },
        false,
        [[255, 255, 255, 128]; 2],
        false,
        false,
        render_model::pack_actor_light(15, 15),
        0,
        0,
        &["MASK_BLEND"],
        RasterState {
            actor_fragment: Some("actor_mask_fragment"),
            ..Default::default()
        },
    );
    let middle = ((SNAPSHOT_SIDE / 2 * SNAPSHOT_SIDE + SNAPSHOT_SIDE / 2) * 4) as usize;
    assert!(
        pixels[middle].abs_diff(128) <= 1,
        "blended coverage must remain fractional: {:?}",
        &pixels[middle..middle + 4]
    );
}

#[test]
fn mask_attachments_reject_stale_resolution_and_sample_counts() {
    use bevy::render::texture::CachedTexture;
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let depth = |side, samples| {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("mask resize witness"),
            size: Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples,
            dimension: TextureDimension::D2,
            format: CORE_3D_DEPTH_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        ViewDepthStencilTexture::new(
            CachedTexture {
                default_view: texture.create_view(&default()),
                texture,
            },
            Some(0.0),
            None,
        )
    };
    let original = depth(64, 1);
    let mask = MaskTarget::new(&device, &original).unwrap();
    assert!(mask.matches(&original));
    assert!(!mask.matches(&depth(127, 1)));
    assert!(!mask.matches(&depth(64, 4)));
    let multisampled = MaskTarget::new(&device, &depth(127, 4)).unwrap();
    assert!(multisampled.matches(&depth(127, 4)));
    assert_eq!(multisampled.view.texture().sample_count(), 1);
    assert_eq!(multisampled.render_view.texture().sample_count(), 4);
}
