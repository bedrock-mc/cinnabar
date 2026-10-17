//! Motion faults must suppress real draw commands until compatible resources are ready.

use super::super::tests::allocation_fault_tests as actor;
use super::*;
use bevy::{
    math::{Mat4, UVec4},
    prelude::{Mut, With},
    render::texture::CachedTexture,
};
use std::{cell::Cell, rc::Rc, sync::Arc};

/// Builds the window's scene depth at the requested dimensions and sample count.
fn depth(device: &RenderDevice, width: u32, samples: u32) -> ViewDepthStencilTexture {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("motion allocation matrix depth"),
        size: Extent3d {
            width,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: TextureDimension::D2,
        format: CORE_3D_DEPTH_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    ViewDepthStencilTexture::new(
        CachedTexture {
            default_view: texture.create_view(&Default::default()),
            texture,
        },
        Some(0.0),
        None,
    )
}

/// Prepares and executes production motion commands, reporting whether they encoded a draw.
fn draw_motion(
    world: &mut World,
    state: &mut Option<MotionView>,
    depth: &ViewDepthStencilTexture,
) -> bool {
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let samples = depth.texture().sample_count();
    let msaa = if samples == 4 {
        Msaa::Sample4
    } else {
        Msaa::Off
    };
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        pipeline
            .prewarm(world.resource::<PipelineCache>(), msaa, false, false)
            .unwrap();
    });
    let pipeline = world.resource::<ActorPipeline>();
    let material = assets::EntityRenderMaterial::Default as u32;
    let ids = [
        pipeline.draw_variant(msaa, false, false, material).unwrap(),
        pipeline
            .motion_variant(ActorPipelineKey {
                msaa,
                hdr: false,
                enhanced: false,
                material,
            })
            .unwrap(),
    ];
    for id in ids {
        world
            .resource_mut::<PipelineCache>()
            .block_on_render_pipeline(id);
    }
    let camera = CameraHistory::new(Mat4::IDENTITY, Mat4::IDENTITY, UVec4::new(0, 0, 16, 16), 0);
    prepare(
        state,
        Some(world.resource::<ActorRenderFrame>()),
        Some(world.resource::<ActorGpu>()),
        Some(world.resource::<ActorPipeline>()),
        camera,
        camera,
        true,
        depth,
        false,
        false,
        &device,
        &queue,
        world.resource::<PipelineCache>(),
    );
    let Some(state) = state.as_ref() else {
        return false;
    };
    let view = world
        .query_filtered::<Entity, With<ExtractedView>>()
        .single(world)
        .unwrap();
    let (drawn, commands) = crate::render_test_support::record(world, &device, |world, context| {
        draw(world, state, view, depth, UVec4::new(0, 0, 16, 16), context)
    });
    queue.submit(commands);
    drawn
}

#[test]
fn gpu_fault_matrix_motion_first_replacement_resize_and_msaa_recover_draws() {
    for label in [
        "rendered actor velocities",
        "actor velocity view",
        "actor velocity history",
        "actor velocity upload",
        "actor velocity bindings",
    ] {
        for transition in ["first", "resize", "msaa", "growth"] {
            let mut app = actor::fixture();
            let frame = actor::publication(1, &[20], 1);
            assert_eq!(actor::draw_frame(&mut app, &frame).len(), 1);
            let world = app.sub_app_mut(RenderApp).world_mut();
            let device = world.resource::<RenderDevice>().clone();
            let initial = depth(&device, 16, 1);
            let mut state = None;
            assert!(
                draw_motion(world, &mut state, &initial),
                "motion positive control"
            );
            assert!(draw_motion(world, &mut state, &initial));
            let old_texture = state.as_ref().unwrap().texture.id();
            let target = match transition {
                "resize" => depth(&device, 32, 1),
                "msaa" => depth(&device, 16, 4),
                _ => depth(&device, 16, 1),
            };
            if transition == "first" {
                state = None;
            }
            if transition == "growth" {
                let mut posed = actor::publication(2, &[20], 1);
                posed.rig.geometry_revision = 1;
                posed.rig.previous_bones = Arc::from([[[1.0; 4]; 3]]);
                posed.rig.current_bones = Arc::clone(&posed.rig.previous_bones);
                assert_eq!(actor::draw_frame(&mut app, &posed).len(), 1);
                assert!(draw_motion(
                    app.sub_app_mut(RenderApp).world_mut(),
                    &mut state,
                    &initial
                ));
                // The next history uses the changed pose after committing instance growth.
                let mut next = actor::publication(3, &[60, 80], 1);
                next.rig.geometry_revision = 1;
                next.rig.previous_bones = Arc::clone(&posed.rig.previous_bones);
                next.rig.current_bones = Arc::clone(&posed.rig.current_bones);
                assert_eq!(actor::draw_frame(&mut app, &next).len(), 2);
            }
            // Texture/view faults require an allocation, while upload/binding faults use new data.
            if transition == "growth"
                && matches!(label, "rendered actor velocities" | "actor velocity view")
            {
                continue;
            }
            let world = app.sub_app_mut(RenderApp).world_mut();
            let hits = Rc::new(Cell::new(0));
            let observed = Rc::clone(&hits);
            let mut uploads = 0;
            {
                let _fault = gpu_allocation::inject(move |owner, _| {
                    if owner != label {
                        return false;
                    }
                    uploads += 1;
                    // Fail the bone upload after a successful instance replacement.
                    let fail =
                        transition != "growth" || label != "actor velocity upload" || uploads >= 2;
                    if fail {
                        observed.set(observed.get() + 1);
                    }
                    fail
                });
                assert!(
                    !draw_motion(world, &mut state, &target),
                    "{label}/{transition}"
                );
            }
            assert!(
                hits.get() > 0,
                "{label}/{transition}: injector must reach consumer"
            );
            if matches!(transition, "resize" | "msaa")
                && matches!(
                    label,
                    "rendered actor velocities" | "actor velocity view" | "actor velocity history"
                )
            {
                assert!(
                    state.is_none(),
                    "incompatible resources cannot survive replacement failure"
                );
            } else if transition == "growth" {
                assert_eq!(
                    state.as_ref().unwrap().texture.id(),
                    old_texture,
                    "compatible attachment is retained"
                );
            }
            assert!(
                draw_motion(world, &mut state, &target),
                "{label}/{transition}: next valid frame draws"
            );
            let recovered = state.as_ref().unwrap();
            assert_eq!(recovered.texture.size(), target.texture().size());
            assert_eq!(
                recovered.texture.sample_count(),
                target.texture().sample_count()
            );
            assert!(app.should_exit().is_none());
        }
    }
}
