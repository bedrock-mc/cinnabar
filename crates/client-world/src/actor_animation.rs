use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use assets::{
    CompiledMolangExpression, EntityAnimationInterpolation, EntityAnimationLoop,
    EntityAnimationProperty, EntityAssetKind, EntityGeometryBone, EntityRigFallback, MolangOp,
    RuntimeEntityAssets, validate_entity_geometry_inheritance,
};
use protocol::{ActorKind, ActorMetadataValue, PlayerSkinGeometry};

use crate::actor_store::ActorSnapshot;

pub const MAX_RUNTIME_BONES_PER_RIG: usize = 96;
pub const MAX_CONTROLLER_TRANSITIONS_PER_TICK: usize = 8;
pub const MAX_MOLANG_OPS_PER_ACTOR_TICK: usize = 4_096;
pub const MAX_MOLANG_OPS_PER_WORLD_TICK: usize = 262_144;
pub const MAX_MOLANG_OPS_PER_RENDER_FRAME: usize = 0;
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
}

#[derive(Clone, Copy, Debug)]
pub struct ActorRigSnapshot<'a> {
    pub actor: ActorLifetimeId,
    pub rig: EntityRigId,
    pub geometry_identifier: &'a str,
    pub geometry_sha256: [u8; 32],
    pub previous: &'a [BoneTransform],
    pub current: &'a [BoneTransform],
    pub completed_tick: u64,
    pub reset_generation: u64,
    pub fallback: EntityRigFallback,
    pub texture: Option<ActorRigTextureSnapshot<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct ActorRigTextureSnapshot<'a> {
    pub width: u16,
    pub height: u16,
    pub rgba8: &'a Arc<[u8]>,
}

/// The compiled vanilla player rig used for the StartGame-owned local avatar.
/// Identity and world transform intentionally remain outside this value: they
/// come from the session and local movement authorities at presentation time.
#[derive(Clone, Copy, Debug)]
pub struct LocalPlayerRigSnapshot<'a> {
    pub rig: EntityRigId,
    pub geometry_identifier: &'a str,
    pub geometry_sha256: [u8; 32],
    pub previous: &'a [BoneTransform],
    pub current: &'a [BoneTransform],
    pub completed_tick: u64,
    pub reset_generation: u64,
    pub fallback: EntityRigFallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalPlayerRigResolution {
    MissingVariant,
    GeometryFingerprintMismatch,
    PoseNotReady,
    Ready,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActorAnimationStats {
    pub evaluated_molang_ops: u64,
    pub actor_budget_exhaustions: u64,
    pub world_budget_exhaustions: u64,
    pub frozen_actors: u64,
}

#[derive(Debug)]
pub(crate) struct ActorAnimationStore {
    assets: Option<Arc<RuntimeEntityAssets>>,
    textures: Arc<[ActorRigTexture]>,
    rigs: BTreeMap<ActorLifetimeId, ActorRigState>,
    runtime_to_lifetime: HashMap<u64, ActorLifetimeId>,
    local_players: BTreeMap<Box<str>, ActorRigState>,
    completed_tick: u64,
    next_reset_generation: u64,
    stats: ActorAnimationStats,
}

#[derive(Debug)]
struct ActorRigState {
    rig: EntityRigId,
    geometry_binding: usize,
    bones: Vec<RuntimeBone>,
    controllers: Vec<ControllerState>,
    previous: Vec<BoneTransform>,
    current: Vec<BoneTransform>,
    reset_generation: u64,
    reset_pending: bool,
    lifetime_epoch: u64,
    animation_epoch: u64,
    completed_tick: u64,
    fallback: EntityRigFallback,
    texture: Option<usize>,
    history: VecDeque<ActorTickInput>,
}

#[derive(Debug)]
struct ActorRigTexture {
    width: u16,
    height: u16,
    rgba8: Arc<[u8]>,
}

#[derive(Clone, Debug)]
struct RuntimeBone {
    name: Box<str>,
    parent: Option<usize>,
    pivot: [f32; 3],
    rotation: [f32; 3],
}

#[derive(Clone, Copy, Debug)]
struct ControllerState {
    controller: usize,
    state: u16,
}

#[derive(Clone, Copy, Debug)]
struct ActorTickInput {
    velocity: [f32; 3],
    on_ground: bool,
    body_yaw: f32,
    head_yaw: f32,
    pitch: f32,
}

/// Sanitized fixed-tick authority for the StartGame-owned local player rig.
/// Action state is intentionally absent until an authoritative source exists.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalPlayerAnimationTickInput {
    pub tick: u64,
    pub velocity: [f32; 3],
    pub on_ground: bool,
    pub body_yaw: f32,
    pub head_yaw: f32,
    pub pitch: f32,
}

struct EvaluatedState {
    pose: Vec<BoneTransform>,
    controllers: Vec<ControllerState>,
    history: VecDeque<ActorTickInput>,
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
        let textures = assets
            .as_ref()
            .map(|assets| {
                assets
                    .rig_textures()
                    .iter()
                    .map(|texture| ActorRigTexture {
                        width: texture.width,
                        height: texture.height,
                        rgba8: Arc::from(texture.rgba8.as_ref()),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let local_players = assets.as_ref().map_or_else(BTreeMap::new, |assets| {
            ["geometry.humanoid.custom", "geometry.humanoid.customSlim"]
                .into_iter()
                .filter_map(|identifier| {
                    let mut state = resolve_rig_for_geometry(
                        assets,
                        &local_player_resolution_actor(),
                        1,
                        Some((identifier, None)),
                    )?;
                    state.reset_generation = 1;
                    Some((identifier.into(), state))
                })
                .collect()
        });
        Self {
            assets,
            textures: textures.into(),
            rigs: BTreeMap::new(),
            runtime_to_lifetime: HashMap::new(),
            local_players,
            completed_tick: 0,
            next_reset_generation: 1,
            stats: ActorAnimationStats::default(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.rigs.clear();
        self.runtime_to_lifetime.clear();
        self.completed_tick = 0;
        self.reset_local_player();
        self.bump_generation();
    }

    pub(crate) fn reset_local_player(&mut self) {
        let mut reset_generation = self.next_reset_generation.max(1);
        for state in self.local_players.values_mut() {
            state.previous.clone_from(&state.current);
            state.reset_pending = true;
            state.completed_tick = state.completed_tick.max(1);
            state.reset_generation = reset_generation;
            reset_generation = reset_generation.saturating_add(1);
            state.history.clear();
        }
        self.next_reset_generation = reset_generation;
    }

    pub(crate) fn advance_local_player_tick(&mut self, input: LocalPlayerAnimationTickInput) {
        if !input.velocity.into_iter().all(f32::is_finite)
            || ![input.body_yaw, input.head_yaw, input.pitch]
                .into_iter()
                .all(f32::is_finite)
        {
            self.reset_local_player();
            return;
        }
        let Some(assets) = self.assets.clone() else {
            return;
        };
        let actor = local_player_animation_actor(input);
        let mut world_left = MAX_MOLANG_OPS_PER_WORLD_TICK;
        for state in self.local_players.values_mut() {
            if state.completed_tick != 0 && input.tick != state.completed_tick.saturating_add(1) {
                state.reset_pending = true;
                state.history.clear();
            }
            let mut budget = EvalBudget {
                actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
                world_left: &mut world_left,
                work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
                transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
                used: 0,
            };
            if let Ok(mut evaluated) =
                evaluate_state(&assets, state, &actor, input.tick, &mut budget)
            {
                apply_local_player_view_pose(state, &mut evaluated.pose, input);
                state.controllers = evaluated.controllers;
                state.history = evaluated.history;
                if state.reset_pending {
                    state.previous.clone_from(&evaluated.pose);
                    state.current = evaluated.pose;
                    state.reset_pending = false;
                    state.reset_generation = self.next_reset_generation;
                    self.next_reset_generation = self.next_reset_generation.saturating_add(1);
                    state.lifetime_epoch = input.tick;
                    state.animation_epoch = input.tick;
                } else {
                    state.previous = std::mem::replace(&mut state.current, evaluated.pose);
                }
                state.completed_tick = input.tick;
            } else {
                self.stats.frozen_actors = self.stats.frozen_actors.saturating_add(1);
            }
            self.stats.evaluated_molang_ops = self
                .stats
                .evaluated_molang_ops
                .saturating_add(budget.used as u64);
        }
    }

    pub(crate) fn remove_runtime(&mut self, runtime_id: u64) {
        if let Some(lifetime) = self.runtime_to_lifetime.remove(&runtime_id) {
            self.rigs.remove(&lifetime);
        }
    }

    pub(crate) fn insert(
        &mut self,
        session_id: u64,
        dimension: i32,
        actor: &ActorSnapshot,
        player_geometry: Option<&PlayerSkinGeometry>,
    ) {
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
        let requested_geometry = match &actor.kind {
            ActorKind::Player { .. } => {
                let Some(geometry) = player_geometry else {
                    return;
                };
                Some(geometry)
            }
            ActorKind::Entity { .. } => None,
        };
        let Some(mut state) =
            resolve_rig_for_skin_geometry(&assets, actor, self.completed_tick, requested_geometry)
        else {
            return;
        };
        state.reset_generation = self.next_reset_generation;
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
        }
    }

    pub(crate) fn advance_tick(&mut self, actors: &HashMap<u64, ActorSnapshot>) {
        self.completed_tick = self.completed_tick.saturating_add(1);
        let Some(assets) = self.assets.clone() else {
            return;
        };
        let mut world_left = MAX_MOLANG_OPS_PER_WORLD_TICK;
        let lifetimes = self.rigs.keys().copied().collect::<Vec<_>>();
        for lifetime in lifetimes {
            let Some(actor) = actors.get(&lifetime.runtime_id) else {
                continue;
            };
            let Some(state) = self.rigs.get_mut(&lifetime) else {
                continue;
            };
            if state.fallback == EntityRigFallback::GeometryOnly {
                state.previous.clone_from(&state.current);
                if state.reset_pending {
                    state.reset_pending = false;
                    state.reset_generation = self.next_reset_generation;
                    self.next_reset_generation = self.next_reset_generation.saturating_add(1);
                    state.animation_epoch = self.completed_tick;
                    state.history.clear();
                }
                state.completed_tick = self.completed_tick;
                continue;
            }
            if world_left == 0 {
                self.stats.world_budget_exhaustions =
                    self.stats.world_budget_exhaustions.saturating_add(1);
                self.stats.frozen_actors = self.stats.frozen_actors.saturating_add(1);
                continue;
            }
            let mut budget = EvalBudget {
                actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
                world_left: &mut world_left,
                work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
                transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
                used: 0,
            };
            let result = evaluate_state(&assets, state, actor, self.completed_tick, &mut budget);
            self.stats.evaluated_molang_ops = self
                .stats
                .evaluated_molang_ops
                .saturating_add(budget.used as u64);
            match result {
                Ok(evaluated) => {
                    state.controllers = evaluated.controllers;
                    state.history = evaluated.history;
                    if state.reset_pending {
                        state.previous.clone_from(&evaluated.pose);
                        state.current = evaluated.pose;
                        state.reset_pending = false;
                        state.reset_generation = self.next_reset_generation;
                        self.next_reset_generation = self.next_reset_generation.saturating_add(1);
                        state.animation_epoch = self.completed_tick;
                    } else {
                        state.previous = std::mem::replace(&mut state.current, evaluated.pose);
                    }
                    state.completed_tick = self.completed_tick;
                }
                Err(EvalError::ActorBudget) => {
                    self.stats.actor_budget_exhaustions =
                        self.stats.actor_budget_exhaustions.saturating_add(1);
                    self.stats.frozen_actors = self.stats.frozen_actors.saturating_add(1);
                }
                Err(EvalError::WorldBudget) => {
                    self.stats.world_budget_exhaustions =
                        self.stats.world_budget_exhaustions.saturating_add(1);
                    self.stats.frozen_actors = self.stats.frozen_actors.saturating_add(1);
                }
                Err(EvalError::Invalid) => {
                    self.stats.frozen_actors = self.stats.frozen_actors.saturating_add(1);
                }
            }
        }
    }

    pub(crate) fn get(&self, runtime_id: u64) -> Option<ActorRigSnapshot<'_>> {
        let lifetime = *self.runtime_to_lifetime.get(&runtime_id)?;
        self.snapshot(lifetime, self.rigs.get(&lifetime)?)
    }

    pub(crate) fn snapshots(&self) -> Vec<ActorRigSnapshot<'_>> {
        self.rigs
            .iter()
            .filter_map(|(&lifetime, state)| self.snapshot(lifetime, state))
            .collect()
    }

    pub(crate) fn local_player(
        &self,
        geometry: &PlayerSkinGeometry,
    ) -> Option<LocalPlayerRigSnapshot<'_>> {
        let (identifier, expected_sha256) = match geometry {
            PlayerSkinGeometry::Wide => ("geometry.humanoid.custom", None),
            PlayerSkinGeometry::Slim => ("geometry.humanoid.customSlim", None),
            PlayerSkinGeometry::Custom {
                identifier,
                data_sha256,
            } => (identifier.as_ref(), Some(data_sha256)),
        };
        let state = self.local_players.get(identifier)?;
        let compiled_geometry = self.geometry(state)?;
        if expected_sha256.is_some_and(|expected| &compiled_geometry.semantic_sha256 != expected) {
            return None;
        }
        (state.previous.len() == state.current.len() && !state.previous.is_empty()).then_some(
            LocalPlayerRigSnapshot {
                rig: state.rig,
                geometry_identifier: compiled_geometry.identifier.as_ref(),
                geometry_sha256: compiled_geometry.semantic_sha256,
                previous: &state.previous,
                current: &state.current,
                completed_tick: state.completed_tick,
                reset_generation: state.reset_generation,
                fallback: state.fallback,
            },
        )
    }

    pub(crate) fn local_player_resolution(
        &self,
        geometry: &PlayerSkinGeometry,
    ) -> LocalPlayerRigResolution {
        let (identifier, expected_sha256) = match geometry {
            PlayerSkinGeometry::Wide => ("geometry.humanoid.custom", None),
            PlayerSkinGeometry::Slim => ("geometry.humanoid.customSlim", None),
            PlayerSkinGeometry::Custom {
                identifier,
                data_sha256,
            } => (identifier.as_ref(), Some(data_sha256)),
        };
        let Some(state) = self.local_players.get(identifier) else {
            return LocalPlayerRigResolution::MissingVariant;
        };
        let Some(compiled_geometry) = self.geometry(state) else {
            return LocalPlayerRigResolution::MissingVariant;
        };
        if expected_sha256.is_some_and(|expected| &compiled_geometry.semantic_sha256 != expected) {
            return LocalPlayerRigResolution::GeometryFingerprintMismatch;
        }
        if state.completed_tick == 0
            || state.reset_generation == 0
            || state.previous.is_empty()
            || state.previous.len() != state.current.len()
        {
            LocalPlayerRigResolution::PoseNotReady
        } else {
            LocalPlayerRigResolution::Ready
        }
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
        let geometry = self.geometry(state)?;
        Some(ActorRigSnapshot {
            actor,
            rig: state.rig,
            geometry_identifier: geometry.identifier.as_ref(),
            geometry_sha256: geometry.semantic_sha256,
            previous: &state.previous,
            current: &state.current,
            completed_tick: state.completed_tick,
            reset_generation: state.reset_generation,
            fallback: state.fallback,
            texture: state
                .texture
                .and_then(|texture| self.textures.get(texture))
                .map(|texture| ActorRigTextureSnapshot {
                    width: texture.width,
                    height: texture.height,
                    rgba8: &texture.rgba8,
                }),
        })
    }

    fn bump_generation(&mut self) {
        self.next_reset_generation = self.next_reset_generation.saturating_add(1);
    }

    fn geometry<'a>(&'a self, state: &ActorRigState) -> Option<&'a assets::EntityGeometry> {
        let candidate = self
            .assets
            .as_ref()?
            .rig_geometries()
            .get(state.geometry_binding)?;
        self.assets
            .as_ref()?
            .geometries()
            .get(candidate.geometry as usize)
    }
}

fn apply_local_player_view_pose(
    state: &ActorRigState,
    pose: &mut [BoneTransform],
    input: LocalPlayerAnimationTickInput,
) {
    apply_view_pose_to_bones(&state.bones, pose, input);
}

fn apply_view_pose_to_bones(
    bones: &[RuntimeBone],
    pose: &mut [BoneTransform],
    input: LocalPlayerAnimationTickInput,
) {
    let Some(head) = bones
        .iter()
        .position(|bone| bone.name.eq_ignore_ascii_case("head"))
    else {
        return;
    };
    let relative_yaw = (input.head_yaw - input.body_yaw + 180.0).rem_euclid(360.0) - 180.0;
    let view = quat_from_euler([input.pitch.clamp(-90.0, 90.0), relative_yaw, 0.0]);
    for (index, transform) in pose.iter_mut().enumerate() {
        let mut current = Some(index);
        let mut under_head = false;
        while let Some(bone) = current {
            if bone == head {
                under_head = true;
                break;
            }
            current = bones.get(bone).and_then(|bone| bone.parent);
        }
        if under_head {
            transform.rotation = quat_multiply(view, transform.rotation);
        }
    }
}

mod resolution;
use resolution::*;
fn evaluate_state(
    assets: &RuntimeEntityAssets,
    state: &ActorRigState,
    actor: &ActorSnapshot,
    tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<EvaluatedState, EvalError> {
    let animation_tick = if state.reset_pending {
        0
    } else {
        tick.saturating_sub(state.animation_epoch)
    };
    let life_tick = tick.saturating_sub(state.lifetime_epoch);
    let mut history = if state.reset_pending {
        VecDeque::with_capacity(MAX_ACTOR_ACTION_HISTORY)
    } else {
        state.history.clone()
    };
    if history.len() == MAX_ACTOR_ACTION_HISTORY {
        history.pop_front();
    }
    history.push_back(ActorTickInput {
        velocity: actor.velocity,
        on_ground: actor.on_ground.unwrap_or(false),
        body_yaw: actor.body_yaw,
        head_yaw: actor.head_yaw,
        pitch: actor.pitch,
    });
    let mut controllers = state.controllers.clone();
    if state.reset_pending {
        for runtime in &mut controllers {
            runtime.state = assets
                .controllers()
                .get(runtime.controller)
                .ok_or(EvalError::Invalid)?
                .initial_state;
        }
    }
    let mut weighted_clips = Vec::new();
    let candidate = assets
        .rig_geometries()
        .get(state.geometry_binding)
        .ok_or(EvalError::Invalid)?;
    let direct_first = candidate.first_animation as usize;
    let direct_end = direct_first
        .checked_add(candidate.animation_count as usize)
        .ok_or(EvalError::Invalid)?;
    for binding in assets
        .rig_animations()
        .get(direct_first..direct_end)
        .ok_or(EvalError::Invalid)?
    {
        budget.charge_work()?;
        weighted_clips.push((binding.clip as usize, 1.0));
    }
    for runtime in &mut controllers {
        budget.charge_work()?;
        advance_controller(
            assets,
            runtime,
            actor,
            &history,
            animation_tick,
            life_tick,
            budget,
        )?;
        let controller = assets
            .controllers()
            .get(runtime.controller)
            .ok_or(EvalError::Invalid)?;
        if runtime.state >= controller.state_count {
            return Err(EvalError::Invalid);
        }
        let state_index = controller.first_state as usize + runtime.state as usize;
        let controller_state = assets
            .controller_states()
            .get(state_index)
            .ok_or(EvalError::Invalid)?;
        let first = controller_state.first_animation as usize;
        let end = first
            .checked_add(controller_state.animation_count as usize)
            .ok_or(EvalError::Invalid)?;
        for animation in assets
            .controller_animations()
            .get(first..end)
            .ok_or(EvalError::Invalid)?
        {
            budget.charge_work()?;
            let weight = animation.weight.map_or(Ok(1.0), |expression| {
                evaluate_expression(
                    assets,
                    expression as usize,
                    actor,
                    &history,
                    animation_tick,
                    life_tick,
                    budget,
                )
            })?;
            if weight.is_finite() && weight != 0.0 {
                weighted_clips.push((animation.clip as usize, weight));
            }
        }
    }
    let local = sample_clips(
        assets,
        state.bones.len(),
        &weighted_clips,
        animation_tick,
        budget,
    )?;
    compose_pose(&state.bones, &local)
        .map(|pose| EvaluatedState {
            pose,
            controllers,
            history,
        })
        .ok_or(EvalError::Invalid)
}

fn advance_controller(
    assets: &RuntimeEntityAssets,
    runtime: &mut ControllerState,
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<(), EvalError> {
    let controller = assets
        .controllers()
        .get(runtime.controller)
        .ok_or(EvalError::Invalid)?;
    loop {
        if runtime.state >= controller.state_count {
            return Err(EvalError::Invalid);
        }
        let state_index = controller.first_state as usize + runtime.state as usize;
        let state = assets
            .controller_states()
            .get(state_index)
            .ok_or(EvalError::Invalid)?;
        let first = state.first_transition as usize;
        let end = first
            .checked_add(state.transition_count as usize)
            .ok_or(EvalError::Invalid)?;
        let mut target = None;
        for transition in assets
            .controller_transitions()
            .get(first..end)
            .ok_or(EvalError::Invalid)?
        {
            budget.charge_work()?;
            let condition = evaluate_expression(
                assets,
                transition.condition as usize,
                actor,
                history,
                tick,
                life_tick,
                budget,
            )?;
            if truthy(condition) {
                target = Some(transition.target_state);
                break;
            }
        }
        let Some(target) = target else {
            return Ok(());
        };
        if !budget.take_transition() {
            return Ok(());
        }
        if target >= controller.state_count {
            return Err(EvalError::Invalid);
        }
        if let Some(expression) = state.on_exit {
            evaluate_expression(
                assets,
                expression as usize,
                actor,
                history,
                tick,
                life_tick,
                budget,
            )?;
        }
        runtime.state = target;
        let target_state = assets
            .controller_states()
            .get(controller.first_state as usize + target as usize)
            .ok_or(EvalError::Invalid)?;
        if let Some(expression) = target_state.on_entry {
            evaluate_expression(
                assets,
                expression as usize,
                actor,
                history,
                tick,
                life_tick,
                budget,
            )?;
        }
    }
}

#[derive(Clone, Copy)]
struct LocalDelta {
    translation: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
}

impl Default for LocalDelta {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

fn sample_clips(
    assets: &RuntimeEntityAssets,
    bone_count: usize,
    clips: &[(usize, f32)],
    tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<LocalDelta>, EvalError> {
    let mut local = vec![LocalDelta::default(); bone_count];
    for &(clip_index, weight) in clips {
        budget.charge_work()?;
        let clip = assets
            .animation_clips()
            .get(clip_index)
            .ok_or(EvalError::Invalid)?;
        let length = clip.length_seconds.get();
        let raw_time = tick as f32 * 0.05;
        let time = match clip.loop_mode {
            EntityAnimationLoop::Loop if length > 0.0 => raw_time.rem_euclid(length),
            EntityAnimationLoop::Once | EntityAnimationLoop::HoldOnLastFrame => {
                raw_time.clamp(0.0, length)
            }
            EntityAnimationLoop::Loop => 0.0,
        };
        let first = clip.first_channel as usize;
        let end = first
            .checked_add(clip.channel_count as usize)
            .ok_or(EvalError::Invalid)?;
        for channel in assets
            .animation_channels()
            .get(first..end)
            .ok_or(EvalError::Invalid)?
        {
            budget.charge_work()?;
            let bone = local
                .get_mut(channel.bone as usize)
                .ok_or(EvalError::Invalid)?;
            let value =
                sample_channel(assets, channel.first_keyframe, channel.keyframe_count, time)?;
            match channel.property {
                EntityAnimationProperty::Translation => {
                    for (axis, value) in value.into_iter().enumerate() {
                        bone.translation[axis] += value * weight;
                    }
                }
                EntityAnimationProperty::Rotation => {
                    for (axis, value) in value.into_iter().enumerate() {
                        bone.rotation[axis] += value * weight;
                    }
                }
                EntityAnimationProperty::Scale => {
                    for (axis, value) in value.into_iter().enumerate() {
                        bone.scale[axis] *= 1.0 + (value - 1.0) * weight;
                    }
                }
            }
        }
    }
    Ok(local)
}

fn sample_channel(
    assets: &RuntimeEntityAssets,
    first: u32,
    count: u32,
    time: f32,
) -> Result<[f32; 3], EvalError> {
    let first = first as usize;
    let frames = assets
        .animation_keyframes()
        .get(
            first
                ..first
                    .checked_add(count as usize)
                    .ok_or(EvalError::Invalid)?,
        )
        .ok_or(EvalError::Invalid)?;
    let first_frame = frames.first().ok_or(EvalError::Invalid)?;
    if time < first_frame.time_seconds.get() {
        return Ok(first_frame.value.map(|value| value.get()));
    }
    let exact_end = frames.partition_point(|frame| frame.time_seconds.get() <= time);
    if exact_end > 0 && frames[exact_end - 1].time_seconds.get() == time {
        return Ok(frames[exact_end - 1].value.map(|value| value.get()));
    }
    if exact_end == frames.len() {
        return Ok(frames[frames.len() - 1].value.map(|value| value.get()));
    }
    let left_index = exact_end - 1;
    let right_index = exact_end;
    let left = &frames[left_index];
    let right = &frames[right_index];
    let left_time = left.time_seconds.get();
    let right_time = right.time_seconds.get();
    let amount = ((time - left_time) / (right_time - left_time)).clamp(0.0, 1.0);
    let left_value = left.value.map(|value| value.get());
    let right_value = right.value.map(|value| value.get());
    match left.interpolation {
        EntityAnimationInterpolation::Step => Ok(left_value),
        EntityAnimationInterpolation::Linear => Ok(lerp3(left_value, right_value, amount)),
        EntityAnimationInterpolation::CatmullRom => {
            let previous = frames
                .get(left_index.saturating_sub(1))
                .unwrap_or(left)
                .value
                .map(|value| value.get());
            let next = frames
                .get(right_index + 1)
                .unwrap_or(right)
                .value
                .map(|value| value.get());
            Ok(std::array::from_fn(|axis| {
                catmull(
                    previous[axis],
                    left_value[axis],
                    right_value[axis],
                    next[axis],
                    amount,
                )
            }))
        }
    }
}

mod evaluation;
use evaluation::*;

#[cfg(test)]
mod tests;
