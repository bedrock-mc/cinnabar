use super::*;
use crate::actor_render::tests::allocation_fault_tests::{draw_frame, fixture, publication};

mod viewport_raster;
use bevy::prelude::UVec4;

#[test]
fn mask_raster_viewport_uses_main_pass_resolution_and_camera_depth() {
    let camera = Viewport {
        physical_position: UVec2::new(12, 8),
        physical_size: UVec2::new(96, 64),
        depth: 0.25..0.75,
    };
    let resolution = MainPassResolutionOverride(UVec2::new(32, 24));
    let viewport = raster_viewport(Some(&camera), Some(&resolution), [128, 96]).unwrap();
    assert_eq!(viewport.physical_position, UVec2::new(12, 8));
    assert_eq!(viewport.physical_size, resolution.0);
    assert_eq!(viewport.depth, 0.25..0.75);

    let viewport = raster_viewport(None, Some(&resolution), [128, 96]).unwrap();
    assert_eq!(viewport.physical_position, UVec2::ZERO);
    assert_eq!(viewport.physical_size, resolution.0);
    assert_eq!(viewport.depth, 0.0..1.0);
}

#[test]
fn mask_raster_viewport_clips_to_attachments_and_handles_empty_views() {
    let viewport = raster_viewport(None, None, [128, 96]).unwrap();
    assert_eq!(viewport.physical_position, UVec2::ZERO);
    assert_eq!(viewport.physical_size, UVec2::new(128, 96));
    assert_eq!(viewport.depth, 0.0..1.0);

    let camera = Viewport {
        physical_position: UVec2::new(12, 8),
        physical_size: UVec2::new(96, 64),
        depth: 0.25..0.75,
    };
    let viewport = raster_viewport(Some(&camera), None, [64, 48]).unwrap();
    assert_eq!(viewport.physical_position, UVec2::new(12, 8));
    assert_eq!(viewport.physical_size, UVec2::new(52, 40));
    assert_eq!(viewport.depth, 0.25..0.75);
    assert!(raster_viewport(Some(&camera), None, [12, 8]).is_none());
    assert!(raster_viewport(Some(&camera), None, [0, 0]).is_none());
    let empty = MainPassResolutionOverride(UVec2::ZERO);
    assert!(raster_viewport(Some(&camera), Some(&empty), [128, 96]).is_none());
}

#[test]
fn mask_selection_keeps_the_dissolve_companion() {
    let mut app = fixture();
    let mut frame = publication(1, &[20, 20], 1);
    let instances = std::sync::Arc::make_mut(&mut frame.rig.instances);
    instances[0].material = assets::EntityRenderMaterial::DissolveDepth as u32;
    instances[1].material = assets::EntityRenderMaterial::DissolveColor as u32;
    let manifest = std::sync::Arc::make_mut(&mut frame.rig.manifest);
    manifest[1].identity = render_model::ActorRenderIdentity {
        layer: 1,
        ..manifest[0].identity
    };
    draw_frame(&mut app, &frame);
    let identity = frame.rig.manifest[0].identity;
    let mask = mod_render::masks::Mask {
        position: None,
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
        style: None,
        depth: MaskDepth::Always,
        entities: vec![MaskEntity {
            connection: identity.session_id,
            dimension: identity.dimension,
            runtime_id: identity.runtime_id,
            spawn_revision: identity.spawn_revision,
        }],
    };
    let spans = selection(&mask, app.sub_app(RenderApp).world().resource::<ActorGpu>()).unwrap();
    assert_eq!(
        spans.len(),
        2,
        "the color layer needs its rejecting depth companion"
    );
}

#[test]
fn mask_dissolve_prepass_writes_only_private_depth() {
    for msaa in [Msaa::Off, Msaa::Sample4] {
        for depth in [MaskDepth::Test, MaskDepth::Always] {
            let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
            MaskSpecializer
                .specialize(
                    Key {
                        actor: ActorPipelineKey {
                            msaa,
                            hdr: false,
                            enhanced: false,
                            material: assets::EntityRenderMaterial::DissolveDepth as u32,
                        },
                        depth,
                    },
                    &mut descriptor,
                )
                .unwrap();
            assert_eq!(
                descriptor.depth_stencil.unwrap().depth_write_enabled,
                Some(true)
            );
            assert!(descriptor.fragment.unwrap().targets.is_empty());
        }
    }
}

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
        position: None,
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
        style: None,
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
    assert!(gpu.artwork_current && gpu.artwork.page(0).unwrap().bind_group.is_some());
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
    for defines in [
        vec![],
        vec!["MASK_BLEND"],
        vec!["ALPHA_TO_COVERAGE"],
        vec!["MASK_DISSOLVE"],
        vec!["MASK_DISSOLVE", "MASK_DISSOLVE_MSAA"],
    ] {
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
fn actor_mask_pixels_preserve_dissolve_rejection() {
    use crate::{
        actor_raster,
        gpu_snapshot::{self, Gpu, RasterState, SNAPSHOT_SIDE},
    };
    let Some(gpu) = Gpu::for_fixture_with_limits(
        "actor mask dissolve rejection",
        wgpu::Features::empty(),
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: crate::required_vertex_storage_buffers(),
            ..Default::default()
        },
    ) else {
        return;
    };
    let vertices = actor_raster::cube([16; 3], true, false);
    let middle = ((SNAPSHOT_SIDE / 2 * SNAPSHOT_SIDE + SNAPSHOT_SIDE / 2) * 4) as usize;
    let mut failures = Vec::new();
    for samples in [1, 4] {
        for depth in [MaskDepth::Test, MaskDepth::Always] {
            for (alpha, multiplier, retained) in [
                (255, 0.0, false),
                (255, 0.49, false),
                (255, 0.5, true),
                (128, 1.0, true),
                (127, 2.0, false),
            ] {
                let pixels = actor_raster::raster_material_with_state(
                    &gpu,
                    &vertices,
                    render_model::ActorMaterial {
                        kind: assets::EntityRenderMaterial::DissolveColor,
                        // The companion, rather than this independent parameter, owns rejection.
                        dissolve_multiplier: 0.0,
                        ..Default::default()
                    },
                    false,
                    [[255, 255, 255, alpha]; 2],
                    false,
                    false,
                    render_model::pack_actor_light(15, 15),
                    0,
                    0,
                    &[],
                    RasterState {
                        actor_fragment: Some("actor_mask_fragment"),
                        actor_companion: Some(render_model::ActorMaterial {
                            kind: assets::EntityRenderMaterial::DissolveDepth,
                            dissolve_multiplier: multiplier,
                            ..Default::default()
                        }),
                        depth_compare: if depth == MaskDepth::Test {
                            wgpu::CompareFunction::GreaterEqual
                        } else {
                            wgpu::CompareFunction::Always
                        },
                        multisample: wgpu::MultisampleState {
                            count: samples,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                );
                if alpha == 255 && multiplier == 0.49 {
                    gpu_snapshot::save(&format!("mask-dissolve-{depth:?}-{samples}"), &pixels);
                }
                let expected = if retained {
                    255
                } else {
                    (gpu_snapshot::CLEAR_COLOR.r * 255.0).round() as u8
                };
                if pixels[middle].abs_diff(expected) > 1 {
                    failures.push(format!("{depth:?}, {samples} samples, alpha {alpha}, multiplier {multiplier}: {:?}", &pixels[middle..middle + 4]));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn mask_attachments_reject_stale_resolution_and_sample_counts() {
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let original = test_depth(&device, 64, 1);
    let mask = MaskTarget::new(
        &device,
        &PipelineCache::new(device.clone(), true),
        &original,
    )
    .unwrap();
    assert!(mask.matches(&original));
    assert!(!mask.matches(&test_depth(&device, 127, 1)));
    assert!(!mask.matches(&test_depth(&device, 64, 4)));
    let multisampled = MaskTarget::new(
        &device,
        &PipelineCache::new(device.clone(), true),
        &test_depth(&device, 127, 4),
    )
    .unwrap();
    assert!(multisampled.matches(&test_depth(&device, 127, 4)));
    assert_eq!(multisampled.view.texture().sample_count(), 1);
    assert_eq!(multisampled.render_view.texture().sample_count(), 4);
}

/// Allocates scene depth on the fixture device for mask allocation and resizing tests.
fn test_depth(device: &RenderDevice, side: u32, samples: u32) -> ViewDepthStencilTexture {
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
        bevy::render::texture::CachedTexture {
            default_view: texture.create_view(&default()),
            texture,
        },
        Some(0.0),
        None,
    )
}

#[test]
fn masks_split_shared_coverage_after_membership_changes() {
    let mut app = fixture();
    let frame = publication(1, &[20, 30], 1);
    let entities: Vec<_> = frame
        .rig
        .manifest
        .iter()
        .map(|entry| MaskEntity {
            connection: entry.identity.session_id,
            dimension: entry.identity.dimension,
            runtime_id: entry.identity.runtime_id,
            spawn_revision: entry.identity.spawn_revision,
        })
        .collect();
    let mut first = mod_render::masks::Mask {
        position: None,
        handle: DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        },
        session: mod_api::drawing::DrawingSession {
            connection: entities[0].connection,
            dimension: entities[0].dimension,
            local: true,
            permissions: mod_api::drawing::DrawingPermissions::ALL,
        },
        depth: MaskDepth::Test,
        entities: vec![entities[0]],
        style: None,
    };
    let mut second = first.clone();
    second.handle.owner = 2;
    let world = app.sub_app_mut(RenderApp).world_mut();
    let view = world
        .query_filtered::<Entity, bevy::prelude::With<ExtractedView>>()
        .single(world)
        .unwrap();
    let depth = test_depth(world.resource::<RenderDevice>(), 64, 1);
    world.entity_mut(view).insert(depth);
    world.init_resource::<render_scene::ModRenderScene>();
    world.resource_mut::<render_scene::ModRenderScene>().masks =
        vec![first.clone(), second.clone()].into();
    draw_frame(&mut app, &frame);
    let masks = app.sub_app(RenderApp).world().resource::<MaskGpu>();
    assert_eq!(
        masks.targets[&(view, first.handle)].view.id(),
        masks.targets[&(view, second.handle)].view.id()
    );
    assert_eq!(
        masks.attachment_bytes,
        mod_render::masks::attachment_bytes(64, 64, 1).unwrap()
    );

    second.entities = vec![entities[1]];
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<render_scene::ModRenderScene>()
        .masks = vec![first.clone(), second.clone()].into();
    draw_frame(&mut app, &frame);
    let masks = app.sub_app(RenderApp).world().resource::<MaskGpu>();
    assert_ne!(
        masks.targets[&(view, first.handle)].view.id(),
        masks.targets[&(view, second.handle)].view.id()
    );
    assert_eq!(
        masks.attachment_bytes,
        mod_render::masks::attachment_bytes(64, 64, 1).unwrap() * 2
    );

    first.entities = second.entities.clone();
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<render_scene::ModRenderScene>()
        .masks = vec![first.clone(), second.clone()].into();
    draw_frame(&mut app, &frame);
    let masks = app.sub_app(RenderApp).world().resource::<MaskGpu>();
    assert_eq!(
        masks.targets[&(view, first.handle)].view.id(),
        masks.targets[&(view, second.handle)].view.id()
    );
    assert_eq!(
        masks.attachment_bytes,
        mod_render::masks::attachment_bytes(64, 64, 1).unwrap()
    );
}

#[test]
fn attachment_pressure_preserves_fitting_groups_and_shares_owner_turns() {
    let masks: Vec<_> = [1, 1, 1, 1, 1, 1, 2, 3]
        .into_iter()
        .enumerate()
        .map(|(i, owner)| mod_render::masks::Mask {
            position: None,
            handle: DrawingHandle {
                owner,
                generation: owner,
                slot: i as u64 + 1,
            },
            session: mod_api::drawing::DrawingSession::default(),
            depth: MaskDepth::Always,
            entities: Vec::new(),
            style: None,
        })
        .collect();
    for (width, height, expected) in [(1920, 1080, 6), (2560, 1440, 3), (3840, 2160, 1)] {
        let mut groups: Vec<_> = masks.iter().collect();
        let bytes = mod_render::masks::attachment_bytes(width, height, 2);
        let charged = admit_groups(&mut groups, bytes);
        assert_eq!(groups.len(), expected);
        assert!(charged <= MAX_MASK_ATTACHMENT_BYTES);
        if expected >= 3 {
            assert_eq!(
                groups
                    .iter()
                    .take(3)
                    .map(|mask| mask.handle.owner)
                    .collect::<Vec<_>>(),
                [1, 2, 3]
            );
        }
    }
    let mut groups: Vec<_> = masks.iter().collect();
    assert_eq!(admit_groups(&mut groups, None), 0);
    assert!(groups.is_empty());
}

#[test]
fn hidden_local_body_is_available_to_translated_masks_without_main_pass_draws() {
    let mut app = fixture();
    let mut frame = publication(1, &[20], 1);
    let identity = frame.rig.manifest[0].identity;
    frame.mask_only_runtime_ids = std::sync::Arc::from([identity.runtime_id]);
    draw_frame(&mut app, &frame);
    let gpu = app.sub_app(RenderApp).world().resource::<ActorGpu>();
    assert!(gpu.spans.is_empty());
    assert!(!gpu.mask_spans.is_empty());
    let mask = mod_render::masks::Mask {
        handle: DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        },
        position: Some([5., 64., 3.]),
        style: None,
        depth: MaskDepth::Always,
        session: mod_api::drawing::DrawingSession {
            connection: identity.session_id,
            dimension: identity.dimension,
            local: true,
            permissions: mod_api::drawing::DrawingPermissions::ALL,
        },
        entities: vec![MaskEntity {
            connection: identity.session_id,
            dimension: identity.dimension,
            runtime_id: identity.runtime_id,
            spawn_revision: identity.spawn_revision,
        }],
    };
    assert!(!selection(&mask, gpu).unwrap().is_empty());
    let root = &gpu.instances[0].world_from_actor;
    assert_eq!(
        displacement(&mask, gpu),
        [5. - root[0][3], 64. - root[1][3], 3. - root[2][3], 0.]
    );
    let ordinary = mod_render::masks::Mask {
        position: None,
        ..mask.clone()
    };
    assert!(!compatible(&mask, &ordinary));
}

#[test]
fn an_independent_layer_stays_visible_beside_its_coverage_only_owner() {
    let mut app = fixture();
    let mut frame = publication(1, &[20, 20], 1);
    let runtime_id = frame.rig.manifest[0].identity.runtime_id;
    let manifest = std::sync::Arc::make_mut(&mut frame.rig.manifest);
    manifest[1].identity.runtime_id = runtime_id;
    manifest[0].identity.layer = render_model::hats::ACTOR_LAYER_HAT;
    frame.mask_only_runtime_ids = std::sync::Arc::from([runtime_id]);
    frame.main_pass_instance_count = Some(1);
    draw_frame(&mut app, &frame);
    let gpu = app.sub_app(RenderApp).world().resource::<ActorGpu>();
    assert_eq!(gpu.spans.iter().map(|span| span.count).sum::<u32>(), 1);
    assert_eq!(gpu.mask_spans.iter().map(|span| span.count).sum::<u32>(), 2);
    assert_eq!(gpu.main_manifest.len(), 1);
}
