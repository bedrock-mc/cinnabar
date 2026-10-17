use bevy::prelude::Projection;
use render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS;

/// Rigs this far outside the view on every side still animate, so only a turn faster than this
/// in one tick shows a rig its held pose for that tick.
const ANIMATION_GUARD_DEGREES: f32 = 30.0;

/// Converts the camera orientation to the actor animation query angles.
pub(super) fn actor_camera_rotation(rotation: bevy::math::Quat) -> [f32; 2] {
    let (yaw, pitch, _) = rotation.to_euler(bevy::math::EulerRot::YXZ);
    [
        -pitch.to_degrees(),
        (180.0 - yaw.to_degrees()).rem_euclid(360.0),
    ]
}

/// The camera's frustum widened by the guard band, with the render distances.
pub(super) fn animation_view(
    transform: &bevy::prelude::Transform,
    projection: &Projection,
) -> Option<actor_animation::ActorAnimationView> {
    let Projection::Perspective(perspective) = projection else {
        return None;
    };
    let (guard, limit) = (ANIMATION_GUARD_DEGREES.to_radians(), 85f32.to_radians());
    let half_vertical = perspective.fov * 0.5;
    let half_horizontal = (half_vertical.tan() * perspective.aspect_ratio).atan();
    let (half_vertical, half_horizontal) = (
        (half_vertical + guard).min(limit),
        (half_horizontal + guard).min(limit),
    );
    let clip = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        half_vertical * 2.0,
        half_horizontal.tan() / half_vertical.tan(),
        perspective.near,
    ) * transform.to_matrix().inverse();
    let [x, y, z, w] = [0, 1, 2, 3].map(|row| clip.row(row));
    let planes = [w + x, w - x, w + y, w - y, z, w - z].map(|plane| plane.to_array());
    planes
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then(|| actor_animation::ActorAnimationView {
            planes,
            camera: transform.translation.to_array(),
            player_distance: MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
            entity_radius: render_model::ACTOR_CANDIDATE_RADIUS_BLOCKS,
        })
}

impl super::ActorPresentationState<'_, '_> {
    /// Keeps the hand's independent perspective through the world's portal projection.
    pub(super) fn hand_camera_fov(&self) -> Option<f32> {
        self.camera
            .single()
            .ok()
            .and_then(|(_, projection)| crate::camera::first_person_hand_fov(projection))
    }

    /// Captures the active render camera's actor admission bounds.
    pub(super) fn cull_view(&self) -> Option<render_model::ActorCullView> {
        self.camera
            .single()
            .ok()
            .map(|(transform, projection)| render_model::ActorCullView {
                clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
                camera_position: transform.translation,
                max_distance: MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
            })
    }
}

/// Applies admitted local swings and camera rotation without advancing actor ticks.
pub(super) fn refresh_interpolation(
    stream: Option<&mut chunk_pipeline::WorldStream>,
    swing_progress: Option<actor_animation::LocalSwingProgress>,
    rotation: bevy::math::Quat,
) {
    if let Some(stream) = stream {
        if let Some(progress) = swing_progress {
            stream.sync_local_swing(progress);
        }
        stream.set_actor_camera_rotation(actor_camera_rotation(rotation));
        stream.advance_actor_interpolation_frame(0);
    }
}
