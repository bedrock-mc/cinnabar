//! Tick-owned rig outputs that renderers and the HUD read.
use super::*;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ActorLifetimeId {
    pub session_id: u64,
    pub dimension: i32,
    pub runtime_id: u64,
    pub spawn_revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EntityRigId(pub u32);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoneTransform {
    pub rotation: [f32; 4],
    pub translation_scale: [f32; 4],
    /// Non-uniform scale in the bone's own frame; `[1; 3]` when the scale is uniform.
    pub axis_scale: [f32; 3],
}

/// Parent links of a rig's posed skeleton, indexed like its pose and `bone_names`.
#[derive(Clone, Copy, Debug)]
pub struct BoneHierarchy<'a>(pub(super) &'a [RuntimeBone]);

impl BoneHierarchy<'_> {
    /// Whether `bone` is `ancestor` itself or one of its descendants.
    pub fn descends_from(self, bone: usize, ancestor: usize) -> bool {
        let mut current = Some(bone);
        // Parent links come from validated skeletons; the bound still stops a malformed cycle.
        for _ in 0..=self.0.len() {
            match current {
                Some(index) if index == ancestor => return true,
                Some(index) => current = self.0.get(index).and_then(|bone| bone.parent),
                None => return false,
            }
        }
        false
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ActorRigSnapshot<'a> {
    /// Coordinate frames cached with the selected rig.
    pub placement_frame: assets::placement::EntityPlacementFrame,
    pub actor: ActorLifetimeId,
    pub rig: EntityRigId,
    pub previous: &'a [BoneTransform],
    pub current: &'a [BoneTransform],
    /// Immutable authored rest transforms from the exact resolved geometry.
    pub rest: &'a [BoneTransform],
    /// Actual fixed-tick observation of this lifetime, independent of pose evaluation.
    pub rest_completed_tick: u64,
    pub rest_reset_generation: u64,
    pub completed_tick: u64,
    pub reset_generation: u64,
    /// Whether authored pose and render-controller evaluation has succeeded for this lifetime.
    pub pose_evaluated: bool,
    pub fallback: EntityRigFallback,
    /// Authored uniform model scale about the feet origin.
    pub scale: f32,
    /// Authored per-axis model scale (`scaleX`, `scaleY`, `scaleZ`) on top of `scale`.
    pub axis_scale: [f32; 3],
    /// Authored main-hand item scale, independent of the owner's body and offhand.
    pub held_item_scale: Option<f32>,
    /// Body yaw in degrees at the previous and current completed tick.
    pub previous_body_yaw: f32,
    pub body_yaw: f32,
    /// Texture layers the rig's render controllers select this tick, in draw order.
    pub render: &'a [RenderTextureLayer],
    /// Indices prepared for the current appearance; absent on unbound diagnostic snapshots.
    pub attachments: Option<&'a Arc<render_model::ActorAttachmentBindings>>,
    /// Lowercase bone names in pose order.
    pub bone_names: &'a [Box<str>],
    /// The skin model the pose drives, instead of the rig's geometry.
    pub skin_geometry: Option<&'a Arc<assets::SkinGeometry>>,
    /// Immutable prepared vertices matching `skin_geometry`; absent when that mesh is invalid.
    pub skin_mesh: Option<&'a render_model::ActorRigGeometry>,
    pub skin_layers: &'a [SkinRenderLayer],
    /// Swing and equip progress at the previous and current completed tick.
    pub hand: [HandPhase; 2],
    /// Main-hand item retained until the native equip motion reaches its low point.
    pub main_hand_equipped: Option<&'a str>,
    /// Previous/current fixed-tick item animation, independent of the avatar skeleton.
    pub item_animation: [ItemAnimationState; 2],
    /// Independent previous/current offhand equip progress; no main-hand attack swing.
    pub off_hand_animation: [ItemAnimationState; 2],
    /// The owner's retained Molang values and lifetime for animated equipment.
    pub animation_variables: ActorAnimationVariables<'a>,
    /// Java 1.7 limb swing, body yaw and equip progress over the last two ticks.
    pub java: JavaMotion,
    /// The main-hand item Java's first-person hand still draws while the equip dips.
    pub java_equipped: Option<&'a JavaHeldItem>,
}

/// Local swing samples from the committed physics ticks, independent of the remote actor clock.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LocalSwingProgress {
    pub bedrock: [f32; 2],
    pub java: [f32; 2],
    pub frame_alpha: Option<f32>,
}

impl LocalSwingProgress {
    /// Samples the native forward wrap using the local physics frame fraction when available.
    pub fn bedrock_progress(self, alpha: f32) -> f32 {
        Self::interpolate(self.bedrock, self.frame_alpha.unwrap_or(alpha))
    }

    /// The native final frame wraps forward before the next tick returns to rest.
    pub(super) fn interpolate([previous, current]: [f32; 2], alpha: f32) -> f32 {
        let mut delta = current - previous;
        if delta < 0.0 {
            delta += 1.0;
        }
        previous + delta * alpha
    }
}

/// The arm's swing and equip progress over one tick, as the first-person item reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandPhase {
    /// 0..1 swing progress; 0 at rest.
    pub attack_time: f32,
    /// 0..1 equip progress; 1 once the held item has settled.
    pub arm_height: f32,
    /// Consecutive ticks the using-item flag has been set.
    pub use_ticks: u32,
}

impl Default for HandPhase {
    fn default() -> Self {
        Self {
            attack_time: 0.0,
            arm_height: 1.0,
            use_ticks: 0,
        }
    }
}

/// Tick-owned inputs to the camera-space legacy item renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemAnimationState {
    pub attack_time: f32,
    pub arm_height: f32,
}

impl From<HandPhase> for ItemAnimationState {
    fn from(phase: HandPhase) -> Self {
        Self {
            attack_time: phase.attack_time,
            arm_height: phase.arm_height,
        }
    }
}

impl Default for ItemAnimationState {
    fn default() -> Self {
        HandPhase::default().into()
    }
}

impl ItemAnimationState {
    /// Interpolates tick observations before nonlinear swing evaluation, including its wrap.
    pub fn interpolate(self, current: Self, fraction: f32) -> Self {
        let mut delta = current.attack_time - self.attack_time;
        if delta < 0.0 {
            delta += 1.0;
        }
        Self {
            attack_time: self.attack_time + delta * fraction,
            arm_height: self.arm_height + (current.arm_height - self.arm_height) * fraction,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActorAnimationStats {
    pub evaluated_molang_ops: u64,
    pub actor_budget_exhaustions: u64,
    pub world_budget_exhaustions: u64,
    pub frozen_actors: u64,
    pub invalid_server_stop_expressions: u64,
    pub unrigged_spawns: u64, // spawns with entity assets loaded but no compiled rig
    pub invalid_skin_geometries: u64, // skin models that fell back to the default geometry
}
