//! Presentation-only application of server camera instructions, presets, fades, FOV overrides and shakes.
//! Orbit presets follow the player without collision; preset kinds are matched by vanilla name.

use std::sync::Arc;

use bevy::prelude::{EulerRot, Quat, Resource, Transform, Vec3};
use protocol::{
    CameraEvent, CameraFadeInstruction, CameraFovInstruction, CameraInstructionEvent, CameraPreset,
    CameraSetInstruction, CameraShakeAction, CameraShakeEvent, CameraShakeType,
};

use super::{
    easing::{ease, kind_from_name},
    shake::{ShakeKind, ShakeOffset, ShakeState},
};

const MAX_EASE_SECONDS: f32 = 3600.0;
const MAX_INHERIT_DEPTH: usize = 8;
const DEFAULT_ORBIT_RADIUS: f32 = 4.0;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresetKind {
    Free,
    FirstPerson,
    ThirdPerson,
    ThirdPersonFront,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Fixed(Pose),
    Orbit { front: bool, radius: f32 },
}

impl Target {
    fn resolve(&self, subject: &Pose) -> Pose {
        match *self {
            Self::Fixed(pose) => pose,
            Self::Orbit {
                front: false,
                radius,
            } => Pose {
                translation: subject.translation - subject.rotation * Vec3::NEG_Z * radius,
                rotation: subject.rotation,
            },
            Self::Orbit {
                front: true,
                radius,
            } => {
                let forward = subject.rotation * Vec3::NEG_Z;
                let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
                let flat = if flat == Vec3::ZERO {
                    Vec3::NEG_Z
                } else {
                    flat
                };
                let translation = subject.translation + flat * radius;
                Pose {
                    translation,
                    rotation: look_rotation(translation, subject.translation)
                        .unwrap_or(subject.rotation),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Pose {
    translation: Vec3,
    rotation: Quat,
}

impl Pose {
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
    fn current(&self, subject: &Pose) -> Pose {
        let progress = if self.duration > 0.0 {
            ease(self.kind, self.elapsed / self.duration)
        } else {
            1.0
        };
        let to = self.to.resolve(subject);
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct FadeState {
    fade_in: f32,
    hold: f32,
    fade_out: f32,
    color: [f32; 3],
    elapsed: f32,
}

impl FadeState {
    fn total(&self) -> f32 {
        self.fade_in + self.hold + self.fade_out
    }

    fn alpha(&self) -> f32 {
        let t = self.elapsed;
        let alpha = if t < self.fade_in {
            t / self.fade_in
        } else if t < self.fade_in + self.hold {
            1.0
        } else if self.fade_out > 0.0 {
            1.0 - (t - self.fade_in - self.hold) / self.fade_out
        } else {
            0.0
        };
        alpha.clamp(0.0, 1.0)
    }
}

/// Counters for well-formed instructions this client cannot apply.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ServerCameraSkips {
    pub unresolved_presets: u64,
    /// Actor-bound instructions whose actor was not loaded when evaluated.
    pub actor_bound: u64,
    pub unknown_shake: u64,
    pub legacy_switch: u64,
}

/// Live server-driven camera state.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct ServerCameraView {
    presets: Arc<[CameraPreset]>,
    attached: Option<i64>,
    focus: Option<(i64, Vec3)>,
    pose: Option<PoseBlend>,
    fov: Option<FovBlend>,
    fade: Option<FadeState>,
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

    pub fn clear(&mut self) {
        *self = Self {
            presets: Arc::clone(&self.presets),
            skips: self.skips,
            seen_resets: self.seen_resets,
            ..Self::default()
        };
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
        let mut pose = match (self.pose, attached) {
            (Some(blend), _) => blend.current(&subject),
            (None, Some(_)) => subject,
            (None, None) => return None,
        };
        if let Some((id, offset)) = self.focus
            && let Some(actor) = (context.actors)(id)
            && let Some(rotation) = look_rotation(pose.translation, actor.position + offset)
        {
            pose.rotation = rotation;
        }
        Some(Transform {
            translation: pose.translation,
            rotation: pose.rotation,
            ..Transform::IDENTITY
        })
    }

    /// Fade overlay as `(rgb, alpha)` while a fade is running.
    #[must_use]
    pub fn fade_overlay(&self) -> Option<([f32; 3], f32)> {
        self.fade.map(|fade| (fade.color, fade.alpha()))
    }

    /// FOV override in the same degrees as the FOV setting, blended toward `base_degrees` on release.
    #[must_use]
    pub fn fov_override_degrees(&self, base_degrees: f32) -> Option<f32> {
        let blend = self.fov?;
        let progress = if blend.duration > 0.0 {
            ease(blend.kind, blend.elapsed / blend.duration)
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

    pub fn advance(&mut self, delta_seconds: f32) {
        if !(delta_seconds.is_finite() && delta_seconds > 0.0) {
            return;
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
        if let Some(fade) = &mut self.fade {
            fade.elapsed += delta_seconds;
            if fade.elapsed >= fade.total() {
                self.fade = None;
            }
        }
        self.shake.advance(delta_seconds);
    }

    /// Applies one committed event against the camera's current unmodified pose and FOV setting.
    pub fn apply(&mut self, sequence: u64, event: &CameraEvent, context: &ViewContext<'_>) {
        self.last_sequence = self.last_sequence.max(sequence);
        match event {
            CameraEvent::Presets(presets) => self.presets = Arc::clone(presets),
            CameraEvent::Switch(_) => self.skips.legacy_switch += 1,
            CameraEvent::Shake(shake) => self.apply_shake(shake),
            CameraEvent::Instruction(instruction) => self.apply_instruction(instruction, context),
        }
    }

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

    fn apply_instruction(
        &mut self,
        instruction: &CameraInstructionEvent,
        context: &ViewContext<'_>,
    ) {
        if instruction.clear == Some(true) {
            self.pose = None;
            self.attached = None;
            self.focus = None;
        }
        if instruction.detach_from_entity {
            self.attached = None;
        }
        if instruction.remove_target {
            self.focus = None;
        }
        if let Some(id) = instruction.attach_to_entity {
            self.attached = Some(id);
            if (context.actors)(id).is_none() {
                self.skips.actor_bound += 1;
            }
        }
        if let Some(target) = &instruction.target {
            let offset = target.center_offset.map_or(Vec3::ZERO, Vec3::from_array);
            self.focus = Some((target.actor_unique_id, offset));
            if (context.actors)(target.actor_unique_id).is_none() {
                self.skips.actor_bound += 1;
            }
        }
        if let Some(set) = &instruction.set {
            self.apply_set(set, context);
        }
        if let Some(fade) = &instruction.fade {
            self.apply_fade(fade);
        }
        if let Some(fov) = &instruction.fov {
            self.apply_fov(fov, context.base_fov);
        }
    }

    fn current_pose(&self, context: &ViewContext<'_>) -> Pose {
        self.pose_override(context).map_or_else(
            || Pose::from_transform(&context.base),
            |t| Pose::from_transform(&t),
        )
    }

    /// Follows `inherit_from` names through the registry, returning the first value each field
    /// declares and the vanilla base kind reached, if any.
    fn resolve_preset(&self, id: u32) -> Option<ResolvedPreset> {
        let mut preset = self.presets.get(usize::try_from(id).ok()?)?;
        let mut resolved = ResolvedPreset::default();
        for _ in 0..MAX_INHERIT_DEPTH {
            resolved.absorb(preset);
            if let Some(kind) = preset_kind_from_name(&preset.name)
                .or_else(|| preset_kind_from_name(&preset.inherit_from))
            {
                resolved.kind = Some(kind);
                break;
            }
            match self
                .presets
                .iter()
                .find(|candidate| candidate.name == preset.inherit_from)
            {
                Some(parent) if !std::ptr::eq(parent, preset) => preset = parent,
                _ => break,
            }
        }
        Some(resolved)
    }

    fn apply_set(&mut self, set: &CameraSetInstruction, context: &ViewContext<'_>) {
        let preset = self.resolve_preset(set.preset_id);
        let kind = preset.as_ref().and_then(|preset| preset.kind);
        let explicit = set.position.is_some()
            || set.rotation_degrees.is_some()
            || set.facing_position.is_some();
        if kind == Some(PresetKind::FirstPerson) {
            self.pose = None;
            return;
        }
        if kind.is_none() && !explicit && preset.as_ref().is_none_or(|p| !p.has_pose()) {
            self.skips.unresolved_presets += 1;
            return;
        }
        let from = self.current_pose(context);
        let (kind_ease, duration) = set.ease.map_or((0, 0.0), |ease| {
            (ease.kind, ease.time_seconds.clamp(0.0, MAX_EASE_SECONDS))
        });
        let to = match kind {
            Some(PresetKind::ThirdPerson | PresetKind::ThirdPersonFront) => {
                let radius = preset
                    .as_ref()
                    .and_then(|preset| preset.radius)
                    .filter(|radius| *radius > 0.0)
                    .unwrap_or(DEFAULT_ORBIT_RADIUS);
                Target::Orbit {
                    front: kind == Some(PresetKind::ThirdPersonFront),
                    radius,
                }
            }
            _ => {
                let preset = preset.unwrap_or_default();
                let translation = Vec3::new(
                    axis(
                        set.position.map(|p| p[0]),
                        preset.position[0],
                        from.translation.x,
                    ),
                    axis(
                        set.position.map(|p| p[1]),
                        preset.position[1],
                        from.translation.y,
                    ),
                    axis(
                        set.position.map(|p| p[2]),
                        preset.position[2],
                        from.translation.z,
                    ),
                );
                let rotation = match (set.facing_position, set.rotation_degrees) {
                    (Some(target), _) => look_rotation(translation, Vec3::from_array(target))
                        .unwrap_or(from.rotation),
                    (None, Some([pitch, yaw])) => bedrock_rotation(yaw, pitch),
                    (None, None) => match preset.rotation_degrees {
                        [None, None] => from.rotation,
                        [pitch, yaw] => {
                            let (from_yaw, from_pitch, _) = from.rotation.to_euler(EulerRot::YXZ);
                            Quat::from_euler(
                                EulerRot::YXZ,
                                yaw.map_or(from_yaw, |yaw| (180.0 - yaw).to_radians()),
                                pitch.map_or(from_pitch, |pitch| -pitch.to_radians()),
                                0.0,
                            )
                        }
                    },
                };
                Target::Fixed(Pose {
                    translation,
                    rotation,
                })
            }
        };
        self.pose = Some(PoseBlend {
            from,
            to,
            elapsed: 0.0,
            duration,
            kind: kind_ease,
        });
    }

    fn apply_fade(&mut self, fade: &CameraFadeInstruction) {
        let previous = self.fade;
        let (fade_in, hold, fade_out) = match (fade.time, previous) {
            (Some(time), _) => (
                time.fade_in_seconds,
                time.hold_seconds,
                time.fade_out_seconds,
            ),
            (None, Some(previous)) => (previous.fade_in, previous.hold, previous.fade_out),
            (None, None) => return,
        };
        let color = fade.color.map_or_else(
            || previous.map_or([0.0; 3], |previous| previous.color),
            |color| [color.red, color.green, color.blue].map(|channel| channel.clamp(0.0, 1.0)),
        );
        let clamp = |seconds: f32| seconds.clamp(0.0, MAX_EASE_SECONDS);
        self.fade = Some(FadeState {
            fade_in: clamp(fade_in),
            hold: clamp(hold),
            fade_out: clamp(fade_out),
            color,
            elapsed: 0.0,
        });
    }

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
            to_degrees: if returning { base_fov } else { fov.degrees },
            returning,
            elapsed: 0.0,
            duration,
            kind: kind_from_name(&fov.ease_type),
        });
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct ResolvedPreset {
    kind: Option<PresetKind>,
    position: [Option<f32>; 3],
    rotation_degrees: [Option<f32>; 2],
    radius: Option<f32>,
}

impl ResolvedPreset {
    /// Fills only the fields no more-derived preset already declared.
    fn absorb(&mut self, preset: &CameraPreset) {
        for (slot, value) in self.position.iter_mut().zip(preset.position) {
            *slot = slot.or(value);
        }
        for (slot, value) in self
            .rotation_degrees
            .iter_mut()
            .zip(preset.rotation_degrees)
        {
            *slot = slot.or(value);
        }
        self.radius = self.radius.or(preset.radius);
    }

    fn has_pose(&self) -> bool {
        self.position
            .iter()
            .chain(&self.rotation_degrees)
            .any(Option::is_some)
    }
}

fn preset_kind_from_name(name: &str) -> Option<PresetKind> {
    match name {
        "minecraft:free" => Some(PresetKind::Free),
        "minecraft:first_person" => Some(PresetKind::FirstPerson),
        "minecraft:third_person" | "minecraft:follow_orbit" | "minecraft:fixed_boom" => {
            Some(PresetKind::ThirdPerson)
        }
        "minecraft:third_person_front" => Some(PresetKind::ThirdPersonFront),
        _ => None,
    }
}

fn axis(explicit: Option<f32>, preset: Option<f32>, current: f32) -> f32 {
    explicit.or(preset).unwrap_or(current)
}

fn bedrock_rotation(yaw_degrees: f32, pitch_degrees: f32) -> Quat {
    Quat::from_euler(
        EulerRot::YXZ,
        (180.0 - yaw_degrees).to_radians(),
        -pitch_degrees.to_radians(),
        0.0,
    )
}

fn look_rotation(from: Vec3, target: Vec3) -> Option<Quat> {
    (from.distance_squared(target) > f32::EPSILON).then(|| {
        Transform::from_translation(from)
            .looking_at(target, Vec3::Y)
            .rotation
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{CameraEase, CameraFadeColor, CameraFadeTimes};

    use super::*;

    fn no_actors(_: i64) -> Option<ActorView> {
        None
    }

    fn ctx(base: Transform) -> ViewContext<'static> {
        ViewContext {
            base,
            subject: base,
            base_fov: 90.0,
            actors: &no_actors,
        }
    }

    fn set_event(set: CameraSetInstruction) -> CameraEvent {
        CameraEvent::Instruction(CameraInstructionEvent {
            set: Some(set),
            ..Default::default()
        })
    }

    fn empty_set() -> CameraSetInstruction {
        CameraSetInstruction {
            preset_id: 0,
            ease: None,
            position: None,
            rotation_degrees: None,
            facing_position: None,
            view_offset: None,
            entity_offset: None,
            default_preset: None,
            remove_ignore_starting_values: false,
        }
    }

    #[test]
    fn set_with_ease_blends_position_from_the_base_pose() {
        let mut view = ServerCameraView::default();
        let base = Transform::from_xyz(0.0, 0.0, 0.0);
        let mut set = empty_set();
        set.position = Some([10.0, 0.0, 0.0]);
        set.ease = Some(CameraEase {
            kind: 0,
            time_seconds: 2.0,
        });
        view.apply(1, &set_event(set), &ctx(base));
        assert_eq!(
            view.pose_override(&ctx(Transform::IDENTITY))
                .unwrap()
                .translation,
            Vec3::ZERO
        );
        view.advance(1.0);
        assert!(
            (view
                .pose_override(&ctx(Transform::IDENTITY))
                .unwrap()
                .translation
                .x
                - 5.0)
                .abs()
                < 1e-4
        );
        view.advance(5.0);
        assert!(
            (view
                .pose_override(&ctx(Transform::IDENTITY))
                .unwrap()
                .translation
                .x
                - 10.0)
                .abs()
                < 1e-4
        );
    }

    #[test]
    fn instant_set_and_clear() {
        let mut view = ServerCameraView::default();
        let mut set = empty_set();
        set.position = Some([1.0, 2.0, 3.0]);
        view.apply(1, &set_event(set), &ctx(Transform::IDENTITY));
        assert_eq!(
            view.pose_override(&ctx(Transform::IDENTITY))
                .unwrap()
                .translation,
            Vec3::new(1.0, 2.0, 3.0)
        );
        view.apply(
            2,
            &CameraEvent::Instruction(CameraInstructionEvent {
                clear: Some(true),
                ..Default::default()
            }),
            &ctx(Transform::IDENTITY),
        );
        assert!(view.pose_override(&ctx(Transform::IDENTITY)).is_none());
        assert_eq!(view.last_sequence(), 2);
    }

    #[test]
    fn bedrock_rotation_zero_yaw_faces_positive_z() {
        let forward = bedrock_rotation(0.0, 0.0) * Vec3::NEG_Z;
        assert!((forward - Vec3::Z).length() < 1e-5);
        let down = bedrock_rotation(0.0, 90.0) * Vec3::NEG_Z;
        assert!(down.y < -0.99);
    }

    #[test]
    fn facing_position_overrides_rotation() {
        let mut view = ServerCameraView::default();
        let mut set = empty_set();
        set.position = Some([0.0, 0.0, 0.0]);
        set.rotation_degrees = Some([0.0, 0.0]);
        set.facing_position = Some([10.0, 0.0, 0.0]);
        view.apply(1, &set_event(set), &ctx(Transform::IDENTITY));
        let forward = view
            .pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .rotation
            * Vec3::NEG_Z;
        assert!((forward - Vec3::X).length() < 1e-5);
    }

    #[test]
    fn preset_only_set_is_counted_not_applied() {
        let mut view = ServerCameraView::default();
        view.apply(1, &set_event(empty_set()), &ctx(Transform::IDENTITY));
        assert!(!view.has_pose_override());
        assert_eq!(view.skips().unresolved_presets, 1);
    }

    #[test]
    fn fade_runs_in_hold_out_then_ends() {
        let mut view = ServerCameraView::default();
        view.apply(
            1,
            &CameraEvent::Instruction(CameraInstructionEvent {
                fade: Some(CameraFadeInstruction {
                    time: Some(CameraFadeTimes {
                        fade_in_seconds: 1.0,
                        hold_seconds: 1.0,
                        fade_out_seconds: 1.0,
                    }),
                    color: Some(CameraFadeColor {
                        red: 1.0,
                        green: 0.0,
                        blue: 2.0,
                    }),
                }),
                ..Default::default()
            }),
            &ctx(Transform::IDENTITY),
        );
        assert_eq!(view.fade_overlay(), Some(([1.0, 0.0, 1.0], 0.0)));
        view.advance(0.5);
        assert!((view.fade_overlay().unwrap().1 - 0.5).abs() < 1e-5);
        view.advance(1.0);
        assert_eq!(view.fade_overlay().unwrap().1, 1.0);
        view.advance(1.0);
        assert!((view.fade_overlay().unwrap().1 - 0.5).abs() < 1e-5);
        view.advance(1.0);
        assert!(view.fade_overlay().is_none());
    }

    #[test]
    fn fov_override_blends_and_releases_to_the_setting() {
        let mut view = ServerCameraView::default();
        let fov = |degrees: f32, clear: bool| {
            CameraEvent::Instruction(CameraInstructionEvent {
                fov: Some(CameraFovInstruction {
                    degrees,
                    ease_time_seconds: 1.0,
                    ease_type: Arc::from("linear"),
                    clear,
                }),
                ..Default::default()
            })
        };
        view.apply(1, &fov(50.0, false), &ctx(Transform::IDENTITY));
        assert_eq!(view.fov_override_degrees(90.0), Some(90.0));
        view.advance(0.5);
        assert!((view.fov_override_degrees(90.0).unwrap() - 70.0).abs() < 1e-4);
        view.advance(1.0);
        assert!((view.fov_override_degrees(90.0).unwrap() - 50.0).abs() < 1e-4);
        view.apply(2, &fov(0.0, true), &ctx(Transform::IDENTITY));
        view.advance(2.0);
        assert_eq!(view.fov_override_degrees(90.0), None);
    }

    #[test]
    fn shakes_route_by_kind_and_stop() {
        let mut view = ServerCameraView::default();
        let shake = |shake_type, action| {
            CameraEvent::Shake(CameraShakeEvent {
                intensity: 1.0,
                duration_seconds: 2.0,
                shake_type,
                action,
            })
        };
        view.apply(
            1,
            &shake(CameraShakeType::Positional, CameraShakeAction::Add),
            &ctx(Transform::IDENTITY),
        );
        assert!(view.is_active());
        view.apply(
            2,
            &shake(CameraShakeType::Unknown(9), CameraShakeAction::Add),
            &ctx(Transform::IDENTITY),
        );
        assert_eq!(view.skips().unknown_shake, 1);
        view.apply(
            3,
            &shake(CameraShakeType::Positional, CameraShakeAction::Stop),
            &ctx(Transform::IDENTITY),
        );
        assert!(!view.is_active());
    }

    #[test]
    fn upstream_reset_drops_state_but_keeps_counters() {
        let mut view = ServerCameraView::default();
        let mut set = empty_set();
        set.position = Some([1.0, 0.0, 0.0]);
        view.apply(1, &set_event(empty_set()), &ctx(Transform::IDENTITY));
        view.apply(2, &set_event(set), &ctx(Transform::IDENTITY));
        view.observe_resets(1);
        assert!(!view.has_pose_override());
        assert_eq!(view.skips().unresolved_presets, 1);
    }

    fn preset(name: &str, inherit: &str) -> CameraPreset {
        CameraPreset {
            name: Arc::from(name),
            inherit_from: Arc::from(inherit),
            ..Default::default()
        }
    }

    fn registry(presets: Vec<CameraPreset>) -> CameraEvent {
        CameraEvent::Presets(presets.into())
    }

    fn set_preset(id: u32) -> CameraEvent {
        let mut set = empty_set();
        set.preset_id = id;
        set_event(set)
    }

    #[test]
    fn free_preset_inheritance_supplies_pose_and_explicit_fields_win() {
        let mut view = ServerCameraView::default();
        let mut parent = preset("custom:base", "minecraft:free");
        parent.position = [Some(5.0), Some(6.0), Some(7.0)];
        let mut child = preset("custom:child", "custom:base");
        child.position = [Some(9.0), None, None];
        view.apply(1, &registry(vec![child, parent]), &ctx(Transform::IDENTITY));
        view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
        let translation = view
            .pose_override(&ctx(Transform::IDENTITY))
            .unwrap()
            .translation;
        assert_eq!(translation, Vec3::new(9.0, 6.0, 7.0));

        let mut set = empty_set();
        set.position = Some([1.0, 2.0, 3.0]);
        view.apply(3, &set_event(set), &ctx(Transform::IDENTITY));
        assert_eq!(
            view.pose_override(&ctx(Transform::IDENTITY))
                .unwrap()
                .translation,
            Vec3::new(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn first_person_preset_releases_the_camera_and_registry_survives_reset() {
        let mut view = ServerCameraView::default();
        let mut free = preset("minecraft:free", "");
        free.position = [Some(1.0); 3];
        view.apply(
            1,
            &registry(vec![free, preset("minecraft:first_person", "")]),
            &ctx(Transform::IDENTITY),
        );
        view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
        assert!(view.has_pose_override());
        view.apply(3, &set_preset(1), &ctx(Transform::IDENTITY));
        assert!(!view.has_pose_override());
        view.apply(4, &set_preset(0), &ctx(Transform::IDENTITY));
        view.observe_resets(1);
        assert!(!view.has_pose_override());
        view.apply(5, &set_preset(0), &ctx(Transform::IDENTITY));
        assert!(view.has_pose_override());
    }

    #[test]
    fn third_person_preset_orbits_the_subject_at_its_radius() {
        let mut view = ServerCameraView::default();
        let mut orbit = preset("minecraft:third_person", "");
        orbit.radius = Some(6.0);
        view.apply(1, &registry(vec![orbit]), &ctx(Transform::IDENTITY));
        view.apply(2, &set_preset(0), &ctx(Transform::IDENTITY));
        let mut context = ctx(Transform::IDENTITY);
        context.subject = Transform::from_xyz(10.0, 70.0, 10.0);
        let camera = view.pose_override(&context).unwrap();
        assert!((camera.translation - Vec3::new(10.0, 70.0, 16.0)).length() < 1e-4);
    }

    #[test]
    fn unknown_preset_ids_are_counted() {
        let mut view = ServerCameraView::default();
        view.apply(1, &set_preset(7), &ctx(Transform::IDENTITY));
        assert_eq!(view.skips().unresolved_presets, 1);
    }

    #[test]
    fn attach_and_target_follow_actor_positions() {
        let actors = |id: i64| {
            (id == 7).then_some(ActorView {
                position: Vec3::new(0.0, 0.0, -10.0),
                yaw_degrees: 0.0,
                pitch_degrees: 0.0,
            })
        };
        let context = ViewContext {
            base: Transform::IDENTITY,
            subject: Transform::IDENTITY,
            base_fov: 90.0,
            actors: &actors,
        };
        let mut view = ServerCameraView::default();
        view.apply(
            1,
            &CameraEvent::Instruction(CameraInstructionEvent {
                attach_to_entity: Some(7),
                ..Default::default()
            }),
            &context,
        );
        let attached = view.pose_override(&context).unwrap();
        assert_eq!(attached.translation, Vec3::new(0.0, 0.0, -10.0));
        // Bedrock yaw 0 faces +Z.
        assert!(((attached.rotation * Vec3::NEG_Z) - Vec3::Z).length() < 1e-5);

        view.apply(
            2,
            &CameraEvent::Instruction(CameraInstructionEvent {
                target: Some(protocol::CameraTargetInstruction {
                    center_offset: None,
                    actor_unique_id: 7,
                }),
                detach_from_entity: true,
                ..Default::default()
            }),
            &context,
        );
        assert!(view.pose_override(&context).is_none());
        assert_eq!(view.skips().actor_bound, 0);
    }

    #[test]
    fn missing_actors_are_counted_not_fatal() {
        let mut view = ServerCameraView::default();
        view.apply(
            1,
            &CameraEvent::Instruction(CameraInstructionEvent {
                attach_to_entity: Some(99),
                ..Default::default()
            }),
            &ctx(Transform::IDENTITY),
        );
        assert_eq!(view.skips().actor_bound, 1);
        // The attachment is kept; until the actor exists the player camera stays in charge.
        assert!(view.has_pose_override());
        assert!(view.pose_override(&ctx(Transform::IDENTITY)).is_none());
    }
}
