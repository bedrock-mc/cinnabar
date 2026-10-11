use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use assets::{
    EntityAnimationInterpolation, EntityAnimationLoop, EntityAssetKind, EntityRigFallback,
    MolangOp, RuntimeEntityAssets,
};
use protocol::{ActorKind, ActorMetadataValue};

use crate::actor_store::ActorSnapshot;

/// Simulation tick duration used by actor clocks and Molang time queries.
use world::TICK_DURATION as ACTOR_TICK_DURATION;

pub const MAX_RUNTIME_BONES_PER_RIG: usize = assets::MAX_ENTITY_GEOMETRY_BONES;
const ANIMATION_TICK_SECONDS: f32 = ACTOR_TICK_DURATION.as_secs_f32();
pub const MAX_CONTROLLER_TRANSITIONS_PER_TICK: usize = 8;
pub const MAX_MOLANG_OPS_PER_ACTOR_TICK: usize = 4_096;
pub const MAX_MOLANG_OPS_PER_WORLD_TICK: usize = 262_144;
/// World-wide ceiling for authored render-time layer expressions; pose histories stay tick-owned.
pub const MAX_MOLANG_OPS_PER_RENDER_FRAME: usize = MAX_MOLANG_OPS_PER_WORLD_TICK;
pub const MAX_ACTOR_ACTION_HISTORY: usize = 32;
const MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK: usize = 4_096;
const MAX_RUNTIME_BINDINGS_PER_RIG: usize = 4_096;

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

#[derive(Clone, Copy, Debug)]
pub struct ActorRigSnapshot<'a> {
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
    pub fallback: EntityRigFallback,
    /// Authored uniform model scale about the feet origin.
    pub scale: f32,
    /// Authored per-axis model scale (`scaleX`, `scaleY`, `scaleZ`) on top of `scale`.
    pub axis_scale: [f32; 3],
    /// Body yaw in degrees at the previous and current completed tick.
    pub previous_body_yaw: f32,
    pub body_yaw: f32,
    /// Texture layers the rig's render controllers select this tick, in draw order.
    pub render: &'a [RenderTextureLayer],
    /// Lowercase bone names in pose order.
    pub bone_names: &'a [Box<str>],
    /// The skin model the pose drives, instead of the rig's geometry.
    pub skin_geometry: Option<&'a Arc<assets::SkinGeometry>>,
    /// Immutable worker-built vertices matching `skin_geometry`; absent when that mesh is invalid.
    pub skin_mesh: Option<&'a render_model::ActorRigGeometry>,
    pub skin_layers: &'a [SkinRenderLayer],
    /// Swing and equip progress at the previous and current completed tick.
    pub hand: [HandPhase; 2],
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

#[derive(Debug)]
pub(crate) struct ActorAnimationStore {
    assets: Option<Arc<RuntimeEntityAssets>>,
    layout: Arc<VariableLayout>,
    /// The session's server-pack entity catalog, in its own index space; its entities win.
    pack: Option<PackCatalog>,
    server_compiler: Option<ServerAnimationCompiler>,
    rigs: BTreeMap<ActorLifetimeId, ActorRigState>,
    skin_preparation: skin::SkinPreparationQueue,
    runtime_to_lifetime: HashMap<u64, ActorLifetimeId>,
    /// First actor the world budget skipped last tick, where the next tick starts.
    first_starved: Option<ActorLifetimeId>,
    local_motion_authority: Option<(u64, (u64, u64))>,
    completed_tick: u64,
    next_reset_generation: u64,
    next_rest_reset_generation: u64,
    stats: ActorAnimationStats,
    #[cfg(test)]
    schedule: schedule::TestSchedule,
}

#[derive(Debug)]
struct PackCatalog {
    /// Rig geometry bindings with artwork; a pack entity without any draws as vanilla does.
    artwork: std::collections::BTreeSet<u32>,
    assets: Arc<RuntimeEntityAssets>,
    layout: Arc<VariableLayout>,
}

#[derive(Debug)]
struct PoseStep {
    evaluate: bool,
    reset_motion_history: bool,
    refresh_view: bool,
}

#[derive(Debug)]
struct ActorRigState {
    /// Resolved from the session pack catalog rather than the vanilla one.
    pack: bool,
    rig: EntityRigId,
    rig_binding: usize,
    geometry_binding: usize,
    bones: Vec<RuntimeBone>,
    /// Lowercase bone names in `bones` order, for part visibility.
    bone_names: Vec<Box<str>>,
    /// This tick's render-controller result.
    render: Vec<RenderTextureLayer>,
    /// Includes dormant controller layers, so admission need not sample ordinary rigs.
    may_use_always_depth_material: bool,
    /// This tick's evaluated `[scale, scaleX, scaleY, scaleZ]`, for rigs that script them.
    scale: Option<[f32; 4]>,
    /// Skeletons of the geometries render controllers draw instead of the rig's, by geometry.
    layer_skeletons: BTreeMap<u32, Option<Arc<render::LayerSkeleton>>>,
    controllers: Vec<ControllerState>,
    server_animations: Vec<server_animation::Controller>,
    clip_clocks: clock::ClipClocks,
    previous: Vec<BoneTransform>,
    current: Vec<BoneTransform>,
    /// Third-person evaluation of the local rig for the HUD, independent of the hand pose.
    ui_pose: Option<Vec<BoneTransform>>,
    ui_animation: Option<hud::UiAnimationState>,
    view_context: Option<bool>,
    rest: Vec<BoneTransform>,
    rest_completed_tick: u64,
    rest_reset_generation: u64,
    rest_reset_pending: bool,
    reset_generation: u64,
    reset_pending: bool,
    lifetime_epoch: u64,
    animation_epoch: u64,
    completed_tick: u64,
    fallback: EntityRigFallback,
    history: VecDeque<ActorTickInput>,
    /// Latest owner query inputs, retained even when its pose does not need frame sampling.
    query_context: ActorTickContext,
    /// Main-hand item the arm has finished equipping.
    equipped_main: Option<Arc<str>>,
    /// Offhand item accepted by the independent native equip clock.
    equipped_off: Option<Arc<str>>,
    /// Retained separately from pose history, so a geometry reset cannot restart equipping.
    off_hand_animation: [ItemAnimationState; 2],
    /// The worn skin's own model, when it names one.
    skin: Option<skin::SkinModel>,
    skin_layers: Vec<SkinRenderLayer>,
    variables: MolangVariables,
    replay: Option<replay::Replay>,
    complete_spear_variables: bool,
    samples_render_frames: bool,
    samples_camera_poses: bool,
    presentation_expressions: evaluation::PresentationReads,
    samples_swing_poses: bool,
    /// Actor kind admits swelling independently of its initially bound clips.
    creeper: bool,
    swell_sampling: Option<Arc<render_frame::swell::SwellSampling>>,
    render_frame: Option<render_frame::FrameState>,
    initialized: bool,
    /// Outside the animation view at its last tick, holding its pose.
    culled: bool,
    local_swing: Option<LocalSwingProgress>,
    motion: MotionState,
    java: java::JavaMotionState,
}

#[derive(Clone, Debug, Default)]
struct RuntimeBone {
    parent: Option<usize>,
    pivot: [f32; 3],
    rotation: [f32; 3],
    has_binding_expression: bool,
    attachable_root: AttachableRootFrame,
}

#[derive(Clone, Copy, Debug, Default)]
enum AttachableRootFrame {
    #[default]
    Actor,
    MatchingOwnerName,
    BindingExpression,
}

#[derive(Clone, Copy, Debug)]
struct ControllerState {
    controller: usize,
    state: u16,
    active: bool,
    /// Animation tick the current state was entered, where its clips start.
    entered_tick: u64,
    /// Outgoing state, its clip epoch and the frame fraction the worn blend began at.
    blend_from: Option<(u16, u64, f32)>,
}

#[derive(Clone, Copy, Debug, Default)]
struct ActorTickInput {
    position: [f32; 3],
    position_delta: [f32; 3],
    velocity: [f32; 3],
    on_ground: bool,
    body_yaw: f32,
    /// Actor rotation yaw, separate from a mob's independently animated body/head.
    yaw: f32,
    head_yaw: f32,
    pitch: f32,
    is_riding: bool,
    distance_moved: f32,
    move_speed: f32,
    walk_distance: f32,
    /// Consecutive ticks the using-item flag has been set.
    item_use_ticks: u32,
    /// Smoothed 0..1 swimming-posture blend.
    swim_amount: f32,
    /// 0..1 equip progress; 1 once the held item has settled.
    arm_height: f32,
    /// Independent offhand equip progress at this tick.
    off_hand_arm_height: f32,
    /// 0..1 swing progress after this tick's motion advance.
    attack_time: f32,
}

struct EvaluatedState {
    pose: Vec<BoneTransform>,
    skin_layers: Vec<SkinRenderLayer>,
    /// `None` when the render controllers ran out of budget, keeping the last choice.
    render: Option<Vec<RenderTextureLayer>>,
    scale: Option<[f32; 4]>,
    controllers: Vec<ControllerState>,
    server_animations: Vec<server_animation::Controller>,
    clip_clocks: clock::ClipClocks,
    variables: MolangVariables,
    render_frame: Option<render_frame::FrameState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EvalError {
    ActorBudget,
    WorldBudget,
    Invalid,
}

struct EvalBudget<'a> {
    actor_left: usize,
    world_left: &'a mut usize,
    work_left: usize,
    transitions_left: usize,
    used: usize,
    /// Operand stack lent to each expression run, so runs reuse one allocation.
    stack: Vec<evaluation::MolangValue>,
    /// Tracks whether an attachable result reads only explicitly memoized inputs.
    static_draw: Option<bool>,
}

impl EvalBudget<'_> {
    fn charge(&mut self) -> Result<(), EvalError> {
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

    fn charge_work(&mut self) -> Result<(), EvalError> {
        if self.work_left == 0 {
            return Err(EvalError::ActorBudget);
        }
        self.work_left -= 1;
        Ok(())
    }

    fn take_transition(&mut self) -> bool {
        if self.transitions_left == 0 {
            return false;
        }
        self.transitions_left -= 1;
        true
    }
}

impl ActorAnimationStore {
    pub(crate) fn diagnostic() -> Self {
        Self::new(None)
    }

    pub(crate) fn with_assets(assets: Arc<RuntimeEntityAssets>) -> Self {
        Self::new(Some(assets))
    }

    fn new(assets: Option<Arc<RuntimeEntityAssets>>) -> Self {
        let mut store = Self {
            layout: Arc::new(
                assets
                    .as_deref()
                    .map(VariableLayout::new)
                    .unwrap_or_default(),
            ),
            assets,
            pack: None,
            server_compiler: None,
            rigs: BTreeMap::new(),
            skin_preparation: skin::SkinPreparationQueue::default(),
            runtime_to_lifetime: HashMap::new(),
            first_starved: None,
            local_motion_authority: None,
            completed_tick: 0,
            next_reset_generation: 1,
            next_rest_reset_generation: 1,
            stats: ActorAnimationStats::default(),
            #[cfg(test)]
            schedule: Default::default(),
        };
        store.prewarm_skin_catalog();
        store
    }

    /// Layers a server-pack entity catalog over the vanilla one for actors spawned afterwards.
    pub(crate) fn set_pack(&mut self, assets: Option<(Arc<RuntimeEntityAssets>, Vec<u32>)>) {
        self.pack = assets.map(|(assets, artwork)| PackCatalog {
            layout: Arc::new(VariableLayout::new(&assets)),
            artwork: artwork.into_iter().collect(),
            assets,
        });
    }

    pub(crate) fn set_server_animation_compiler(&mut self, compiler: ServerAnimationCompiler) {
        self.server_compiler = Some(compiler);
    }

    pub(crate) fn clear(&mut self) {
        self.rigs.clear();
        self.skin_preparation = skin::SkinPreparationQueue::default();
        self.prewarm_skin_catalog();
        self.local_motion_authority = None;
        self.runtime_to_lifetime.clear();
        self.completed_tick = 0;
        self.bump_generation();
    }

    pub(crate) fn remove_runtime(&mut self, runtime_id: u64) {
        if let Some(lifetime) = self.runtime_to_lifetime.remove(&runtime_id) {
            self.rigs.remove(&lifetime);
        }
    }

    pub(crate) fn insert(&mut self, session_id: u64, dimension: i32, actor: &ActorSnapshot) {
        self.remove_runtime(actor.runtime_id);
        let Some(assets) = self.assets.clone() else {
            return;
        };
        let lifetime = ActorLifetimeId {
            session_id,
            dimension,
            runtime_id: actor.runtime_id,
            spawn_revision: actor.spawn_revision,
        };
        let from_pack = self.pack.as_ref().and_then(|pack| {
            let mut state = resolve_rig(&pack.assets, &pack.layout, actor, self.completed_tick)?;
            if !pack.artwork.contains(&state.rig.0) {
                return None;
            }
            state.pack = true;
            state.rig = EntityRigId(assets::PACK_RIG_ID_BASE.checked_add(state.rig.0)?);
            Some(state)
        });
        let resolved =
            from_pack.or_else(|| resolve_rig(&assets, &self.layout, actor, self.completed_tick));
        let Some(mut state) = resolved else {
            self.stats.unrigged_spawns = self.stats.unrigged_spawns.saturating_add(1);
            return;
        };
        if let Some((runtime_id, authority)) = self.local_motion_authority
            && runtime_id == actor.runtime_id
        {
            state
                .java
                .local_body
                .set_authority(Some(authority), &mut state.java.motion);
        }
        state.reset_generation = self.next_reset_generation;
        state.rest_reset_generation = self.take_rest_generation().unwrap_or(0);
        self.bump_generation();
        self.runtime_to_lifetime.insert(actor.runtime_id, lifetime);
        self.rigs.insert(lifetime, state);
    }

    pub(crate) fn mark_reset(&mut self, runtime_id: u64) {
        let Some(lifetime) = self.runtime_to_lifetime.get(&runtime_id) else {
            return;
        };
        if let Some(state) = self.rigs.get_mut(lifetime) {
            state.reset_pending = true;
            state.rest_reset_pending = true;
        }
    }

    /// Drops Java's equip progress to zero at the actor's next tick.
    pub(crate) fn reset_java_equip(&mut self, runtime_id: u64) {
        let Some(lifetime) = self.runtime_to_lifetime.get(&runtime_id) else {
            return;
        };
        if let Some(state) = self.rigs.get_mut(lifetime) {
            state.java.reset_equip();
        }
    }

    /// Applies Java's limb boost on every hurt event, independently of the hurt countdown.
    pub(crate) fn hurt_java_limbs(&mut self, runtime_id: u64) {
        let Some(lifetime) = self.runtime_to_lifetime.get(&runtime_id) else {
            return;
        };
        if let Some(state) = self.rigs.get_mut(lifetime) {
            state.java.hurt();
        }
    }

    /// Restarts the arm swing whose progress feeds `variable.attack_time`.
    pub(crate) fn start_swing(&mut self, runtime_id: u64, ticks: i32) {
        let Some(lifetime) = self.runtime_to_lifetime.get(&runtime_id) else {
            return;
        };
        if let Some(state) = self.rigs.get_mut(lifetime) {
            state.local_swing = None;
            state.java.motion.local_swing_alpha = None;
            state.motion.set_local_swing(None);
            state.motion.start_swing(ticks);
            state.java.start_swing(ticks);
        }
    }

    /// Installs authoritative local swing samples and requests evaluation only when they change.
    pub(crate) fn sync_local_swing(
        &mut self,
        runtime_id: u64,
        progress: LocalSwingProgress,
    ) -> bool {
        let Some(state) = self
            .runtime_to_lifetime
            .get(&runtime_id)
            .and_then(|id| self.rigs.get_mut(id))
        else {
            return false;
        };
        let changed = state
            .local_swing
            .is_none_or(|old| old.bedrock != progress.bedrock || old.java != progress.java);
        state.local_swing = Some(progress);
        state.motion.set_local_swing(Some(progress.bedrock[1]));
        state.java.motion.swing = progress.java;
        state.java.motion.local_swing_alpha = progress.frame_alpha;
        if state.java.local_body.active() {
            state.java.motion.body_frame_alpha = progress.frame_alpha;
        }
        if let Some(input) = state.history.back_mut() {
            input.attack_time = progress.bedrock[1];
        }
        changed
    }

    /// Advances tick state; only the frame's final tick evaluates visual controllers and poses.
    pub(crate) fn advance_tick(
        &mut self,
        actors: &HashMap<u64, ActorSnapshot>,
        view: Option<&ActorAnimationView>,
        exempt: Option<u64>,
        evaluate: bool,
        reset_motion_history: bool,
        context: impl Fn(&ActorSnapshot) -> ActorTickContext,
    ) {
        self.evaluate_tick(
            actors,
            view,
            exempt,
            PoseStep {
                evaluate,
                reset_motion_history,
                refresh_view: false,
            },
            context,
        );
    }

    /// Changes the local draw context without advancing motion or clip time.
    pub(crate) fn refresh_local_view(
        &mut self,
        actors: &HashMap<u64, ActorSnapshot>,
        runtime_id: u64,
        context: impl Fn(&ActorSnapshot) -> ActorTickContext,
    ) {
        self.evaluate_tick(
            actors,
            None,
            Some(runtime_id),
            PoseStep {
                evaluate: true,
                reset_motion_history: false,
                refresh_view: true,
            },
            context,
        );
    }

    pub(crate) fn get(&self, runtime_id: u64) -> Option<ActorRigSnapshot<'_>> {
        let lifetime = *self.runtime_to_lifetime.get(&runtime_id)?;
        self.snapshot(lifetime, self.rigs.get(&lifetime)?)
    }

    /// The local actor's full-body pose while the main evaluation drives first-person hands.
    pub(crate) fn ui_pose(&self, runtime_id: u64) -> Option<&[BoneTransform]> {
        let state = self.rigs.get(self.runtime_to_lifetime.get(&runtime_id)?)?;
        match state.ui_pose.as_deref() {
            Some([]) => None,
            Some(pose) => Some(pose),
            None => Some(&state.current),
        }
    }

    pub(crate) fn snapshots(&self) -> impl Iterator<Item = ActorRigSnapshot<'_>> {
        self.rigs
            .iter()
            .filter_map(|(&lifetime, state)| self.snapshot(lifetime, state))
    }

    pub(crate) const fn stats(&self) -> ActorAnimationStats {
        self.stats
    }

    fn snapshot<'a>(
        &'a self,
        actor: ActorLifetimeId,
        state: &'a ActorRigState,
    ) -> Option<ActorRigSnapshot<'a>> {
        if state.previous.len() != state.current.len() {
            return None;
        }
        Some(ActorRigSnapshot {
            actor,
            rig: state.rig,
            previous: &state.previous,
            current: &state.current,
            rest: &state.rest,
            rest_completed_tick: if state.rest_reset_pending {
                0
            } else {
                state.rest_completed_tick
            },
            rest_reset_generation: state.rest_reset_generation,
            completed_tick: state.completed_tick,
            reset_generation: state.reset_generation,
            fallback: state.fallback,
            scale: state.scale.map_or_else(
                || {
                    if state.pack {
                        self.pack.as_ref().map(|pack| &pack.assets)
                    } else {
                        self.assets.as_ref()
                    }
                    .and_then(|assets| assets.rig_bindings().get(state.rig_binding))
                    .map_or(1.0, |rig| rig.scale.get())
                },
                |scale| scale[0],
            ),
            axis_scale: state
                .scale
                .map_or([1.0; 3], |scale| [scale[1], scale[2], scale[3]]),
            previous_body_yaw: state.motion.previous_body_yaw,
            body_yaw: state.motion.body_yaw,
            render: &state.render,
            bone_names: state.posed_bone_names(),
            skin_geometry: state
                .skin_skeleton()
                .map(|skeleton| &skeleton.prepared.geometry),
            skin_mesh: state
                .skin_skeleton()
                .and_then(|skeleton| skeleton.prepared.mesh.as_ref()),
            skin_layers: &state.skin_layers,
            hand: state.hand_phases(),
            item_animation: state.hand_phases().map(ItemAnimationState::from),
            off_hand_animation: state.off_hand_animation,
            animation_variables: ActorAnimationVariables::new(
                if state.pack {
                    self.pack.as_ref().map(|pack| pack.assets.as_ref())
                } else {
                    self.assets.as_deref()
                },
                &state.variables,
                state.completed_tick.saturating_sub(state.lifetime_epoch),
            )
            .with_input(state.history.back().copied())
            .with_item_context(Some(&state.query_context), state.complete_spear_variables),
            java: state.java.motion,
            java_equipped: state.java.equipped(),
        })
    }

    fn bump_generation(&mut self) {
        self.next_reset_generation = self.next_reset_generation.saturating_add(1);
    }

    fn take_rest_generation(&mut self) -> Option<u64> {
        let next = self.next_rest_reset_generation.checked_add(1)?;
        let generation = self.next_rest_reset_generation;
        self.next_rest_reset_generation = next;
        Some(generation)
    }
}

/// The rig's authored scale times its largest per-axis scale, bounding its culling box.
fn model_scale(state: &ActorRigState, assets: &RuntimeEntityAssets) -> f32 {
    let scale = state.scale.map_or_else(
        || {
            assets
                .rig_bindings()
                .get(state.rig_binding)
                .map_or(1.0, |rig| rig.scale.get())
        },
        |scale| scale[0],
    );
    let axes = state
        .scale
        .map_or([1.0; 3], |scale| [scale[1], scale[2], scale[3]]);
    scale
        * axes
            .iter()
            .fold(1.0_f32, |largest, axis| largest.max(axis.abs()))
}

fn resolve_rig(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    actor: &ActorSnapshot,
    completed_tick: u64,
) -> Option<ActorRigState> {
    let identifier = match &actor.kind {
        ActorKind::Player { .. } => "minecraft:player",
        ActorKind::Entity { identifier } => identifier,
    };
    let entity_symbol = assets
        .symbol_candidates(EntityAssetKind::Entity, identifier)
        .first()?;
    let entity_symbol_index = assets
        .symbols()
        .iter()
        .position(|symbol| std::ptr::eq(symbol, entity_symbol))?;
    let rig_binding = assets
        .rig_bindings()
        .iter()
        .position(|rig| rig.entity_symbol as usize == entity_symbol_index)?;
    resolve_binding(assets, layout, actor, completed_tick, rig_binding)
}

mod attachable;
mod clock;
pub(crate) mod custom_emotes;
mod evaluation;
mod geometry;
mod horse;
mod hud;
mod java;
mod spear;
pub use java::retarget::JavaRetargetCache;
pub use java::{JavaHeldItem, JavaMotion, java_mounted_body_yaw, java_walked_distance};
mod motion;
mod particles;
mod pose;
pub use particles::ActorParticleController;
mod query;
mod render;
mod render_frame;
pub use render_frame::{ActorRenderFrame, ActorRenderLayers};
mod replay;
mod server_animation;
pub use server_animation::ServerAnimationCompiler;
mod schedule;
mod skin;
mod skin_layers;
mod skin_preparation;
mod tick;
mod view;
pub use attachable::{
    AttachableAnimationInput, AttachableBoneParent, AttachableRigSnapshot, AttachablesRuntime,
};
pub use evaluation::ActorAnimationVariables;
use evaluation::{EngineSlots, Evaluator, MolangValue, MolangVariables, VariableLayout};
use geometry::{collect_controllers, resolve_binding, resolve_bones, skeleton};
pub use motion::ACTOR_SWING_TICKS;
use motion::{MotionInput, MotionState};
pub use pose::MODEL_PART_ORIGIN_Y;
use pose::{compose_pose, sample_clips};
pub use render::RenderTextureLayer;
pub use skin_layers::SkinRenderLayer;
pub(crate) use tick::{ActorTickContext, WornArmor};
use tick::{advance_motion, evaluate_state};
pub use view::ActorAnimationView;

mod local_motion;
pub use local_motion::LocalSwingMotionSample;

#[cfg(test)]
mod local_motion_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod server_tests;

#[cfg(test)]
mod hurt_tests;

#[cfg(test)]
#[path = "actor_animation/crystal_tests.rs"]
mod crystal_tests;

#[cfg(test)]
mod dragon_tests;

#[cfg(test)]
mod creeper_tests;
