use protocol::identity::FifoSequence;
use render_api::PoseGeneration;
use simulation_clock::PhysicsTick;
use simulation_clock::identity::SessionGeneration;

use bevy::prelude::{
    Entity, EulerRot, Quat, Res, ResMut, Resource, Single, SystemSet, Transform, Vec3, With,
};
use semantic_input::PerspectiveMode;
use sim::WorldCollisionIdentity;

mod sleeping;

use crate::camera::{
    CameraSettingsAuthority, FlyCamera, collision_safe_perspective_pose, collision_safe_rig_pose,
    perspective_pose, unavailable_world_perspective_pose,
};

pub const LOCAL_AVATAR_EYE_HEIGHT_BLOCKS: f32 = protocol::STANDING_PLAYER_EYE_HEIGHT;

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
    NonFiniteFeet,
    InvalidRotation,
    PoseGenerationExhausted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalPlayerFrameSample {
    pub session_generation: SessionGeneration,
    pub actor_session_id: u64,
    pub fifo_sequence: FifoSequence,
    pub physics_tick: PhysicsTick,
    pub perspective: PerspectiveMode,
    pub world_collision_identity: WorldCollisionIdentity,
    pub pose: Transform,
    pub eye: Vec3,
    pub feet: Vec3,
    pub rotation: Quat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrozenLocalPlayerFrame {
    session_generation: SessionGeneration,
    actor_session_id: u64,
    fifo_sequence: FifoSequence,
    physics_tick: PhysicsTick,
    pose_generation: PoseGeneration,
    perspective: PerspectiveMode,
    world_collision_identity: WorldCollisionIdentity,
    pose: Transform,
    eye: Vec3,
    feet: Vec3,
    rotation: Quat,
    direction: Vec3,
}

impl FrozenLocalPlayerFrame {
    #[must_use]
    pub const fn session_generation(&self) -> SessionGeneration {
        self.session_generation
    }

    #[must_use]
    pub const fn actor_session_id(&self) -> u64 {
        self.actor_session_id
    }

    #[must_use]
    pub const fn fifo_sequence(&self) -> FifoSequence {
        self.fifo_sequence
    }

    #[must_use]
    pub const fn physics_tick(&self) -> PhysicsTick {
        self.physics_tick
    }

    #[must_use]
    pub const fn pose_generation(&self) -> PoseGeneration {
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
    pub const fn feet(&self) -> Vec3 {
        self.feet
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
    pose_generation: PoseGeneration,
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
        if !sample.feet.is_finite() {
            return Err(LocalPlayerFrameError::NonFiniteFeet);
        }
        let norm = sample.rotation.length_squared();
        if !sample.rotation.is_finite() || !norm.is_finite() || norm <= f32::EPSILON {
            return Err(LocalPlayerFrameError::InvalidRotation);
        }
        let pose_generation = self
            .pose_generation
            .checked_next()
            .ok_or(LocalPlayerFrameError::PoseGenerationExhausted)?;
        let rotation = sample.rotation.normalize();
        let snapshot = FrozenLocalPlayerFrame {
            session_generation: sample.session_generation,
            actor_session_id: sample.actor_session_id,
            fifo_sequence: sample.fifo_sequence,
            physics_tick: sample.physics_tick,
            pose_generation,
            perspective: sample.perspective,
            world_collision_identity: sample.world_collision_identity,
            pose: sample.pose,
            eye: sample.eye,
            feet: sample.feet,
            rotation,
            direction: (rotation * Vec3::NEG_Z).normalize_or_zero(),
        };
        self.pose_generation = pose_generation;
        self.snapshot = Some(snapshot);
        Ok(())
    }

    pub fn reset(&mut self, _reason: LocalPlayerFrameReset) {
        if self.snapshot.take().is_some() {
            self.pose_generation = self.pose_generation.saturating_next();
        }
    }

    #[must_use]
    pub const fn snapshot(&self) -> Option<&FrozenLocalPlayerFrame> {
        self.snapshot.as_ref()
    }
}

/// Retains the physics position, input look, and one shared sleeping presentation pose.
/// Presentation getters can read the bed pose without changing physics or gameplay aim.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct LocalViewPose {
    eye_translation: Vec3,
    feet_translation: Vec3,
    rotation: ViewRotation,
    freelook_rotation: Option<ViewRotation>,
    detached_eye: Option<Vec3>,
    camera_reanchor_epoch: u64,
    sleeping: Option<sleeping::SleepingPose>,
    sleeping_camera: bool,
    sleeping_changed: bool,
}

/// Keeps complete yaw turns that a quaternion alone cannot represent.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ViewRotation {
    rotation: Quat,
    yaw: f32,
}

impl ViewRotation {
    /// Reads continuous yaw together with the orientation's bounded pitch and roll.
    fn angles(self) -> (f32, f32, f32) {
        let (_, pitch, roll) = self.rotation.to_euler(EulerRot::YXZ);
        (self.yaw, pitch, roll)
    }

    /// Stores a valid orientation, using an explicit yaw or its nearest equivalent heading.
    fn set(&mut self, rotation: Quat, yaw: Option<f32>) {
        let norm = rotation.length_squared();
        if !rotation.is_finite() || !norm.is_finite() || norm <= f32::EPSILON {
            return;
        }
        if yaw.is_some_and(|yaw| !yaw.is_finite()) {
            return;
        }
        self.rotation = rotation.normalize();
        self.yaw = yaw.unwrap_or_else(|| {
            let (yaw, _, _) = self.rotation.to_euler(EulerRot::YXZ);
            let delta = (yaw - self.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            self.yaw + delta
        });
    }
}

impl Default for LocalViewPose {
    fn default() -> Self {
        Self {
            eye_translation: Vec3::new(0.0, 80.0, 0.0),
            feet_translation: Vec3::new(0.0, 80.0 - protocol::PLAYER_NETWORK_OFFSET, 0.0),
            rotation: ViewRotation {
                rotation: Quat::IDENTITY,
                yaw: 0.0,
            },
            freelook_rotation: None,
            detached_eye: None,
            camera_reanchor_epoch: 0,
            sleeping: None,
            sleeping_camera: false,
            sleeping_changed: false,
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
        match self.sleeping {
            Some(pose) => pose.eye,
            None => self.eye_translation,
        }
    }

    #[must_use]
    pub const fn feet_translation(self) -> Vec3 {
        match self.sleeping {
            Some(pose) => pose.feet,
            None => self.feet_translation,
        }
    }

    #[must_use]
    pub const fn rotation(self) -> Quat {
        self.rotation.rotation
    }

    /// The input heading, including freelook, before any sleeping presentation override.
    pub fn look_rotation(self) -> Quat {
        self.freelook_rotation.unwrap_or(self.rotation).rotation
    }

    /// Presentation pitch and continuous yaw in Minecraft degrees.
    pub fn camera_look(self) -> [f32; 2] {
        let (yaw, pitch, _) = if self.sleeping_camera
            && let Some(pose) = self.sleeping
        {
            pose.rotation.to_euler(EulerRot::YXZ)
        } else {
            self.freelook_rotation.unwrap_or(self.rotation).angles()
        };
        [-pitch.to_degrees(), -yaw.to_degrees()]
    }

    /// The shared presentation heading; third-person orbit and gameplay keep their input heading.
    pub fn camera_rotation(self) -> Quat {
        if self.sleeping_camera
            && let Some(pose) = self.sleeping
        {
            return pose.rotation;
        }
        self.look_rotation()
    }

    /// Starts or releases a camera-only translation, without relocating physics or interaction origins.
    pub fn set_detached_translation(&mut self, enabled: bool) {
        if enabled {
            self.detached_eye.get_or_insert(self.eye_translation);
        } else {
            self.detached_eye = None;
        }
    }

    /// The detached camera's current eye, falling back to the body's eye.
    pub fn detached_eye(&self) -> Vec3 {
        self.detached_eye.unwrap_or(self.eye_translation)
    }

    /// Advances only an existing detached view; gameplay's subject position is unchanged.
    pub fn move_detached_eye(&mut self, position: Vec3) {
        if self.detached_eye.is_some() && position.is_finite() {
            self.detached_eye = Some(position);
        }
    }

    /// Restores the actor heading on release with the winding nearest the outgoing camera.
    pub fn set_freelook(&mut self, held: bool) {
        if held {
            self.freelook_rotation.get_or_insert(self.rotation);
        } else if let Some(freelook) = self.freelook_rotation.take() {
            let turns = ((freelook.yaw - self.rotation.yaw) / std::f32::consts::TAU).round();
            self.rotation.yaw += turns * std::f32::consts::TAU;
        }
    }

    pub fn set_look_rotation(&mut self, rotation: Quat) {
        self.freelook_rotation
            .as_mut()
            .unwrap_or(&mut self.rotation)
            .set(rotation, None);
    }

    /// Stores the input orientation and its full yaw delta in radians for hand inertia.
    pub fn turn_look_rotation(&mut self, rotation: Quat, yaw_delta: f32) {
        let heading = self
            .freelook_rotation
            .as_mut()
            .unwrap_or(&mut self.rotation);
        heading.set(rotation, Some(heading.yaw + yaw_delta));
    }

    /// Invalidates camera presentation history without changing gameplay or network state.
    pub fn reanchor_camera(&mut self) {
        self.camera_reanchor_epoch = self.camera_reanchor_epoch.wrapping_add(1);
    }

    pub const fn camera_reanchor_epoch(&self) -> u64 {
        self.camera_reanchor_epoch
    }

    pub fn set_eye_translation(&mut self, translation: Vec3) {
        if translation.is_finite() {
            self.feet_translation += translation - self.eye_translation;
            self.eye_translation = translation;
        }
    }

    /// Physics publishes both origins together: visual eye lowering never moves actor feet.
    pub fn set_subject_position(&mut self, eye: Vec3, feet: Vec3) {
        if eye.is_finite() && feet.is_finite() {
            self.eye_translation = eye;
            self.feet_translation = feet;
        }
    }

    pub fn set_rotation(&mut self, rotation: Quat) {
        self.rotation.set(rotation, None);
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
    session_generation: SessionGeneration,
    actor_session_id: u64,
    fifo_sequence: FifoSequence,
    physics_tick: PhysicsTick,
    pose_generation: PoseGeneration,
    perspective: PerspectiveMode,
    world_collision_identity: WorldCollisionIdentity,
    origin: Vec3,
    direction: Vec3,
}

impl FrozenInteractionOrigin {
    #[must_use]
    pub const fn session_generation(&self) -> SessionGeneration {
        self.session_generation
    }

    #[must_use]
    pub const fn actor_session_id(&self) -> u64 {
        self.actor_session_id
    }

    #[must_use]
    pub const fn fifo_sequence(&self) -> FifoSequence {
        self.fifo_sequence
    }

    #[must_use]
    pub const fn physics_tick(&self) -> PhysicsTick {
        self.physics_tick
    }

    #[must_use]
    pub const fn pose_generation(&self) -> PoseGeneration {
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
    /// Assists only the frozen interaction direction, preserving its authority and eye origin.
    pub fn assist_direction(&mut self, direction: Vec3) {
        if let Some(ray) = self.0.as_mut()
            && let Some(direction) = direction.try_normalize()
            && direction.is_finite()
        {
            ray.direction = direction;
        }
    }

    pub fn publish_from_local_player_frame(&mut self, carrier: &LocalPlayerFrameCarrier) {
        self.0 = carrier.snapshot().map(|frame| FrozenInteractionOrigin {
            session_generation: frame.session_generation(),
            actor_session_id: frame.actor_session_id(),
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
        session_generation: SessionGeneration,
        fifo_sequence: FifoSequence,
        physics_tick: PhysicsTick,
        pose_generation: PoseGeneration,
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
    session_generation: SessionGeneration,
    runtime_id: u64,
    pose_generation: PoseGeneration,
    visible: bool,
    eye: Vec3,
    feet: Vec3,
    rotation: Quat,
}

impl FrozenLocalAvatarVisibility {
    #[must_use]
    pub const fn session_generation(self) -> SessionGeneration {
        self.session_generation
    }

    #[must_use]
    pub const fn runtime_id(self) -> u64 {
        self.runtime_id
    }

    #[must_use]
    pub const fn pose_generation(self) -> PoseGeneration {
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
    pub const fn feet(self) -> Vec3 {
        self.feet
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

/// Session-scoped local body identity. Movement publishes only a frozen
/// visibility/pose handoff; the actor render arena owns culling and capacity.
#[derive(Resource, Debug, Default)]
pub struct LocalAvatarPresentation {
    session_generation: SessionGeneration,
    runtime_id: Option<u64>,
}

impl LocalAvatarPresentation {
    pub fn begin_session(&mut self, session_generation: SessionGeneration, runtime_id: u64) {
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
            feet: frame.feet(),
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
        feet: Vec3,
        rotation: Quat,
        carrier: &mut LocalAvatarVisibilityCarrier,
    ) {
        let Some(runtime_id) = self.runtime_id else {
            carrier.clear();
            return;
        };
        let rotation_length_squared = rotation.length_squared();
        if self.session_generation.get() == 0
            || !eye.is_finite()
            || !feet.is_finite()
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
            snapshot.visible == visible
                && snapshot.eye == eye
                && snapshot.feet == feet
                && snapshot.rotation == rotation
        }) {
            return;
        }
        let pose_generation = prior.map_or(Some(PoseGeneration::new(1)), |snapshot| {
            snapshot.pose_generation.checked_next()
        });
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
            feet,
            rotation,
        });
    }
}

pub fn reset_local_player_session(
    session_generation: SessionGeneration,
    runtime_id: u64,
    eye_position: [f32; 3],
    settings: &mut CameraSettingsAuthority,
    view: &mut LocalViewPose,
    avatar: &mut LocalAvatarPresentation,
) {
    settings.reset_perspective();
    view.reanchor_camera();
    view.set_freelook(false);
    view.set_detached_translation(false);
    view.set_sleeping_presentation(None, false);
    let eye = Vec3::from_array(eye_position);
    view.set_subject_position(eye, eye - Vec3::Y * protocol::PLAYER_NETWORK_OFFSET);
    avatar.begin_session(session_generation, runtime_id);
}

pub fn publish_interaction_origin(
    carrier: Res<LocalPlayerFrameCarrier>,
    mut snapshot: ResMut<InteractionOriginSnapshot>,
) {
    snapshot.publish_from_local_player_frame(&carrier);
}

/// Publication identities observed from the completed gameplay frame.
pub struct CameraPublicationContext<'a, 'w> {
    pub clock: crate::observations::SessionObservation,
    pub physics: &'a dyn crate::observations::PhysicsObservation,
    pub receipt: ResMut<'w, crate::local_player_camera_receipt::CameraPublicationAttempt>,
}

/// Height of a bed's top above its block, where a sleeper lies.
pub const BED_TOP_HEIGHT: f32 = 0.5625;

/// The eye and feet a player sleeping in `bed` presents: lying on the bed top at the block's
/// centre, with the eye at the sleeping actor-state offset.
#[must_use]
pub fn sleeping_subject_position(bed: [i32; 3]) -> (Vec3, Vec3) {
    let feet = Vec3::new(
        bed[0] as f32 + 0.5,
        bed[1] as f32 + BED_TOP_HEIGHT,
        bed[2] as f32 + 0.5,
    );
    (
        feet + Vec3::Y * client_world::SLEEPING_PLAYER_NETWORK_OFFSET,
        feet,
    )
}

/// The view a player sleeping with `sleep_rotation` degrees looks along: from the pillow
/// toward the foot of the bed, level, matching the sleeping animation's `270 - rotation` turn.
///
/// Provisional: the native first-person sleeping view is unmeasured.
#[must_use]
pub fn sleeping_view_rotation(sleep_rotation: f32) -> Quat {
    let yaw = 270.0 - sleep_rotation;
    Quat::from_euler(
        bevy::math::EulerRot::YXZ,
        (180.0 - yaw).to_radians(),
        0.0,
        0.0,
    )
}

/// Publishes presentation from the supplied committed world and completed physics observations.
pub fn resolve_camera_pose(
    client_world: crate::observations::WorldObservation<'_>,
    collisions: &dyn crate::observations::CollisionLookup,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    mut published: ResMut<CameraPose>,
    publication: CameraPublicationContext,
    mut camera_transform: Single<(Entity, &mut Transform), With<FlyCamera>>,
) {
    let CameraPublicationContext {
        clock,
        physics,
        mut receipt,
    } = publication;
    let perspective = settings.perspective();
    let rotation = settings
        .death_rotation()
        .unwrap_or_else(|| view.camera_rotation());
    let transform = if matches!(
        settings.detached_view(),
        Some(crate::camera::DetachedView::Fly { .. })
    ) {
        Transform::from_translation(view.detached_eye()).with_rotation(view.camera_rotation())
    } else if let Some(stream) = client_world.stream.as_ref() {
        let collision_world = sim::PaletteWorld::new(
            stream.collision_store(),
            collisions.registry(stream.network_id_mode()),
            stream.current_dimension(),
            Default::default(),
        );
        match settings.rig() {
            Some(rig) => collision_safe_rig_pose(
                view.eye_translation(),
                view.camera_rotation(),
                rig,
                &collision_world,
            ),
            None => collision_safe_perspective_pose(
                view.eye_translation(),
                rotation,
                perspective,
                &collision_world,
            ),
        }
    } else {
        unavailable_world_perspective_pose(view.eye_translation(), rotation, perspective)
    };
    *camera_transform.1 = transform;
    *published = CameraPose::new(transform);
    if let Some(stream) = client_world.stream.as_ref()
        && let (Some(state), Some(world)) = (physics.state(), physics.last_world_identity())
    {
        receipt.prepare(
            crate::local_player_camera_receipt::CameraOwner::current(
                stream,
                clock.session_generation(),
            ),
            camera_transform.0,
            transform,
            perspective,
            PhysicsTick::new(state.tick),
            world.clone(),
        );
    }
}

/// Publishes presentation from the supplied committed world and completed physics observations.
pub fn publish_local_player_frame(
    client_world: crate::observations::WorldObservation<'_>,
    publication: CameraPublicationContext,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    camera: Res<CameraPose>,
    mut carrier: ResMut<LocalPlayerFrameCarrier>,
) {
    let CameraPublicationContext {
        clock,
        physics: local_physics,
        mut receipt,
    } = publication;
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
    let Some((camera_proof, world_collision_identity)) = receipt.take_prepared(
        crate::local_player_camera_receipt::CameraOwner::current(
            stream,
            clock.session_generation(),
        ),
        *camera.transform(),
        settings.perspective(),
        PhysicsTick::new(state.tick),
        world_collision_identity,
    ) else {
        carrier.reset(LocalPlayerFrameReset::Correction);
        return;
    };
    let sample = LocalPlayerFrameSample {
        session_generation: clock.session_generation(),
        actor_session_id: stream.authority().actor_session_id(),
        fifo_sequence: FifoSequence::new(stream.committed_sequence()),
        physics_tick: PhysicsTick::new(state.tick),
        perspective: settings.perspective(),
        world_collision_identity,
        pose: *camera.transform(),
        eye: view.eye_translation(),
        feet: view.feet_translation(),
        rotation: view.rotation(),
    };
    if carrier.publish(sample).is_err() {
        carrier.reset(LocalPlayerFrameReset::Correction);
    } else if let Some(frame) = carrier.snapshot() {
        receipt.finish(camera_proof, frame.pose_generation());
    }
}

#[cfg(test)]
mod sleeping_tests {
    use bevy::math::Vec3;

    use super::{sleeping_subject_position, sleeping_view_rotation};

    #[test]
    fn sleepers_lie_on_the_bed_top_and_look_toward_its_foot() {
        let (eye, feet) = sleeping_subject_position([2, 70, -5]);
        assert_eq!(feet, Vec3::new(2.5, 70.5625, -4.5));
        assert!((eye.y - feet.y - client_world::SLEEPING_PLAYER_NETWORK_OFFSET).abs() < 1e-4);
        // A south-facing bed (sleep rotation 90) puts the head south, so the view faces north.
        let forward = sleeping_view_rotation(90.0) * Vec3::NEG_Z;
        assert!(forward.abs_diff_eq(Vec3::NEG_Z, 1e-5), "{forward}");
        let forward = sleeping_view_rotation(0.0) * Vec3::NEG_Z;
        assert!(
            forward.abs_diff_eq(Vec3::X, 1e-5),
            "west-facing bed looks east: {forward}"
        );
    }
}
