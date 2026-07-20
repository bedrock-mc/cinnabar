use bevy::prelude::{Quat, Res, ResMut, Resource, Single, SystemSet, Transform, Vec3, With};
use semantic_input::PerspectiveMode;
use sim::WorldCollisionIdentity;

use crate::{
    camera::{
        CameraSettingsAuthority, FlyCamera, collision_safe_perspective_pose, perspective_pose,
        unavailable_world_perspective_pose,
    },
    environment::WorldClock,
    movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    runtime::world::ClientWorld,
};

pub const LOCAL_AVATAR_EYE_HEIGHT_BLOCKS: f32 = 1.62;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalPlayerFrameSet {
    Physics,
    Camera,
    Interaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalPlayerFrameReset {
    Correction,
    Session,
    Dimension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalPlayerFrameError {
    NonFinitePose,
    NonFiniteEye,
    InvalidRotation,
    PoseGenerationExhausted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalPlayerFrameSample {
    pub session_generation: u64,
    pub fifo_sequence: u64,
    pub physics_tick: u64,
    pub perspective: PerspectiveMode,
    pub world_collision_identity: WorldCollisionIdentity,
    pub pose: Transform,
    pub eye: Vec3,
    pub rotation: Quat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrozenLocalPlayerFrame {
    session_generation: u64,
    fifo_sequence: u64,
    physics_tick: u64,
    pose_generation: u64,
    perspective: PerspectiveMode,
    world_collision_identity: WorldCollisionIdentity,
    pose: Transform,
    eye: Vec3,
    rotation: Quat,
    direction: Vec3,
}

impl FrozenLocalPlayerFrame {
    #[must_use]
    pub const fn session_generation(&self) -> u64 {
        self.session_generation
    }

    #[must_use]
    pub const fn fifo_sequence(&self) -> u64 {
        self.fifo_sequence
    }

    #[must_use]
    pub const fn physics_tick(&self) -> u64 {
        self.physics_tick
    }

    #[must_use]
    pub const fn pose_generation(&self) -> u64 {
        self.pose_generation
    }

    #[must_use]
    pub const fn perspective(&self) -> PerspectiveMode {
        self.perspective
    }

    #[must_use]
    pub const fn world_collision_identity(&self) -> &WorldCollisionIdentity {
        &self.world_collision_identity
    }

    #[must_use]
    pub const fn pose(&self) -> &Transform {
        &self.pose
    }

    #[must_use]
    pub const fn eye(&self) -> Vec3 {
        self.eye
    }

    #[must_use]
    pub const fn rotation(&self) -> Quat {
        self.rotation
    }

    #[must_use]
    pub const fn direction(&self) -> Vec3 {
        self.direction
    }
}

/// One frame-frozen handoff consumed by camera, interaction, publication, and
/// network sampling. A successful publish replaces the complete identity and
/// pose atomically; malformed samples leave the prior frame untouched.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct LocalPlayerFrameCarrier {
    pose_generation: u64,
    snapshot: Option<FrozenLocalPlayerFrame>,
}

impl LocalPlayerFrameCarrier {
    pub fn publish(&mut self, sample: LocalPlayerFrameSample) -> Result<(), LocalPlayerFrameError> {
        if !sample.pose.translation.is_finite()
            || !sample.pose.rotation.is_finite()
            || !sample.pose.scale.is_finite()
        {
            return Err(LocalPlayerFrameError::NonFinitePose);
        }
        if !sample.eye.is_finite() {
            return Err(LocalPlayerFrameError::NonFiniteEye);
        }
        if !sample.rotation.is_finite() || sample.rotation.length_squared() <= f32::EPSILON {
            return Err(LocalPlayerFrameError::InvalidRotation);
        }
        let pose_generation = self
            .pose_generation
            .checked_add(1)
            .ok_or(LocalPlayerFrameError::PoseGenerationExhausted)?;
        let rotation = sample.rotation.normalize();
        let snapshot = FrozenLocalPlayerFrame {
            session_generation: sample.session_generation,
            fifo_sequence: sample.fifo_sequence,
            physics_tick: sample.physics_tick,
            pose_generation,
            perspective: sample.perspective,
            world_collision_identity: sample.world_collision_identity,
            pose: sample.pose,
            eye: sample.eye,
            rotation,
            direction: (rotation * Vec3::NEG_Z).normalize_or_zero(),
        };
        self.pose_generation = pose_generation;
        self.snapshot = Some(snapshot);
        Ok(())
    }

    pub fn reset(&mut self, _reason: LocalPlayerFrameReset) {
        if self.snapshot.take().is_some() {
            self.pose_generation = self.pose_generation.saturating_add(1);
        }
    }

    #[must_use]
    pub const fn snapshot(&self) -> Option<&FrozenLocalPlayerFrame> {
        self.snapshot.as_ref()
    }
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

/// One immutable eye ray and its complete simulation/publication identity.
#[derive(Debug, Clone, PartialEq)]
pub struct FrozenInteractionOrigin {
    session_generation: u64,
    fifo_sequence: u64,
    physics_tick: u64,
    pose_generation: u64,
    perspective: PerspectiveMode,
    world_collision_identity: WorldCollisionIdentity,
    origin: Vec3,
    direction: Vec3,
}

impl FrozenInteractionOrigin {
    #[must_use]
    pub const fn session_generation(&self) -> u64 {
        self.session_generation
    }

    #[must_use]
    pub const fn fifo_sequence(&self) -> u64 {
        self.fifo_sequence
    }

    #[must_use]
    pub const fn physics_tick(&self) -> u64 {
        self.physics_tick
    }

    #[must_use]
    pub const fn pose_generation(&self) -> u64 {
        self.pose_generation
    }

    #[must_use]
    pub const fn perspective(&self) -> PerspectiveMode {
        self.perspective
    }

    #[must_use]
    pub const fn world_collision_identity(&self) -> &WorldCollisionIdentity {
        &self.world_collision_identity
    }

    #[must_use]
    pub const fn origin(&self) -> Vec3 {
        self.origin
    }

    #[must_use]
    pub const fn direction(&self) -> Vec3 {
        self.direction
    }
}

/// Frame-frozen interaction/outbound ray. Absence is authoritative after a
/// correction, session replacement, dimension replacement, or unavailable
/// completed physics frame.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct InteractionOriginSnapshot(Option<FrozenInteractionOrigin>);

impl InteractionOriginSnapshot {
    pub fn publish_from_local_player_frame(&mut self, carrier: &LocalPlayerFrameCarrier) {
        self.0 = carrier.snapshot().map(|frame| FrozenInteractionOrigin {
            session_generation: frame.session_generation(),
            fifo_sequence: frame.fifo_sequence(),
            physics_tick: frame.physics_tick(),
            pose_generation: frame.pose_generation(),
            perspective: frame.perspective(),
            world_collision_identity: frame.world_collision_identity().clone(),
            origin: frame.eye(),
            direction: frame.direction(),
        });
    }

    #[must_use]
    pub const fn outbound_ray(&self) -> Option<&FrozenInteractionOrigin> {
        self.0.as_ref()
    }

    #[must_use]
    pub fn outbound_ray_for_authority(
        &self,
        session_generation: u64,
        fifo_sequence: u64,
        physics_tick: u64,
        pose_generation: u64,
        world_collision_identity: &WorldCollisionIdentity,
    ) -> Option<&FrozenInteractionOrigin> {
        self.0.as_ref().filter(|ray| {
            ray.session_generation == session_generation
                && ray.fifo_sequence == fifo_sequence
                && ray.physics_tick == physics_tick
                && ray.pose_generation == pose_generation
                && &ray.world_collision_identity == world_collision_identity
        })
    }

    pub fn invalidate(&mut self) {
        self.0 = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrozenLocalAvatarVisibility {
    session_generation: u64,
    runtime_id: u64,
    pose_generation: u64,
    visible: bool,
    eye: Vec3,
    rotation: Quat,
}

impl FrozenLocalAvatarVisibility {
    #[must_use]
    pub const fn session_generation(self) -> u64 {
        self.session_generation
    }

    #[must_use]
    pub const fn runtime_id(self) -> u64 {
        self.runtime_id
    }

    #[must_use]
    pub const fn pose_generation(self) -> u64 {
        self.pose_generation
    }

    #[must_use]
    pub const fn visible(self) -> bool {
        self.visible
    }

    #[must_use]
    pub const fn eye(self) -> Vec3 {
        self.eye
    }

    #[must_use]
    pub const fn rotation(self) -> Quat {
        self.rotation
    }
}

#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct LocalAvatarVisibilityCarrier(Option<FrozenLocalAvatarVisibility>);

impl LocalAvatarVisibilityCarrier {
    #[must_use]
    pub const fn snapshot(&self) -> Option<&FrozenLocalAvatarVisibility> {
        self.0.as_ref()
    }

    fn replace(&mut self, snapshot: FrozenLocalAvatarVisibility) {
        self.0 = Some(snapshot);
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }
}

/// Session-scoped local body identity. Phase 3 publishes only a frozen
/// visibility/pose handoff; the Phase 4 render arena owns culling and capacity.
#[derive(Resource, Debug, Default)]
pub struct LocalAvatarPresentation {
    session_generation: u64,
    runtime_id: Option<u64>,
}

impl LocalAvatarPresentation {
    pub fn begin_session(&mut self, session_generation: u64, runtime_id: u64) {
        self.session_generation = session_generation;
        self.runtime_id = (runtime_id != 0).then_some(runtime_id);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn publish_visibility(
        &self,
        frame: &FrozenLocalPlayerFrame,
        carrier: &mut LocalAvatarVisibilityCarrier,
    ) {
        let Some(runtime_id) = self.runtime_id else {
            carrier.clear();
            return;
        };
        if frame.session_generation() != self.session_generation {
            carrier.clear();
            return;
        }
        carrier.replace(FrozenLocalAvatarVisibility {
            session_generation: self.session_generation,
            runtime_id,
            pose_generation: frame.pose_generation(),
            visible: frame.perspective() != PerspectiveMode::FirstPerson,
            eye: frame.eye(),
            rotation: frame.rotation(),
        });
    }

    /// Publishes the local body from a current subject pose selected by the
    /// caller. Rendering perspective is not movement or interaction authority;
    /// callers must not pass a boomed third-person camera translation here.
    pub fn publish_view_visibility(
        &self,
        perspective: PerspectiveMode,
        eye: Vec3,
        rotation: Quat,
        carrier: &mut LocalAvatarVisibilityCarrier,
    ) {
        let Some(runtime_id) = self.runtime_id else {
            carrier.clear();
            return;
        };
        let rotation_length_squared = rotation.length_squared();
        if self.session_generation == 0
            || !eye.is_finite()
            || !rotation.is_finite()
            || !rotation_length_squared.is_finite()
            || rotation_length_squared <= f32::EPSILON
        {
            carrier.clear();
            return;
        }
        let visible = perspective != PerspectiveMode::FirstPerson;
        let rotation = rotation.normalize();
        let prior = carrier.snapshot().filter(|snapshot| {
            snapshot.session_generation == self.session_generation
                && snapshot.runtime_id == runtime_id
        });
        if prior.is_some_and(|snapshot| {
            snapshot.visible == visible && snapshot.eye == eye && snapshot.rotation == rotation
        }) {
            return;
        }
        let pose_generation =
            prior.map_or(Some(1), |snapshot| snapshot.pose_generation.checked_add(1));
        let Some(pose_generation) = pose_generation else {
            carrier.clear();
            return;
        };
        carrier.replace(FrozenLocalAvatarVisibility {
            session_generation: self.session_generation,
            runtime_id,
            pose_generation,
            visible,
            eye,
            rotation,
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
    carrier: Res<LocalPlayerFrameCarrier>,
    mut snapshot: ResMut<InteractionOriginSnapshot>,
) {
    snapshot.publish_from_local_player_frame(&carrier);
}

pub(crate) fn publish_local_player_frame(
    client_world: Res<ClientWorld>,
    clock: Res<WorldClock>,
    local_physics: Res<LocalPhysicsController>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    camera: Res<CameraPose>,
    mut carrier: ResMut<LocalPlayerFrameCarrier>,
) {
    let Some(stream) = client_world.stream.as_ref() else {
        carrier.reset(LocalPlayerFrameReset::Session);
        return;
    };
    let (Some(state), Some(world_collision_identity)) =
        (local_physics.state(), local_physics.last_world_identity())
    else {
        carrier.reset(LocalPlayerFrameReset::Correction);
        return;
    };
    let sample = LocalPlayerFrameSample {
        session_generation: clock.session_generation(),
        fifo_sequence: stream.committed_sequence(),
        physics_tick: state.tick,
        perspective: settings.perspective(),
        world_collision_identity: world_collision_identity.clone(),
        pose: *camera.transform(),
        eye: view.eye_translation(),
        rotation: view.rotation(),
    };
    if carrier.publish(sample).is_err() {
        carrier.reset(LocalPlayerFrameReset::Correction);
    }
}
