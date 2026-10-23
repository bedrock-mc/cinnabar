use std::sync::Arc;

use bevy::prelude::PerspectiveProjection;

use super::*;
use crate::actor_publication::HAND_FOV_DEGREES;

#[test]
fn fishing_line_anchor_follows_the_interpolated_held_item_bone() {
    let bone = |x| render_model::RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [x, 0.0, 0.0, 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    let submission = render_model::ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: render_model::ActorRigRenderInput {
            identity: render_model::ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 7,
                spawn_revision: 1,
                ingress_sequence: 1,
                source_tick: None,
                movement_revision: 0,
                pose_generation: render_api::PoseGeneration::new(1),
                layer: crate::presentation::equipment::LAYER_MAIN_HAND,
            },
            rig: render_model::EntityRigId(1),
            previous_bones: Arc::from([bone(0.0)]),
            current_bones: Arc::from([bone(1.0)]),
            completed_tick: 1,
            reset_generation: 0,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
        ],
        texture_layer: 0,
        route: render_model::ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: render_model::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    };
    let batch = crate::presentation::actors::ActorPresentationBatch {
        submissions: vec![submission],
        skin_layers: Vec::new(),
        artwork: Default::default(),
    };

    assert!(fishing_line_anchors_for(&batch, 0.5, |_| false).is_empty());
    assert_eq!(
        fishing_line_anchors_for(&batch, 0.5, |_| true).as_slice(),
        &[FishingLineAnchor {
            runtime_id: 7,
            from: [9.5625, 20.9375, 30.0],
        }]
    );
}

#[test]
fn first_person_anchor_reprojection_preserves_the_hand_screen_position() {
    let camera = Transform::from_xyz(10.0, 20.0, 30.0);
    let aspect = 16.0 / 9.0;
    let hand_fov = HAND_FOV_DEGREES.to_radians();
    let point = [0.4, -0.2, -0.9];
    let projection = Projection::Perspective(PerspectiveProjection {
        fov: 100.0_f32.to_radians(),
        aspect_ratio: aspect,
        ..Default::default()
    });
    let view = hand_point_to_world_view(point, &projection, aspect, hand_fov).unwrap();
    let world = camera.to_matrix().transform_point3(view);
    assert!(world.is_finite());

    let hand_projection = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        hand_fov,
        aspect,
        render_api::CAMERA_NEAR_PLANE_BLOCKS,
    );
    let hand_ndc = (hand_projection * Vec3::from_array(point).extend(1.0)).truncate();
    let hand_w = (hand_projection * Vec3::from_array(point).extend(1.0)).w;
    let world_clip = projection.get_clip_from_view() * view.extend(1.0);
    assert!((world_clip.x / world_clip.w - hand_ndc.x / hand_w).abs() < 1e-5);
    assert!((world_clip.y / world_clip.w - hand_ndc.y / hand_w).abs() < 1e-5);
}
