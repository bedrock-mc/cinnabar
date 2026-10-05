use super::*;
use crate::{ActorGpuInstance, ActorRigGeometrySpan};
use render_model::ActorRigVertex;

fn single_instance_frame() -> ActorRigRenderFrame {
    ActorRigRenderFrame {
        frame_generation: 1,
        geometry_revision: 1,
        instances: Arc::from([ActorGpuInstance::default()]),
        previous_bones: Arc::from([[[0.0; 4]; 3]]),
        current_bones: Arc::from([[[0.0; 4]; 3]]),
        geometry_vertices: crate::actor::ActorRigVertexSegments::from_vertices([
            ActorRigVertex::default(),
        ]),
        geometry_spans: Arc::from([ActorRigGeometrySpan {
            first_vertex: 0,
            vertex_count: 1,
        }]),
        manifest: Arc::from([]),
        maximum_vertex_count: 36,
        rejects: crate::ActorRigRejects::default(),
    }
}

fn skin() -> render_api::SkinRgba8 {
    vec![0u8; render_model::STANDARD_SKIN_BYTES].into()
}

fn light() -> HandRigLight {
    HandRigLight {
        block_level: 15,
        sky_level: 0,
        daylight: 1.0,
        pad: 0,
    }
}

#[test]
fn first_person_target_blends_translucent_block_texels_over_the_scene() {
    let pipeline = pipeline_descriptor(hand_rig_layout());
    let target = pipeline.fragment.unwrap().targets.remove(0).unwrap();
    assert_eq!(target.blend, Some(BlendState::ALPHA_BLENDING));
}

#[test]
fn held_alpha_selectors_preserve_each_hands_artwork_layer() {
    for alpha_mode in [
        HandItemAlphaMode::Opaque,
        HandItemAlphaMode::Cutout,
        HandItemAlphaMode::Blend,
    ] {
        for hand in [0, HAND_OFFHAND_LAYER_FLAG] {
            let layer = 17 | HAND_ITEM_LAYER_FLAG | hand | alpha_mode.texture_layer_flag();
            assert_eq!(layer & HAND_TEXTURE_LAYER_MASK, 17);
            assert_eq!(layer & HAND_OFFHAND_LAYER_FLAG, hand);
            assert_eq!(
                layer & (HAND_BLEND_LAYER_FLAG | HAND_CUTOUT_LAYER_FLAG),
                alpha_mode.texture_layer_flag(),
            );
        }
    }
}

#[test]
fn publish_accepts_a_single_instance_lit_rig_and_activates() {
    let mut scene = HandRigScene::default();
    assert!(scene.publish(single_instance_frame(), skin(), light(), 1.2, 7));
    assert!(scene.is_active());
}

#[test]
fn publish_rejects_a_wrong_sized_skin_and_stays_inactive() {
    let mut scene = HandRigScene::default();
    let bad_skin = vec![0u8; 10].into();
    assert!(!scene.publish(single_instance_frame(), bad_skin, light(), 1.2, 7));
    assert!(!scene.is_active());
}

#[test]
fn publish_rejects_non_finite_or_out_of_range_fov_and_zero_revision() {
    let mut scene = HandRigScene::default();
    assert!(!scene.publish(single_instance_frame(), skin(), light(), f32::NAN, 7));
    assert!(!scene.publish(single_instance_frame(), skin(), light(), 0.0, 7));
    assert!(!scene.publish(single_instance_frame(), skin(), light(), 4.0, 7));
    assert!(!scene.publish(single_instance_frame(), skin(), light(), 1.2, 0));
    assert!(!scene.is_active());
}

#[test]
fn publish_rejects_an_empty_rig_and_clears_a_prior_frame() {
    let mut scene = HandRigScene::default();
    assert!(scene.publish(single_instance_frame(), skin(), light(), 1.2, 7));
    assert!(!scene.publish(ActorRigRenderFrame::default(), skin(), light(), 1.2, 8));
    assert!(!scene.is_active());
}

#[test]
fn item_atlas_is_kept_only_when_its_pixel_count_matches_and_a_frame_is_active() {
    let atlas = |bytes: usize| HandItemAtlas {
        width: 2,
        height: 2,
        layers: 2,
        rgba8: Arc::from(vec![0u8; bytes]),
    };
    let mut scene = HandRigScene::default();
    scene.set_item_atlases([Some(atlas(32)), None]);
    assert!(!scene.is_active());

    assert!(scene.publish(single_instance_frame(), skin(), light(), 1.2, 7));
    scene.set_item_atlases([Some(atlas(31)), Some(atlas(32))]);
    let frame = scene.frame.as_ref().unwrap();
    assert!(frame.item_atlases[0].is_none());
    assert!(frame.item_atlases[1].is_some());
    scene.set_item_atlases([Some(atlas(32)), None]);
    assert!(scene.frame.as_ref().unwrap().item_atlases[0].is_some());
    assert!(scene.frame.as_ref().unwrap().item_atlases[1].is_none());
    scene.set_item_atlases([
        Some(HandItemAtlas {
            width: u16::MAX,
            height: u16::MAX,
            layers: u32::MAX,
            rgba8: Arc::from([]),
        }),
        None,
    ]);
    assert!(scene.frame.as_ref().unwrap().item_atlases[0].is_none());
}

/// A per-frame pose rewrites the same buffers instead of reallocating them and the bind group.
#[test]
fn pose_updates_reuse_their_buffers() {
    use bevy::{
        ecs::system::RunSystemOnce,
        render::renderer::{RenderDevice, RenderQueue, WgpuWrapper},
    };
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
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let mut world = bevy::prelude::World::new();
    world.insert_resource(device.clone());
    world.run_system_once(init_gpu).unwrap();
    let mut gpu = world.remove_resource::<HandRigGpu>().unwrap();
    let mut scene = HandRigScene::default();
    let mut buffers = Vec::new();
    for revision in 1..=3 {
        assert!(scene.publish(single_instance_frame(), skin(), light(), 1.2, revision));
        upload_pose(&mut gpu, &device, &queue, scene.frame.as_ref().unwrap());
        buffers.push(gpu.instances.as_ref().unwrap().id());
    }
    assert!(buffers.windows(2).all(|pair| pair[0] == pair[1]));

    // Main and offhand pages may have unrelated dimensions and layer counts.
    let main_pixels: Arc<[u8]> = vec![255; 16].into();
    let off_pixels: Arc<[u8]> = vec![127; 48].into();
    scene.set_item_atlases([
        Some(HandItemAtlas {
            width: 2,
            height: 2,
            layers: 1,
            rgba8: Arc::clone(&main_pixels),
        }),
        Some(HandItemAtlas {
            width: 4,
            height: 1,
            layers: 3,
            rgba8: Arc::clone(&off_pixels),
        }),
    ]);
    upload_atlas(&mut gpu, &device, &queue, scene.frame.as_ref().unwrap());
    assert_eq!(gpu.atlases[0].as_ref().unwrap().size, [2, 2, 1]);
    assert_eq!(gpu.atlases[1].as_ref().unwrap().size, [4, 1, 3]);
    assert!(Arc::ptr_eq(
        &gpu.atlases[1].as_ref().unwrap().pixels,
        &off_pixels
    ));

    // The same pixel allocation with new dimensions is not the same texture.
    scene.set_item_atlases([
        Some(HandItemAtlas {
            width: 1,
            height: 4,
            layers: 1,
            rgba8: main_pixels,
        }),
        Some(HandItemAtlas {
            width: 4,
            height: 1,
            layers: 3,
            rgba8: Arc::clone(&off_pixels),
        }),
    ]);
    upload_atlas(&mut gpu, &device, &queue, scene.frame.as_ref().unwrap());
    assert_eq!(gpu.atlases[0].as_ref().unwrap().size, [1, 4, 1]);
    assert_eq!(gpu.atlases[1].as_ref().unwrap().size, [4, 1, 3]);
    scene.set_item_atlases([
        None,
        Some(HandItemAtlas {
            width: 4,
            height: 1,
            layers: 3,
            rgba8: off_pixels,
        }),
    ]);
    upload_atlas(&mut gpu, &device, &queue, scene.frame.as_ref().unwrap());
    assert!(gpu.atlases[0].is_none());
    assert!(gpu.atlases[1].is_some());
}

#[test]
fn review_render_hand_atlas_rejects_device_dimension_and_layer_limits() {
    use bevy::{ecs::system::RunSystemOnce, render::renderer::WgpuWrapper};
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let mut world = World::new();
    world.insert_resource(device.clone());
    world.run_system_once(init_gpu).unwrap();
    let mut gpu = world.remove_resource::<HandRigGpu>().unwrap();
    let mut scene = HandRigScene::default();
    assert!(scene.publish(single_instance_frame(), skin(), light(), 1.2, 1));
    let wide = u16::try_from(device.limits().max_texture_dimension_2d + 1).unwrap();
    let layers = device.limits().max_texture_array_layers + 1;
    for (width, layer_count) in [(wide, 1), (1, layers)] {
        scene.set_item_atlases([
            Some(HandItemAtlas {
                width,
                height: 1,
                layers: layer_count,
                rgba8: vec![255; width as usize * layer_count as usize * 4].into(),
            }),
            None,
        ]);
        upload_atlas(&mut gpu, &device, &queue, scene.frame.as_ref().unwrap());
        assert!(gpu.atlases[0].is_none());
    }
}
