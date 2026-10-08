//! Presentation-only application of server camera instructions, presets, fades, FOV overrides and shakes.
//! Preset capabilities are resolved once; frame evaluation borrows prepared state.

use std::sync::Arc;

use bevy::prelude::{EulerRot, Quat, Resource, Transform, Vec2, Vec3};
use view_presentation::camera::bedrock_camera_rotation as bedrock_rotation;
use protocol::{
    CameraAimAssistPresetSettings, CameraEvent, CameraFadeInstruction, CameraFovInstruction,
    CameraInstructionEvent, CameraPreset, CameraSetInstruction, CameraShakeAction,
    CameraShakeEvent, CameraShakeType, CameraSpline,
};

use super::{
    fade::FadeState,
    presets::{PresetKind, PresetOverrides, ResolvedPreset, preset_kind_from_name},
    spline::SplinePlayback,
    target::{TargetFocus, TargetSettings},
};

use super::super::{
    easing::{ease, kind_from_name},
    shake::{ShakeKind, ShakeOffset, ShakeState},
};

const MAX_EASE_SECONDS: f32 = 3600.0;

/// One actor's placement as the camera needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorView {
    pub position: Vec3,
    pub yaw_degrees: f32,
    pub pitch_degrees: f32,
}

/// Everything a camera evaluation reads besides its own state.
pub struct ViewContext<'a> {
    /// The unmodified resolved camera transform.
    pub base: Transform,
    /// The local player's eye pose, the anchor for orbit presets.
    pub subject: Transform,
    /// Horizontal FOV setting in degrees.
    pub base_fov: f32,
    pub actors: &'a dyn Fn(i64) -> Option<ActorView>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Fixed(Pose),
    Orbit {
        front: bool,
        radius: f32,
        rotation: Option<Quat>,
        view_offset: Vec2,
        entity_offset: Vec3,
        fixed_pivot: bool,
    },
}

impl Target {
    /// Resolves the orbit around the entity-relative pivot, then adds horizontal and world-up view offsets.
    fn resolve(&self, subject: &Pose) -> Pose {
        match *self {
            Self::Fixed(pose) => pose,
            Self::Orbit {
                front,
                radius,
                rotation,
                view_offset,
                entity_offset,
                fixed_pivot,
            } => {
                let orbit_rotation = rotation.unwrap_or(subject.rotation);
                let pivot_rotation = if fixed_pivot {
                    orbit_rotation
                } else {
                    subject.rotation
                };
                let (yaw, _, _) = pivot_rotation.to_euler(EulerRot::YXZ);
                let offset_yaw = yaw
                    - if fixed_pivot {
                        std::f32::consts::PI * 1.5
                    } else {
                        std::f32::consts::PI
                    };
                let pivot = subject.translation + Quat::from_rotation_y(offset_yaw) * entity_offset;
                let forward = orbit_rotation * Vec3::NEG_Z;
                let (translation, rotation) = if front {
                    let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
                    let flat = if flat == Vec3::ZERO {
                        Vec3::NEG_Z
                    } else {
                        flat
                    };
                    let translation = pivot + flat * radius;
                    (
                        translation,
                        look_rotation(translation, pivot).unwrap_or(subject.rotation),
                    )
                } else {
                    (pivot - forward * radius, orbit_rotation)
                };
                let direction = rotation * Vec3::NEG_Z;
                let right = direction.cross(Vec3::Y).normalize_or_zero();
                Pose {
                    translation: translation + right * view_offset.x + Vec3::Y * view_offset.y,
                    rotation,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Pose {
    pub(super) translation: Vec3,
    pub(super) rotation: Quat,
}

impl Pose {
    /// Keeps only the translation and rotation that camera blends own.
    fn from_transform(transform: &Transform) -> Self {
        Self {
            translation: transform.translation,
            rotation: transform.rotation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PoseBlend {
    from: Pose,
    to: Target,
    elapsed: f32,
    duration: f32,
    kind: u8,
}

impl PoseBlend {
    /// Samples a pose transition against its current moving orbit anchor.
    fn current(&self, subject: &Pose, animated: Option<Pose>) -> Pose {
        let progress = if self.duration > 0.0 {
            ease(self.kind, self.elapsed / self.duration)
        } else {
            1.0
        };
        let to = animated.unwrap_or_else(|| self.to.resolve(subject));
        Pose {
            translation: self.from.translation.lerp(to.translation, progress),
            rotation: self.from.rotation.slerp(to.rotation, progress).normalize(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FovBlend {
    from_degrees: f32,
    to_degrees: f32,
    returning: bool,
    elapsed: f32,
    duration: f32,
    kind: u8,
}

/// Counters for well-formed instructions this client cannot apply.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ServerCameraSkips {
    pub unresolved_presets: u64,
    /// Actor-bound instructions whose actor was not loaded when evaluated.
    pub actor_bound: u64,
    pub unknown_shake: u64,
    pub legacy_switch: u64,
    pub invalid_splines: u64,
    pub invalid_fades: u64,
    pub invalid_options: u64,
}

/// Live server-driven camera state.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct ServerCameraView {
    presets: Arc<[CameraPreset]>,
    splines: Arc<[CameraSpline]>,
    overrides: Vec<PresetOverrides>,
    spline: Option<SplinePlayback>,
    active_aim_assist: Option<CameraAimAssistPresetSettings>,
    active_base_preset_name: Option<Arc<str>>,
    active_preset_name: Option<Arc<str>>,
    active_control_scheme: Option<u8>,
    active_preset_index: Option<usize>,
    attached: Option<i64>,
    active_listener: Option<u8>,
    active_player_effects: Option<bool>,
    yaw_limits: Option<[f32; 2]>,
    pose: Option<PoseBlend>,
    fov: Option<FovBlend>,
    fade: FadeState,
    shake: ShakeState,
    skips: ServerCameraSkips,
    last_sequence: u64,
    seen_resets: u64,
}

impl ServerCameraView {
    #[must_use]
    pub const fn skips(&self) -> ServerCameraSkips {
        self.skips
    }

    #[must_use]
    pub const fn last_sequence(&self) -> u64 {
        self.last_sequence
    }

    /// Restarts sequence tracking and drops state when the retained queue was reset upstream.
    pub fn observe_resets(&mut self, resets: u64) {
        if resets != self.seen_resets {
            self.seen_resets = resets;
            self.clear();
        }
    }

    /// Restores the player camera while keeping server registries and diagnostic counters.
    pub fn clear(&mut self) {
        *self = Self {
            presets: Arc::clone(&self.presets),
            splines: Arc::clone(&self.splines),
            overrides: vec![PresetOverrides::default(); self.presets.len()],
            skips: self.skips,
            seen_resets: self.seen_resets,
            ..Self::default()
        };
        self.prepare_presets();
    }

    #[must_use]
    pub fn shake_offset(&self) -> ShakeOffset {
        self.shake.offset()
    }

    #[must_use]
    pub fn has_pose_override(&self) -> bool {
        self.pose.is_some() || self.attached.is_some()
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.has_pose_override() || self.shake.is_active()
    }

    /// Server-authored camera pose, if any: an explicit or preset pose, or an entity attachment.
    #[must_use]
    pub fn pose_override(&self, context: &ViewContext<'_>) -> Option<Transform> {
        let attached = self.attached.and_then(|id| (context.actors)(id));
        let subject = attached.map_or_else(
            || Pose::from_transform(&context.subject),
            |actor| Pose {
                translation: actor.position,
                rotation: bedrock_rotation(actor.yaw_degrees, actor.pitch_degrees),
            },
        );
        let animated = self.spline.as_ref().filter(|spline| {
            !spline.is_finished()
                && self.active_base_preset_name.as_deref() == Some("minecraft:free")
        });
        let mut pose = match (self.pose, attached) {
            (Some(blend), _) => blend.current(&subject, animated.map(SplinePlayback::sample)),
            (None, Some(_)) => subject,
            (None, None) => return None,
        };
        if let Some(focus) = self.active_focus() {
            pose.rotation = focus.sample(pose.translation, (context.actors)(focus.actor));
        }
        Some(Transform {
            translation: pose.translation,
            rotation: pose.rotation,
            ..Transform::IDENTITY
        })
    }

    /// Shortens orbit booms against world geometry without affecting stationary cameras.
    pub fn collision_safe_pose(
        &self,
        context: &ViewContext<'_>,
        pose: Transform,
        world: &impl sim::CollisionWorld,
    ) -> Transform {
        let Some(PoseBlend {
            to:
                Target::Orbit {
                    radius,
                    front,
                    rotation,
                    entity_offset,
                    view_offset,
                    fixed_pivot,
                    ..
                },
            ..
        }) = self.pose
        else {
            return pose;
        };
        if radius <= 0.0 {
            return pose;
        }
        let subject =
            self.attached
                .and_then(|id| (context.actors)(id))
                .map_or(context.subject, |actor| Transform {
                    translation: actor.position,
                    rotation: bedrock_rotation(actor.yaw_degrees, actor.pitch_degrees),
                    ..Transform::IDENTITY
                });
        let pivot_rotation = if fixed_pivot {
            rotation.unwrap_or(subject.rotation)
        } else {
            subject.rotation
        };
        let (yaw, _, _) = pivot_rotation.to_euler(EulerRot::YXZ);
        let offset_yaw = yaw
            - if fixed_pivot {
                std::f32::consts::PI * 1.5
            } else {
                std::f32::consts::PI
            };
        let direction =
            rotation.unwrap_or(subject.rotation) * if front { Vec3::Z } else { Vec3::NEG_Z };
        let right = direction.cross(Vec3::Y).normalize_or_zero();
        let pivot = subject.translation
            + Quat::from_rotation_y(offset_yaw) * entity_offset
            + right * view_offset.x
            + Vec3::Y * view_offset.y;
        super::super::rig::sweep_boom(pivot, pose, world)
    }

    /// Fade overlay as `(rgb, alpha)` while a fade is running.
    #[must_use]
    pub fn fade_overlay(&self) -> Option<([f32; 3], f32)> {
        self.fade.overlay()
    }

    /// FOV override in the same degrees as the FOV setting, blended toward `base_degrees` on release.
    #[must_use]
    pub fn fov_override_degrees(&self, base_degrees: f32) -> Option<f32> {
        let blend = self.fov?;
        let progress = if blend.duration > 0.0 {
            ease(blend.kind, (blend.elapsed / blend.duration).min(1.0))
        } else {
            1.0
        };
        let target = if blend.returning {
            base_degrees
        } else {
            blend.to_degrees
        };
        Some(blend.from_degrees + (target - blend.from_degrees) * progress)
    }

    /// Advances active animations without allocating or rebuilding their prepared data.
    pub fn advance(&mut self, delta_seconds: f32) {
        if !(delta_seconds.is_finite() && delta_seconds > 0.0) {
            return;
        }
        if let Some(spline) = &mut self.spline {
            spline.advance(delta_seconds);
        }
        if let Some(pose) = &mut self.pose {
            pose.elapsed = (pose.elapsed + delta_seconds).min(pose.duration.max(0.0));
        }
        if let Some(fov) = &mut self.fov {
            fov.elapsed += delta_seconds;
            if fov.returning && fov.elapsed >= fov.duration {
                self.fov = None;
            }
        }
        self.fade.advance(delta_seconds);
        self.shake.advance(delta_seconds);
    }

    /// Applies one committed event against the camera's current unmodified pose and FOV setting.
    pub fn apply(&mut self, sequence: u64, event: &CameraEvent, context: &ViewContext<'_>) {
        sim::minecraft_sin(0.0);
        self.last_sequence = self.last_sequence.max(sequence);
        match event {
            CameraEvent::Presets(presets) => {
                self.presets = Arc::clone(presets);
                self.prepare_presets();
            }
            CameraEvent::Splines(splines) => self.splines = Arc::clone(splines),
            CameraEvent::AimAssist(_)
            | CameraEvent::AimAssistPresets(_)
            | CameraEvent::AimAssistActorPriority(_) => {}
            CameraEvent::Switch(_) => self.skips.legacy_switch += 1,
            CameraEvent::Shake(shake) => self.apply_shake(shake),
            CameraEvent::Instruction(instruction) => self.apply_instruction(instruction, context),
        }
    }

    /// Routes shake commands to the independently tracked positional and rotational queues.
    fn apply_shake(&mut self, shake: &CameraShakeEvent) {
        match shake.action {
            CameraShakeAction::Stop => self.shake.stop_all(),
            CameraShakeAction::Add => {
                let kind = match shake.shake_type {
                    CameraShakeType::Positional => ShakeKind::Positional,
                    CameraShakeType::Rotational => ShakeKind::Rotational,
                    CameraShakeType::Unknown(_) => {
                        self.skips.unknown_shake += 1;
                        return;
                    }
                };
                if !self
                    .shake
                    .add(kind, shake.intensity, shake.duration_seconds)
                {
                    self.skips.unknown_shake += 1;
                }
            }
            CameraShakeAction::Unknown(_) => self.skips.unknown_shake += 1,
        }
    }

    /// Applies independent packet options in their camera-state order.
    fn apply_instruction(
        &mut self,
        instruction: &CameraInstructionEvent,
        context: &ViewContext<'_>,
    ) {
        if let Some(set) = &instruction.set {
            self.apply_set(set, context);
        }
        if let Some(target) = &instruction.target {
            self.apply_target(target, context);
        }
        if instruction.remove_target {
            self.remove_target(context);
        }
        if instruction.clear == Some(true) {
            self.remove_target(context);
            self.active_aim_assist = None;
            self.active_base_preset_name = None;
            self.active_preset_name = None;
            self.active_control_scheme = None;
            self.active_listener = None;
            self.active_player_effects = None;
            self.yaw_limits = None;
            self.active_preset_index = None;
            self.pose = None;
            self.attached = None;
            self.fov = None;
        }
        if let Some(fade) = &instruction.fade {
            self.apply_fade(fade);
        }
        if let Some(fov) = &instruction.fov {
            self.apply_fov(fov, context.base_fov);
        }
        if let Some(instruction) = &instruction.spline {
            let definition = if instruction.load_from_json {
                self.splines
                    .iter()
                    .find(|spline| spline.name == instruction.spline.name)
            } else {
                Some(&instruction.spline)
            };
            match definition.and_then(SplinePlayback::new) {
                Some(spline) => self.spline = Some(spline),
                None => self.skips.invalid_splines += 1,
            }
        }
        if let Some(id) = instruction.attach_to_entity {
            self.attached = Some(id);
            if (context.actors)(id).is_none() {
                self.skips.actor_bound += 1;
            }
        }
        if instruction.detach_from_entity {
            self.attached = None;
        }
    }

    /// Captures the rendered pose so interrupted transitions start without a jump.
    fn current_pose(&self, context: &ViewContext<'_>) -> Pose {
        self.pose_override(context).map_or_else(
            || Pose::from_transform(&context.base),
            |t| Pose::from_transform(&t),
        )
    }

    /// Follows `inherit_from` names through the registry, returning the first value each field
    /// declares and the vanilla base kind reached, if any.
    fn resolve_preset(&self, id: u32) -> Option<ResolvedPreset> {
        let mut index = usize::try_from(id).ok()?;
        let mut visited = [false; protocol::MAX_CAMERA_PRESETS];
        let selected = self.presets.get(index)?;
        let native = preset_kind_from_name(&selected.name).is_some();
        let mut resolved = ResolvedPreset {
            starting_rotation: (!native).then_some(selected.starting_rotation).flatten(),
            use_starting_rotation: native || selected.apply_inherited_starting_rotation,
            ..Default::default()
        };
        loop {
            if *visited.get(index)? {
                return None;
            }
            visited[index] = true;
            let preset = self.presets.get(index)?;
            resolved.absorb(preset);
            if native {
                resolved.radius = None;
                resolved.yaw_limit_min = None;
                resolved.yaw_limit_max = None;
            }
            if preset.inherit_from.is_empty() {
                resolved.kind = preset_kind_from_name(&preset.name);
                resolved.base_name = Some(Arc::clone(&preset.name));
                return resolved.kind.map(|_| resolved);
            }
            if let Some(parent) = self
                .presets
                .iter()
                .position(|candidate| candidate.name == preset.inherit_from)
            {
                index = parent;
            } else {
                resolved.kind = preset_kind_from_name(&preset.inherit_from);
                resolved.base_name = Some(Arc::clone(&preset.inherit_from));
                return resolved.kind.map(|_| resolved);
            }
        }
    }

    /// Native free and orbit rigs hide first-person hands regardless of the local perspective option.
    pub fn renders_first_person(&self, fallback: bool) -> bool {
        self.active_base_preset_name
            .as_deref()
            .map_or(fallback, |name| name == "minecraft:first_person")
    }

    /// Free cameras retain the FOV option without gameplay-driven zoom or widening.
    pub fn gameplay_fov_enabled(&self) -> bool {
        self.active_base_preset_name.as_deref() != Some("minecraft:free")
    }

    /// Stationary free cameras omit the portal-distortion component independently of player effects.
    pub fn portal_distortion_enabled(&self) -> bool {
        self.active_base_preset_name.as_deref() != Some("minecraft:free")
    }

    /// Follow-orbit consumes routed yaw/pitch deltas; fixed-boom retains its instructed direction.
    pub fn apply_look_delta(&mut self, delta_radians: Vec2) -> Option<Quat> {
        if self.active_base_preset_name.as_deref() != Some("minecraft:follow_orbit")
            || !delta_radians.is_finite()
        {
            return None;
        }
        let Target::Orbit {
            rotation: Some(rotation),
            ..
        } = &mut self.pose.as_mut()?.to
        else {
            return None;
        };
        let (yaw, pitch, _) = rotation.to_euler(EulerRot::YXZ);
        let mut yaw = yaw + delta_radians.x;
        if let Some([low, high]) = self.yaw_limits {
            let native_yaw = (std::f32::consts::PI - yaw + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            yaw = std::f32::consts::PI - native_yaw.clamp(low, high);
        }
        *rotation = Quat::from_euler(
            EulerRot::YXZ,
            yaw,
            (pitch + delta_radians.y).clamp(-super::super::PITCH_LIMIT, super::super::PITCH_LIMIT),
            0.0,
        );
        self.overrides
            .get_mut(self.active_preset_index?)?
            .orbit_rotation = Some(*rotation);
        Some(*rotation)
    }

    /// Returns the inherited listener selector; an omitted selector follows the camera.
    pub const fn active_listener(&self) -> Option<u8> {
        self.active_listener
    }

    /// A preset can add player-state effects to a base camera that lacks them.
    pub fn player_effects_enabled(&self) -> bool {
        self.active_player_effects == Some(true)
            || self.active_base_preset_name.as_deref() != Some("minecraft:free")
    }

    /// Returns the resolved aim-assist settings for the currently selected camera preset.
    pub fn active_aim_assist(&self) -> Option<&CameraAimAssistPresetSettings> {
        self.active_aim_assist.as_ref()
    }

    /// Returns the selected camera preset identifier sent in activation acknowledgements.
    pub fn active_preset_name(&self) -> Option<&str> {
        self.active_preset_name.as_deref()
    }

    /// Identifies the vanilla base preset independently of its inherited custom name.
    pub fn active_base_preset_name(&self) -> Option<&str> {
        self.active_base_preset_name.as_deref()
    }

    /// Returns the inherited control scheme selected by the camera preset.
    pub const fn active_control_scheme(&self) -> Option<u8> {
        self.active_control_scheme
    }

    /// Resolves inherited preset defaults, then applies explicit instruction values.
    fn apply_set(&mut self, set: &CameraSetInstruction, context: &ViewContext<'_>) {
        let Some(preset) = self.resolve_preset(set.preset_id) else {
            self.skips.unresolved_presets += 1;
            return;
        };
        let from = self.current_pose(context);
        let activating = self.active_preset_index != Some(set.preset_id as usize);
        let kind = preset.kind;
        self.active_aim_assist = preset.aim_assist.clone();
        self.active_base_preset_name = preset.base_name.clone();
        self.active_preset_name = Some(Arc::clone(&self.presets[set.preset_id as usize].name));
        self.active_control_scheme = preset.control_scheme;
        self.active_listener = preset.listener;
        self.active_player_effects = preset.player_effects;
        self.yaw_limits = if preset.yaw_limit_min.is_some() || preset.yaw_limit_max.is_some() {
            let low = preset
                .yaw_limit_min
                .map_or(-std::f32::consts::PI, f32::to_radians);
            let high = preset
                .yaw_limit_max
                .map_or(std::f32::consts::PI, f32::to_radians);
            if low <= high {
                Some([low, high])
            } else {
                self.skips.invalid_options += 1;
                None
            }
        } else {
            None
        };
        if preset.listener.is_some_and(|value| value > 1) {
            self.skips.invalid_options += 1;
            self.active_listener = None;
        }
        self.active_preset_index = Some(set.preset_id as usize);
        let has_offsets = kind != Some(PresetKind::Free);
        let mut overrides = self.overrides[set.preset_id as usize];
        overrides.position = set.position.or(overrides.position);
        overrides.rotation = set
            .rotation_degrees
            .map(|[pitch, yaw]| bedrock_rotation(yaw, pitch))
            .or(overrides.rotation);
        if has_offsets {
            overrides.view_offset = set.view_offset.or(overrides.view_offset);
            overrides.entity_offset = set.entity_offset.or(overrides.entity_offset);
        } else {
            self.skips.invalid_options +=
                u64::from(set.view_offset.is_some()) + u64::from(set.entity_offset.is_some());
        }
        let translation = Vec3::new(
            axis(
                overrides.position.map(|p| p[0]),
                preset.position[0],
                from.translation.x,
            ),
            axis(
                overrides.position.map(|p| p[1]),
                preset.position[1],
                from.translation.y,
            ),
            axis(
                overrides.position.map(|p| p[2]),
                preset.position[2],
                from.translation.z,
            ),
        );
        if let Some(target) = set.facing_position {
            overrides.rotation = Some(facing_rotation(
                translation,
                Vec3::from_array(target),
                overrides.rotation.unwrap_or(from.rotation),
            ));
        }
        if set.default_preset == Some(true) {
            overrides.position = None;
            overrides.rotation = None;
            if set.remove_ignore_starting_values {
                overrides.view_offset = None;
                overrides.entity_offset = None;
            }
        }
        let boom = matches!(
            preset.base_name.as_deref(),
            Some("minecraft:follow_orbit" | "minecraft:fixed_boom")
        );
        if boom {
            let reset = set.default_preset == Some(true) && set.remove_ignore_starting_values;
            let first = overrides.orbit_rotation.is_none();
            if activating && preset.base_name.as_deref() == Some("minecraft:follow_orbit") {
                overrides.orbit_rotation = Some(context.subject.rotation);
            } else if first {
                overrides.orbit_rotation = Some(bedrock_rotation(0.0, 0.0));
            }
            if reset
                || first && (preset.use_starting_rotation || preset.starting_rotation.is_some())
            {
                let [pitch, yaw] = preset.starting_rotation.unwrap_or([45.0, 45.0]);
                overrides.orbit_rotation = Some(bedrock_rotation(yaw, pitch));
            }
            if set.default_preset != Some(true)
                && (set.rotation_degrees.is_some() || set.facing_position.is_some())
            {
                overrides.orbit_rotation = overrides.rotation;
            }
        }
        self.overrides[set.preset_id as usize] = overrides;
        let (kind_ease, duration) = set.ease.map_or((0, 0.0), |ease| {
            (ease.kind, ease.time_seconds.clamp(0.0, MAX_EASE_SECONDS))
        });
        let to = match kind {
            Some(
                PresetKind::FirstPerson | PresetKind::ThirdPerson | PresetKind::ThirdPersonFront,
            ) => {
                let radius = if kind == Some(PresetKind::FirstPerson) {
                    0.0
                } else {
                    preset.radius.filter(|radius| *radius > 0.0).unwrap_or(
                        match preset.base_name.as_deref() {
                            Some("minecraft:follow_orbit" | "minecraft:fixed_boom") => 10.0,
                            _ => super::super::THIRD_PERSON_RADIUS_BLOCKS,
                        },
                    )
                };
                Target::Orbit {
                    front: kind == Some(PresetKind::ThirdPersonFront),
                    fixed_pivot: preset.base_name.as_deref() == Some("minecraft:fixed_boom"),
                    radius,
                    rotation: boom.then_some(overrides.orbit_rotation).flatten(),
                    view_offset: overrides
                        .view_offset
                        .or(preset.view_offset)
                        .map_or(Vec2::ZERO, Vec2::from_array),
                    entity_offset: overrides
                        .entity_offset
                        .or(preset.entity_offset)
                        .map_or(Vec3::ZERO, |[x, y, z]| Vec3::new(-x, y, z)),
                }
            }
            _ => {
                let translation = Vec3::new(
                    axis(
                        overrides.position.map(|p| p[0]),
                        preset.position[0],
                        from.translation.x,
                    ),
                    axis(
                        overrides.position.map(|p| p[1]),
                        preset.position[1],
                        from.translation.y,
                    ),
                    axis(
                        overrides.position.map(|p| p[2]),
                        preset.position[2],
                        from.translation.z,
                    ),
                );
                let rotation = overrides.rotation.unwrap_or_else(|| {
                    let (from_yaw, from_pitch, _) = from.rotation.to_euler(EulerRot::YXZ);
                    let [pitch, yaw] = preset.rotation_degrees;
                    Quat::from_euler(
                        EulerRot::YXZ,
                        yaw.map_or(from_yaw, |yaw| (180.0 - yaw).to_radians()),
                        pitch.map_or(from_pitch, |pitch| -pitch.to_radians()),
                        0.0,
                    )
                });
                Target::Fixed(Pose {
                    translation,
                    rotation,
                })
            }
        };
        if let Target::Fixed(pose) = to {
            self.overrides[set.preset_id as usize].stationary = Some(pose);
        }
        self.pose = Some(PoseBlend {
            from,
            to,
            elapsed: 0.0,
            duration,
            kind: kind_ease,
        });
    }

    /// Adds a fade without changing an active animation's color.
    fn apply_fade(&mut self, fade: &CameraFadeInstruction) {
        if !self.fade.apply(fade) {
            self.skips.invalid_fades += 1;
        }
    }

    /// Blends FOV independently from pose and restores the local setting on clear.
    fn apply_fov(&mut self, fov: &CameraFovInstruction, base_fov: f32) {
        let from = self.fov_override_degrees(base_fov).unwrap_or(base_fov);
        let duration = if fov.ease_time_seconds.is_finite() {
            fov.ease_time_seconds.clamp(0.0, MAX_EASE_SECONDS)
        } else {
            0.0
        };
        let returning = fov.clear || !fov.degrees.is_finite();
        if returning && duration <= 0.0 {
            self.fov = None;
            return;
        }
        self.fov = Some(FovBlend {
            from_degrees: from,
            to_degrees: if returning {
                base_fov
            } else {
                fov.degrees.clamp(30.0, 110.0)
            },
            returning,
            elapsed: 0.0,
            duration,
            kind: kind_from_name(&fov.ease_type),
        });
    }
}

/// Gives an instruction component precedence over its preset and current pose.
fn axis(explicit: Option<f32>, preset: Option<f32>, current: f32) -> f32 {
    explicit.or(preset).unwrap_or(current)
}

/// Keeps yaw at a vertical target and both axes at a coincident target.
pub(super) fn facing_rotation(from: Vec3, target: Vec3, previous: Quat) -> Quat {
    let delta = target - from;
    if delta.length_squared() < 0.0001 {
        return previous;
    }
    let horizontal = delta.x.hypot(delta.z);
    let (old_yaw, old_pitch, _) = previous.to_euler(EulerRot::YXZ);
    let yaw = if horizontal >= 0.01 {
        std::f32::consts::PI + delta.x.atan2(delta.z)
    } else {
        old_yaw
    };
    let pitch = if delta.length_squared() >= 0.0001 {
        delta.y.atan2(horizontal)
    } else {
        old_pitch
    };
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0)
}

/// Keeps the existing rotation when a facing target coincides with the camera.
fn look_rotation(from: Vec3, target: Vec3) -> Option<Quat> {
    (from.distance_squared(target) > f32::EPSILON).then(|| {
        Transform::from_translation(from)
            .looking_at(target, Vec3::Y)
            .rotation
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "options_tests.rs"]
mod options_tests;

mod targeting;
