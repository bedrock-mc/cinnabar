use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use assets::{
    CompiledMolangExpression, EntityAnimationInterpolation, EntityAnimationLoop,
    EntityAnimationProperty, EntityAssetKind, EntityGeometryBone, EntityRigFallback,
    ItemActionPhase, MolangOp, RuntimeEntityAssets, validate_entity_geometry_inheritance,
};
use protocol::{ActorActionKind, ActorGameMode, ActorKind, ActorMetadataValue, PlayerSkinGeometry};

use crate::actor_store::ActorSnapshot;
use crate::{
    action::{
        RemoteActionSnapshot, RemoteActionStore, STANDARD_ATTACK_ACTIVE_TICKS,
        STANDARD_ATTACK_RECOVER_TICKS, STANDARD_ATTACK_TOTAL_TICKS,
    },
    item::{ActorItemInput, ActorItemKind, ItemStateStore},
};

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
    pub previous: &'a [BoneTransform],
    pub current: &'a [BoneTransform],
    pub completed_tick: u64,
    pub reset_generation: u64,
    pub fallback: EntityRigFallback,
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
    rigs: BTreeMap<ActorLifetimeId, ActorRigState>,
    runtime_to_lifetime: HashMap<u64, ActorLifetimeId>,
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
    distance_moved: f32,
    hand_bob: f32,
    fallback: EntityRigFallback,
    history: VecDeque<ActorTickInput>,
}

#[derive(Clone, Debug)]
struct RuntimeBone {
    name: Box<str>,
    parent: Option<usize>,
    pivot: [f32; 3],
    rotation: [f32; 3],
    locators: Box<[RuntimeLocator]>,
}

#[derive(Clone, Debug)]
struct RuntimeLocator {
    name: Box<str>,
    offset: [f32; 3],
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
    action: ActorActionInput,
    items: ActorItemInput,
    distance_moved: f32,
    hand_bob: f32,
    riding_y_offset: f32,
    default_bone_pivots: [f32; 8],
    root_locator_offset: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default)]
struct ActorActionInput {
    attack_time: f32,
    item_use_normalized: f32,
    use_item_interval_progress: f32,
    use_item_startup_progress: f32,
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

    pub(crate) fn has_assets(&self) -> bool {
        self.assets.is_some()
    }

    fn new(assets: Option<Arc<RuntimeEntityAssets>>) -> Self {
        Self {
            assets,
            rigs: BTreeMap::new(),
            runtime_to_lifetime: HashMap::new(),
            completed_tick: 0,
            next_reset_generation: 1,
            stats: ActorAnimationStats::default(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.rigs.clear();
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
        self.insert_with_skin(session_id, dimension, actor, None);
    }

    pub(crate) fn insert_with_skin(
        &mut self,
        session_id: u64,
        dimension: i32,
        actor: &ActorSnapshot,
        geometry: Option<&PlayerSkinGeometry>,
    ) {
        let Some(assets) = self.assets.clone() else {
            return;
        };
        let lifetime = ActorLifetimeId {
            session_id,
            dimension,
            runtime_id: actor.runtime_id,
            spawn_revision: actor.spawn_revision,
        };
        let Some(mut state) = resolve_rig(&assets, actor, self.completed_tick, geometry) else {
            return;
        };
        self.remove_runtime(actor.runtime_id);
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

    pub(crate) fn advance_tick(
        &mut self,
        actors: &HashMap<u64, ActorSnapshot>,
        actions: &RemoteActionStore,
        items: &ItemStateStore,
    ) {
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
                    state.distance_moved = 0.0;
                    state.hand_bob = 0.0;
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
            let action = actions.get(lifetime).map(action_input).unwrap_or_default();
            let item_input = items.animation_input(lifetime);
            let riding_y_offset = actor
                .mount_unique_id
                .and_then(|mounted_unique_id| {
                    actors.values().find(|mounted| {
                        mounted.unique_id == mounted_unique_id
                            && matches!(
                                &mounted.kind,
                                ActorKind::Entity { identifier }
                                    if matches!(identifier.strip_prefix("minecraft:"), Some("minecart" | "hopper_minecart" | "tnt_minecart" | "chest_minecart" | "command_block_minecart" | "boat" | "chest_boat" | "strider"))
                            )
                    })
                })
                .map_or(0.0, |_| -3.0);
            if state.reset_pending {
                state.distance_moved = 0.0;
            } else {
                let distance = actor.velocity[0].hypot(actor.velocity[2]) * 0.05;
                if distance.is_finite() {
                    state.distance_moved = (state.distance_moved + distance).max(0.0);
                }
            }
            let result = evaluate_state(
                &assets,
                state,
                actor,
                action,
                item_input,
                riding_y_offset,
                self.completed_tick,
                &mut budget,
            );
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
            completed_tick: state.completed_tick,
            reset_generation: state.reset_generation,
            fallback: state.fallback,
        })
    }

    fn bump_generation(&mut self) {
        self.next_reset_generation = self.next_reset_generation.saturating_add(1);
    }
}

fn resolve_rig(
    assets: &RuntimeEntityAssets,
    actor: &ActorSnapshot,
    completed_tick: u64,
    requested_geometry: Option<&PlayerSkinGeometry>,
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
    let rig = assets
        .rig_bindings()
        .iter()
        .find(|rig| rig.entity_symbol as usize == entity_symbol_index)?;
    let first = rig.first_geometry as usize;
    let end = first.checked_add(rig.geometry_count as usize)?;
    let candidates = assets.rig_geometries().get(first..end)?;
    let mut world_left = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    let mut budget = EvalBudget {
        actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
        world_left: &mut world_left,
        work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
        transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
        used: 0,
    };
    let requested_identifier = requested_geometry.map(|geometry| match geometry {
        PlayerSkinGeometry::Wide => "geometry.humanoid.custom",
        PlayerSkinGeometry::Slim => "geometry.humanoid.customSlim",
        PlayerSkinGeometry::Custom { identifier, .. } => identifier.as_ref(),
    });
    let exact_match = requested_identifier.and_then(|identifier| {
        let mut matches = candidates
            .iter()
            .enumerate()
            .filter_map(|(offset, candidate)| {
                assets
                    .geometries()
                    .get(candidate.geometry as usize)
                    .is_some_and(|geometry| geometry.identifier.as_ref() == identifier)
                    .then_some(offset)
            });
        let selected = matches.next()?;
        matches.next().is_none().then_some(selected)
    });
    // A skin carrier can refer to geometry that is not present in this
    // animation bundle. Keep the actor on a valid rig in that case instead
    // of dropping its animation state; the visual geometry carrier remains
    // available to the renderer for a later geometry-specific path.
    let mut candidate_offset = exact_match.unwrap_or(0);
    let empty_history = VecDeque::new();
    if requested_geometry.is_none() || exact_match.is_none() {
        for (offset, candidate) in candidates.iter().enumerate().skip(1) {
            let Some(condition) = candidate.condition else {
                continue;
            };
            let Ok(selected) = evaluate_expression(
                assets,
                condition as usize,
                actor,
                &empty_history,
                0,
                0,
                &mut budget,
            ) else {
                continue;
            };
            if truthy(selected) {
                candidate_offset = offset;
                break;
            }
        }
    }
    let geometry_binding = first + candidate_offset;
    let candidate = &assets.rig_geometries()[geometry_binding];
    if candidate.animation_count as usize + candidate.controller_count as usize
        > MAX_RUNTIME_BINDINGS_PER_RIG
    {
        return None;
    }
    let bones = resolve_bones(assets, candidate.geometry as usize)?;
    let current = compose_pose(&bones, &[])?;
    let controller_first = candidate.first_controller as usize;
    let controller_end = controller_first.checked_add(candidate.controller_count as usize)?;
    let controllers = assets
        .rig_controllers()
        .get(controller_first..controller_end)?
        .iter()
        .map(|binding| {
            let controller = assets.controllers().get(binding.controller as usize)?;
            Some(ControllerState {
                controller: binding.controller as usize,
                state: controller.initial_state,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ActorRigState {
        // The renderer needs the resolved geometry candidate, not only the
        // entity-level binding that may contain several candidates.
        rig: EntityRigId(geometry_binding as u32),
        geometry_binding,
        bones,
        controllers,
        previous: current.clone(),
        current,
        reset_generation: 0,
        reset_pending: false,
        lifetime_epoch: completed_tick,
        animation_epoch: completed_tick,
        completed_tick,
        distance_moved: 0.0,
        hand_bob: 0.0,
        fallback: rig.fallback,
        history: VecDeque::with_capacity(MAX_ACTOR_ACTION_HISTORY),
    })
}

fn action_input(action: &RemoteActionSnapshot) -> ActorActionInput {
    let attack_time = match &action.kind {
        ActorActionKind::SwingArm
        | ActorActionKind::CriticalHit
        | ActorActionKind::MagicCriticalHit
        | ActorActionKind::Custom { .. } => match action.phase {
            ItemActionPhase::Windup { .. } => 0.0,
            ItemActionPhase::Active { elapsed_ticks } => {
                f32::from(
                    elapsed_ticks
                        .saturating_add(1)
                        .min(STANDARD_ATTACK_ACTIVE_TICKS),
                ) / f32::from(STANDARD_ATTACK_TOTAL_TICKS)
            }
            ItemActionPhase::Recover { elapsed_ticks } => {
                f32::from(
                    STANDARD_ATTACK_ACTIVE_TICKS
                        .saturating_add(elapsed_ticks.saturating_add(1))
                        .min(STANDARD_ATTACK_ACTIVE_TICKS + STANDARD_ATTACK_RECOVER_TICKS),
                ) / f32::from(STANDARD_ATTACK_TOTAL_TICKS)
            }
            ItemActionPhase::Idle
            | ItemActionPhase::Cancelled
            | ItemActionPhase::UseHeld { .. } => -1.0,
        },
        ActorActionKind::Wake
        | ActorActionKind::RowRight
        | ActorActionKind::RowLeft
        | ActorActionKind::Ignored { .. } => -1.0,
    };
    let item_use_normalized = match action.phase {
        ItemActionPhase::UseHeld {
            elapsed_ticks,
            duration_ticks,
        } if duration_ticks > 0 => {
            f32::from(elapsed_ticks.min(duration_ticks)) / f32::from(duration_ticks)
        }
        _ => 0.0,
    };
    ActorActionInput {
        attack_time,
        item_use_normalized,
        use_item_interval_progress: item_use_normalized,
        use_item_startup_progress: item_use_normalized.min(1.0),
    }
}

fn resolve_bones(assets: &RuntimeEntityAssets, geometry_index: usize) -> Option<Vec<RuntimeBone>> {
    let parents = validate_entity_geometry_inheritance(assets.geometries()).ok()?;
    let mut chain = Vec::new();
    let mut current = geometry_index;
    for _ in 0..=parents.len() {
        chain.push(current);
        let Some(parent) = parents.get(current).copied().flatten() else {
            break;
        };
        current = parent;
    }
    if chain
        .last()
        .and_then(|index| parents.get(*index))
        .copied()
        .flatten()
        .is_some()
    {
        return None;
    }
    chain.reverse();
    let mut merged: Vec<EntityGeometryBone> = Vec::new();
    for index in chain {
        for child in assets.geometries().get(index)?.bones.iter() {
            if let Some(existing) = merged
                .iter_mut()
                .find(|bone| bone.name.eq_ignore_ascii_case(&child.name))
            {
                overlay_bone(existing, child);
            } else {
                merged.push(child.clone());
            }
        }
    }
    if merged.len() > MAX_RUNTIME_BONES_PER_RIG {
        return None;
    }
    merged
        .iter()
        .map(|bone| {
            let parent = bone.parent.as_ref().map(|name| {
                merged
                    .iter()
                    .position(|candidate| candidate.name.eq_ignore_ascii_case(name))
            });
            Some(RuntimeBone {
                name: bone.name.clone(),
                parent: match parent {
                    Some(Some(index)) => Some(index),
                    Some(None) => return None,
                    None => None,
                },
                pivot: scalars(bone.pivot.as_ref()),
                rotation: scalars(bone.rotation.as_ref()),
                locators: bone
                    .locators
                    .iter()
                    .map(|locator| RuntimeLocator {
                        name: locator.name.clone(),
                        offset: locator.offset.map(|value| value.get()),
                    })
                    .collect(),
            })
        })
        .collect()
}

fn overlay_bone(base: &mut EntityGeometryBone, child: &EntityGeometryBone) {
    if child.parent.is_some() {
        base.parent.clone_from(&child.parent);
    }
    if child.pivot.is_some() {
        base.pivot = child.pivot;
    }
    if child.rotation.is_some() {
        base.rotation = child.rotation;
    }
    if child.mirror.is_some() {
        base.mirror = child.mirror;
    }
    if child.inflate.is_some() {
        base.inflate = child.inflate;
    }
    if child.never_render.is_some() {
        base.never_render = child.never_render;
    }
    if child.reset.is_some() {
        base.reset = child.reset;
    }
    if !child.locators.is_empty() {
        base.locators.clone_from(&child.locators);
    }
    if !child.cubes.is_empty() {
        base.cubes.clone_from(&child.cubes);
    }
}

fn scalars(values: Option<&[assets::EntityGeometryScalar; 3]>) -> [f32; 3] {
    values.map_or([0.0; 3], |values| values.map(|value| value.get()))
}

fn default_bone_pivots(bones: &[RuntimeBone]) -> [f32; 8] {
    const NAMES: [&str; 4] = ["rightarm", "leftarm", "rightitem", "leftitem"];
    let mut pivots = [0.0; 8];
    for (index, name) in NAMES.into_iter().enumerate() {
        if let Some(bone) = bones
            .iter()
            .find(|bone| bone.name.eq_ignore_ascii_case(name))
        {
            pivots[index * 2] = bone.pivot[1];
            pivots[index * 2 + 1] = bone.pivot[2];
        }
    }
    pivots
}

fn root_locator_offset(bones: &[RuntimeBone]) -> [f32; 3] {
    bones
        .iter()
        .flat_map(|bone| bone.locators.iter())
        .find(|locator| {
            locator
                .name
                .eq_ignore_ascii_case("armor_offset.default_neck")
        })
        .map_or([0.0; 3], |locator| locator.offset)
}

mod evaluation;
use evaluation::*;
mod pose;
use pose::{LocalDelta, evaluate_state};

#[cfg(test)]
mod tests;
