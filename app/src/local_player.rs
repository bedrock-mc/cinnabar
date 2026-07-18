use bevy::prelude::{Quat, Res, ResMut, Resource, Single, SystemSet, Transform, Vec3, With};
use render::{ActorRenderSource, MAX_RENDERED_PLAYERS};
use semantic_input::PerspectiveMode;

use crate::{
    camera::{
        CameraSettingsAuthority, FlyCamera, collision_safe_perspective_pose, perspective_pose,
        unavailable_world_perspective_pose,
    },
    movement::PhysicsCollisionRegistries,
    runtime::world::ClientWorld,
};

pub const LOCAL_AVATAR_EYE_HEIGHT_BLOCKS: f32 = 1.62;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalPlayerFrameSet {
    Physics,
    Camera,
    Interaction,
}

/// Authoritative local eye position and view orientation.
///
/// Physics owns translation. Semantic look input owns rotation. Perspective
/// and camera collision never mutate this pose.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct LocalViewPose {
    eye_translation: Vec3,
    rotation: Quat,
}

impl Default for LocalViewPose {
    fn default() -> Self {
        Self {
            eye_translation: Vec3::new(0.0, 80.0, 0.0),
            rotation: Quat::IDENTITY,
        }
    }
}

impl LocalViewPose {
    #[must_use]
    pub fn new(eye_translation: Vec3, rotation: Quat) -> Self {
        let mut pose = Self::default();
        pose.set_eye_translation(eye_translation);
        pose.set_rotation(rotation);
        pose
    }

    #[must_use]
    pub const fn eye_translation(self) -> Vec3 {
        self.eye_translation
    }

    #[must_use]
    pub const fn rotation(self) -> Quat {
        self.rotation
    }

    pub fn set_eye_translation(&mut self, translation: Vec3) {
        if translation.is_finite() {
            self.eye_translation = translation;
        }
    }

    pub fn set_rotation(&mut self, rotation: Quat) {
        if rotation.is_finite() && rotation.length_squared() > f32::EPSILON {
            self.rotation = rotation.normalize();
        }
    }
}

/// Collision-resolved transform presented by the one production camera writer.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct CameraPose {
    transform: Transform,
}

impl Default for CameraPose {
    fn default() -> Self {
        let view = LocalViewPose::default();
        Self::new(perspective_pose(
            view.eye_translation(),
            view.rotation(),
            PerspectiveMode::FirstPerson,
        ))
    }
}

impl CameraPose {
    #[must_use]
    pub const fn new(transform: Transform) -> Self {
        Self { transform }
    }

    #[must_use]
    pub const fn transform(&self) -> &Transform {
        &self.transform
    }
}

/// Frame-frozen eye ray used by interaction and outbound movement sampling.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct InteractionOriginSnapshot {
    frame_sequence: u64,
    origin: Vec3,
    direction: Vec3,
}

impl Default for InteractionOriginSnapshot {
    fn default() -> Self {
        Self::from_local_view(0, LocalViewPose::default())
    }
}

impl InteractionOriginSnapshot {
    #[must_use]
    pub fn from_local_view(frame_sequence: u64, view: LocalViewPose) -> Self {
        Self {
            frame_sequence,
            origin: view.eye_translation(),
            direction: (view.rotation() * Vec3::NEG_Z).normalize_or_zero(),
        }
    }

    #[must_use]
    pub const fn frame_sequence(self) -> u64 {
        self.frame_sequence
    }

    #[must_use]
    pub const fn origin(self) -> Vec3 {
        self.origin
    }

    #[must_use]
    pub const fn direction(self) -> Vec3 {
        self.direction
    }
}

/// Session-scoped local body publication. It removes every duplicate local
/// runtime ID before optionally adding exactly one third-person source.
#[derive(Resource, Debug, Default)]
pub struct LocalAvatarPresentation {
    session_generation: u64,
    runtime_id: Option<u64>,
    retired_runtime_id: Option<u64>,
    movement_revision: u64,
}

impl LocalAvatarPresentation {
    pub fn begin_session(&mut self, session_generation: u64, runtime_id: u64) {
        self.retired_runtime_id = self.runtime_id;
        self.session_generation = session_generation;
        self.runtime_id = (runtime_id != 0).then_some(runtime_id);
        self.movement_revision = 0;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn reconcile_sources(
        &mut self,
        perspective: PerspectiveMode,
        view: LocalViewPose,
        sources: &mut Vec<ActorRenderSource>,
    ) {
        if let Some(retired_runtime_id) = self.retired_runtime_id.take() {
            sources.retain(|source| source.runtime_id != retired_runtime_id);
        }
        let Some(runtime_id) = self.runtime_id else {
            return;
        };
        sources.retain(|source| source.runtime_id != runtime_id);
        if perspective == PerspectiveMode::FirstPerson {
            return;
        }
        sources.sort_unstable_by_key(|source| source.runtime_id);
        sources.dedup_by_key(|source| source.runtime_id);
        sources.truncate(MAX_RENDERED_PLAYERS.saturating_sub(1));
        self.movement_revision = self.movement_revision.wrapping_add(1);
        let (yaw, pitch, _) = view.rotation().to_euler(bevy::math::EulerRot::YXZ);
        let yaw_degrees = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
        let pitch_degrees = -pitch.to_degrees();
        let mut position = view.eye_translation();
        position.y -= LOCAL_AVATAR_EYE_HEIGHT_BLOCKS;
        sources.push(ActorRenderSource {
            runtime_id,
            unique_id: i64::try_from(runtime_id).unwrap_or(i64::MAX),
            spawn_revision: self.session_generation,
            movement_revision: self.movement_revision,
            previous_position: position.to_array(),
            previous_pitch_degrees: pitch_degrees,
            previous_yaw_degrees: yaw_degrees,
            previous_head_yaw_degrees: yaw_degrees,
            position: position.to_array(),
            pitch_degrees,
            yaw_degrees,
            head_yaw_degrees: yaw_degrees,
            teleported: false,
            skin: None,
        });
    }
}

pub fn reset_local_player_session(
    session_generation: u64,
    runtime_id: u64,
    eye_position: [f32; 3],
    settings: &mut CameraSettingsAuthority,
    view: &mut LocalViewPose,
    avatar: &mut LocalAvatarPresentation,
) {
    settings.reset_perspective();
    view.set_eye_translation(Vec3::from_array(eye_position));
    avatar.begin_session(session_generation, runtime_id);
}

pub(crate) fn resolve_camera_pose(
    client_world: Res<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    mut published: ResMut<CameraPose>,
    mut camera_transform: Single<&mut Transform, With<FlyCamera>>,
) {
    let perspective = settings.perspective();
    let transform = if let Some(stream) = client_world.stream.as_ref() {
        let collision_world = sim::PaletteWorld::new(
            stream.collision_store(),
            collisions.registry(stream.network_id_mode()),
            stream.current_dimension(),
        );
        collision_safe_perspective_pose(
            view.eye_translation(),
            view.rotation(),
            perspective,
            &collision_world,
        )
    } else {
        unavailable_world_perspective_pose(view.eye_translation(), view.rotation(), perspective)
    };
    **camera_transform = transform;
    published.transform = transform;
}

pub(crate) fn publish_interaction_origin(
    view: Res<LocalViewPose>,
    mut snapshot: ResMut<InteractionOriginSnapshot>,
) {
    let sequence = snapshot.frame_sequence.wrapping_add(1);
    *snapshot = InteractionOriginSnapshot::from_local_view(sequence, *view);
}
