//! Per-rig runtime state and the evaluation budget shared by every pose pass.
use super::*;

#[derive(Debug)]
pub(super) struct ActorRigState {
    pub(super) placement_frame: assets::placement::EntityPlacementFrame,
    /// Resolved from the session pack catalog rather than the vanilla one.
    pub(super) pack: bool,
    pub(super) rig: EntityRigId,
    pub(super) rig_binding: usize,
    pub(super) geometry_binding: usize,
    pub(super) bones: Vec<RuntimeBone>,
    /// Lowercase bone names in `bones` order, for part visibility.
    pub(super) bone_names: Vec<Box<str>>,
    /// This tick's render-controller result.
    pub(super) render: Vec<RenderTextureLayer>,
    /// Includes dormant controller layers, so admission need not sample ordinary rigs.
    pub(super) may_use_always_depth_material: bool,
    /// This tick's evaluated `[scale, scaleX, scaleY, scaleZ]`, for rigs that script them.
    pub(super) scale: Option<[f32; 4]>,
    /// Skeletons of the geometries render controllers draw instead of the rig's, by geometry.
    pub(super) layer_skeletons: BTreeMap<u32, Option<Arc<render::LayerSkeleton>>>,
    pub(super) controllers: Vec<ControllerState>,
    pub(super) server_animations: Vec<server_animation::Controller>,
    pub(super) clip_clocks: clock::ClipClocks,
    pub(super) previous: Vec<BoneTransform>,
    pub(super) current: Vec<BoneTransform>,
    /// Third-person evaluation of the local rig for the HUD, independent of the hand pose.
    pub(super) ui_pose: Option<Vec<BoneTransform>>,
    pub(super) ui_animation: Option<hud::UiAnimationState>,
    pub(super) view_context: Option<bool>,
    pub(super) rest: Vec<BoneTransform>,
    pub(super) rest_completed_tick: u64,
    pub(super) rest_reset_generation: u64,
    pub(super) rest_reset_pending: bool,
    pub(super) reset_generation: u64,
    pub(super) reset_pending: bool,
    pub(super) lifetime_epoch: u64,
    pub(super) animation_epoch: u64,
    pub(super) completed_tick: u64,
    pub(super) fallback: EntityRigFallback,
    pub(super) history: VecDeque<ActorTickInput>,
    /// The sole history sample is provisional until a real motion tick observes it.
    pub(super) presentation_seeded: bool,
    /// Latest owner query inputs, retained even when its pose does not need frame sampling.
    pub(super) query_context: ActorTickContext,
    /// Main-hand item the arm has finished equipping.
    pub(super) equipped_main: Option<Arc<str>>,
    /// Offhand item accepted by the independent native equip clock.
    pub(super) equipped_off: Option<Arc<str>>,
    /// Retained separately from pose history, so a geometry reset cannot restart equipping.
    pub(super) off_hand_animation: [ItemAnimationState; 2],
    /// The worn skin's own model, when it names one.
    pub(super) skin: Option<skin::SkinModel>,
    pub(super) skin_layers: Vec<SkinRenderLayer>,
    pub(super) variables: MolangVariables,
    pub(super) replay: Option<replay::Replay>,
    pub(super) complete_spear_variables: bool,
    pub(super) samples_render_frames: bool,
    pub(super) samples_camera_poses: bool,
    pub(super) presentation_expressions: evaluation::PresentationReads,
    pub(super) samples_swing_poses: bool,
    /// Actor kind admits swelling independently of its initially bound clips.
    pub(super) creeper: bool,
    pub(super) swell_sampling: Option<Arc<render_frame::swell::SwellSampling>>,
    pub(super) render_frame: Option<render_frame::FrameState>,
    pub(super) initialized: bool,
    /// Outside the animation view at its last tick, holding its pose.
    pub(super) culled: bool,
    pub(super) local_swing: Option<LocalSwingProgress>,
    pub(super) motion: MotionState,
    pub(super) java: java::JavaMotionState,
}

#[derive(Clone, Debug, Default)]
pub(super) struct RuntimeBone {
    pub(super) parent: Option<usize>,
    pub(super) pivot: [f32; 3],
    pub(super) rotation: [f32; 3],
    pub(super) has_binding_expression: bool,
    pub(super) attachable_root: AttachableRootFrame,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) enum AttachableRootFrame {
    #[default]
    Actor,
    MatchingOwnerName,
    BindingExpression,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ControllerState {
    pub(super) controller: usize,
    pub(super) state: u16,
    pub(super) active: bool,
    /// Animation tick the current state was entered, where its clips start.
    pub(super) entered_tick: u64,
    /// Outgoing state, its clip epoch and the frame fraction the worn blend began at.
    pub(super) blend_from: Option<(u16, u64, f32)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ActorTickInput {
    pub(super) placement_frame: assets::placement::EntityPlacementFrame,
    pub(super) position: [f32; 3],
    pub(super) position_delta: [f32; 3],
    pub(super) velocity: [f32; 3],
    pub(super) on_ground: bool,
    pub(super) body_yaw: f32,
    /// Actor rotation yaw, separate from a mob's independently animated body/head.
    pub(super) yaw: f32,
    pub(super) head_yaw: f32,
    pub(super) pitch: f32,
    pub(super) is_riding: bool,
    pub(super) distance_moved: f32,
    pub(super) move_speed: f32,
    pub(super) walk_distance: f32,
    /// Consecutive ticks the using-item flag has been set.
    pub(super) item_use_ticks: u32,
    /// Smoothed 0..1 swimming-posture blend.
    pub(super) swim_amount: f32,
    /// 0..1 equip progress; 1 once the held item has settled.
    pub(super) arm_height: f32,
    /// Independent offhand equip progress at this tick.
    pub(super) off_hand_arm_height: f32,
    /// 0..1 swing progress after this tick's motion advance.
    pub(super) attack_time: f32,
}

pub(super) struct EvaluatedState {
    pub(super) pose: Vec<BoneTransform>,
    pub(super) skin_layers: Vec<SkinRenderLayer>,
    /// `None` when the render controllers ran out of budget, keeping the last choice.
    pub(super) render: Option<Vec<RenderTextureLayer>>,
    pub(super) scale: Option<[f32; 4]>,
    pub(super) controllers: Vec<ControllerState>,
    pub(super) server_animations: Vec<server_animation::Controller>,
    pub(super) clip_clocks: clock::ClipClocks,
    pub(super) variables: MolangVariables,
    pub(super) render_frame: Option<render_frame::FrameState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EvalError {
    ActorBudget,
    WorldBudget,
    Invalid,
}

pub(super) struct EvalBudget<'a> {
    pub(super) actor_left: usize,
    pub(super) world_left: &'a mut usize,
    pub(super) work_left: usize,
    pub(super) transitions_left: usize,
    pub(super) used: usize,
    /// Operand stack lent to each expression run, so runs reuse one allocation.
    pub(super) stack: Vec<evaluation::MolangValue>,
    /// Tracks whether an attachable result reads only explicitly memoized inputs.
    pub(super) static_draw: Option<bool>,
}

impl EvalBudget<'_> {
    pub(super) fn charge(&mut self) -> Result<(), EvalError> {
        if self.actor_left == 0 {
            return Err(EvalError::ActorBudget);
        }
        if *self.world_left == 0 {
            return Err(EvalError::WorldBudget);
        }
        self.actor_left -= 1;
        *self.world_left -= 1;
        self.used += 1;
        Ok(())
    }

    pub(super) fn charge_work(&mut self) -> Result<(), EvalError> {
        if self.work_left == 0 {
            return Err(EvalError::ActorBudget);
        }
        self.work_left -= 1;
        Ok(())
    }

    pub(super) fn take_transition(&mut self) -> bool {
        if self.transitions_left == 0 {
            return false;
        }
        self.transitions_left -= 1;
        true
    }
}
