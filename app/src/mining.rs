//! Creative block-mining production and immutable tick attachment.

use std::num::NonZeroU64;

use bevy::prelude::{Res, ResMut, Resource};
use protocol::{
    BlockAction, BlockActionKind, BlockActions, BlockUseRequest, PlayerAuthInputInteractions,
    PlayerInputMode, VerifiedNetworkItemStack,
};
use semantic_input::{Action, InputMode};
use sim::{BlockHit, PaletteWorld, Vec3, WorldCollisionIdentity};

use crate::{
    local_player::{FrozenInteractionOrigin, InteractionOriginSnapshot},
    movement::{MovementTicker, PhysicsCollisionRegistries},
    runtime::world::ClientWorld,
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::{UiRuntime, inventory_ledger::PlayerInventorySlot},
};

/// Creative pick ranges observed for the three input modes the app exposes.
/// They are deliberately isolated here; survival timing and reach remain out
/// of this first interaction slice until their complete authority exists.
const CREATIVE_MOUSE_REACH_BLOCKS: f64 = 5.7;
const CREATIVE_GAMEPAD_REACH_BLOCKS: f64 = 5.6;
const CREATIVE_TOUCH_REACH_BLOCKS: f64 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreativeMiningAbility {
    InstantBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrozenMiningFrame {
    pub(crate) session_generation: u64,
    pub(crate) position_authority_generation: u64,
    pub(crate) fifo_sequence: u64,
    pub(crate) physics_tick: u64,
    pub(crate) pose_generation: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenMiningRay {
    pub(crate) origin: [f32; 3],
    pub(crate) direction: [f32; 3],
    pub(crate) movement_world_identity: WorldCollisionIdentity,
    pub(crate) world_identity: WorldCollisionIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrozenMiningSelection {
    pub(crate) slot: u8,
    pub(crate) item: VerifiedNetworkItemStack,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenMiningTarget {
    pub(crate) position: [i32; 3],
    pub(crate) face: u8,
    pub(crate) relative_hit: [f32; 3],
    pub(crate) runtime_id: u32,
    pub(crate) identity: WorldCollisionIdentity,
}

/// Complete immutable authority behind one creative break candidate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenCreativeMining {
    pub(crate) frame: FrozenMiningFrame,
    pub(crate) ray: FrozenMiningRay,
    pub(crate) reach: f64,
    pub(crate) input_mode: PlayerInputMode,
    pub(crate) ability: CreativeMiningAbility,
    pub(crate) selection: FrozenMiningSelection,
    pub(crate) target: FrozenMiningTarget,
}

impl FrozenCreativeMining {
    fn still_authorized_by(&self, current: &Self) -> bool {
        self.frame.session_generation == current.frame.session_generation
            && self.frame.position_authority_generation
                == current.frame.position_authority_generation
            && self.frame.fifo_sequence <= current.frame.fifo_sequence
            && self.frame.physics_tick <= current.frame.physics_tick
            && self.frame.pose_generation <= current.frame.pose_generation
            && self.ray.origin.into_iter().all(f32::is_finite)
            && self.ray.direction.into_iter().all(f32::is_finite)
            && self.ray.movement_world_identity == current.ray.movement_world_identity
            && self.ray.world_identity == current.ray.world_identity
            && self.reach == current.reach
            && self.input_mode == current.input_mode
            && self.ability == current.ability
            && self.selection == current.selection
            && self.target.position == current.target.position
            && self.target.face == current.target.face
            && self.target.runtime_id == current.target.runtime_id
            && self.target.identity == current.target.identity
    }

    pub(crate) fn into_tick_payload(self, player_position: [f32; 3]) -> QueuedMiningInteraction {
        let target = &self.target;
        let mut block_actions = BlockActions::new();
        for kind in [
            BlockActionKind::StartDestroy,
            BlockActionKind::PredictDestroy,
        ] {
            block_actions
                .push(BlockAction {
                    kind,
                    position: target.position,
                    face: target.face,
                })
                .expect("one creative break uses two of eight bounded block actions");
        }
        let interactions = PlayerAuthInputInteractions {
            block_actions,
            block_destroy: Some(BlockUseRequest {
                block_position: target.position,
                face: target.face,
                selected_slot: self.selection.slot,
                selected_item: self.selection.item.clone(),
                player_position,
                relative_hit: target.relative_hit,
                block_runtime_id: u64::from(target.runtime_id),
            }),
        };
        QueuedMiningInteraction {
            authority: self,
            interactions,
        }
    }
}

/// Concrete payload retained on a completed movement tick through retries.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QueuedMiningInteraction {
    authority: FrozenCreativeMining,
    pub(crate) interactions: PlayerAuthInputInteractions,
}

impl QueuedMiningInteraction {
    pub(crate) fn still_authorized_by(&self, current: &FrozenCreativeMining) -> bool {
        self.authority.still_authorized_by(current)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingAttackPress {
    input_authority_generation: NonZeroU64,
}

/// One bounded attack edge waiting for an exact completed physics tick.
#[derive(Resource, Debug, Default)]
pub(crate) struct MiningRuntime {
    pending_press: Option<PendingAttackPress>,
    position_authority: Option<(u64, u64)>,
}

impl MiningRuntime {
    fn synchronize_position_authority(&mut self, ticker: &mut MovementTicker) -> bool {
        let position_authority = ticker.mining_authority_identity();
        let changed = self
            .position_authority
            .is_some_and(|previous| previous != position_authority);
        self.position_authority = Some(position_authority);
        if changed {
            self.pending_press = None;
            ticker.retain_creative_mining(None);
        }
        changed
    }

    fn update_press(
        &mut self,
        pressed: bool,
        input_authority_generation: NonZeroU64,
        current: Option<FrozenCreativeMining>,
        ticker: &mut MovementTicker,
    ) -> Option<u64> {
        if self.synchronize_position_authority(ticker) {
            return None;
        }
        if self
            .pending_press
            .is_some_and(|pending| pending.input_authority_generation != input_authority_generation)
        {
            self.pending_press = None;
        }
        if pressed {
            self.pending_press = Some(PendingAttackPress {
                input_authority_generation,
            });
        }

        ticker.retain_creative_mining(current.as_ref());
        let pending = self.pending_press?;
        if pending.input_authority_generation != input_authority_generation {
            self.pending_press = None;
            return None;
        }
        let Some(current) = current else {
            self.pending_press = None;
            return None;
        };
        let attached = ticker.attach_creative_mining(current);
        if attached.is_some() {
            self.pending_press = None;
        }
        attached
    }

    #[cfg(test)]
    const fn has_pending_press(&self) -> bool {
        self.pending_press.is_some()
    }
}

/// Produces at most one instant creative break per fresh attack press.
///
/// This runs after committed world publication and immediately before the
/// movement flush. It never mutates the local world; inbound server block
/// updates remain the sole block-change authority.
pub(crate) fn produce_creative_mining(
    input: Res<SemanticInputSnapshot>,
    origin: Res<InteractionOriginSnapshot>,
    ui: Res<UiRuntime>,
    client_world: Res<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    mut runtime: ResMut<MiningRuntime>,
    mut movement: ResMut<MovementTicker>,
) {
    let position_authority_changed = runtime.synchronize_position_authority(&mut movement);
    let Some(input_snapshot) = input.snapshot() else {
        runtime.pending_press = None;
        movement.retain_creative_mining(None);
        return;
    };
    let attack_pressed = !position_authority_changed && input.phase(Action::Attack).pressed;
    if !attack_pressed && runtime.pending_press.is_none() && !movement.has_queued_creative_mining()
    {
        return;
    }
    let position_authority_generation = movement.mining_authority_identity().1;
    let current = creative_observation(
        &origin,
        &ui,
        &client_world,
        &collisions,
        input_snapshot.input_mode,
        position_authority_generation,
    );
    let _ = runtime.update_press(
        attack_pressed,
        input_snapshot.authority_generation,
        current,
        &mut movement,
    );
}

fn creative_observation(
    origin: &InteractionOriginSnapshot,
    ui: &UiRuntime,
    client_world: &ClientWorld,
    collisions: &PhysicsCollisionRegistries,
    input_mode: InputMode,
    position_authority_generation: u64,
) -> Option<FrozenCreativeMining> {
    let ability = creative_mining_ui_ability(ui.ui_focused(), ui.player_game_mode()?)?;
    let ray = origin.outbound_ray()?;
    let stream = client_world.stream.as_ref()?;
    if ray.session_generation() != ui.session_id()
        || ray.session_generation() != stream.actor_session_id()
    {
        return None;
    }
    if stream.committed_sequence() != ray.fifo_sequence() {
        return None;
    }
    let input_mode = protocol_input_mode(input_mode);
    let reach = creative_reach(input_mode);
    let registry = collisions.registry(stream.network_id_mode());
    let world = PaletteWorld::new(
        stream.collision_store(),
        registry,
        stream.current_dimension(),
    );
    let hit = world
        .block_interaction_ray_current(sim_vec(ray.origin()), sim_vec(ray.direction()), reach)
        .ok()??;
    let ray_world_identity = hit.identity.clone();
    let selection = verified_selection(ui)?;
    Some(frozen_observation(
        ray,
        input_mode,
        reach,
        selection,
        (ray_world_identity, hit),
        position_authority_generation,
        ability,
    ))
}

fn verified_selection(ui: &UiRuntime) -> Option<FrozenMiningSelection> {
    let selected = ui.selected_stack_snapshot()?;
    let stack = match selected.state {
        PlayerInventorySlot::Unknown => return None,
        PlayerInventorySlot::Empty => protocol::NetworkItemStack::empty(),
        PlayerInventorySlot::Present(stack) => stack.clone(),
    };
    let item = VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).ok()?;
    Some(FrozenMiningSelection {
        slot: selected.slot,
        item,
    })
}

fn frozen_observation(
    ray: &FrozenInteractionOrigin,
    input_mode: PlayerInputMode,
    reach: f64,
    selection: FrozenMiningSelection,
    ray_hit: (WorldCollisionIdentity, BlockHit),
    position_authority_generation: u64,
    ability: CreativeMiningAbility,
) -> FrozenCreativeMining {
    let (ray_world_identity, hit) = ray_hit;
    FrozenCreativeMining {
        frame: FrozenMiningFrame {
            session_generation: ray.session_generation(),
            position_authority_generation,
            fifo_sequence: ray.fifo_sequence(),
            physics_tick: ray.physics_tick(),
            pose_generation: ray.pose_generation(),
        },
        ray: FrozenMiningRay {
            origin: ray.origin().to_array(),
            direction: ray.direction().to_array(),
            movement_world_identity: ray.world_collision_identity().clone(),
            world_identity: ray_world_identity,
        },
        reach,
        input_mode,
        ability,
        selection,
        target: FrozenMiningTarget {
            position: hit.block_pos,
            face: hit.face,
            relative_hit: [
                hit.hit_local.x as f32,
                hit.hit_local.y as f32,
                hit.hit_local.z as f32,
            ],
            runtime_id: hit.runtime_id,
            identity: hit.identity,
        },
    }
}

const fn creative_mining_ability(
    game_mode: protocol::PlayerGameMode,
) -> Option<CreativeMiningAbility> {
    match game_mode {
        protocol::PlayerGameMode::Creative => Some(CreativeMiningAbility::InstantBreak),
        protocol::PlayerGameMode::Survival
        | protocol::PlayerGameMode::Adventure
        | protocol::PlayerGameMode::Spectator
        | protocol::PlayerGameMode::Unknown => None,
    }
}

const fn creative_mining_ui_ability(
    ui_focused: bool,
    game_mode: protocol::PlayerGameMode,
) -> Option<CreativeMiningAbility> {
    if ui_focused {
        None
    } else {
        creative_mining_ability(game_mode)
    }
}

const fn protocol_input_mode(input_mode: InputMode) -> PlayerInputMode {
    match input_mode {
        InputMode::KeyboardMouse => PlayerInputMode::Mouse,
        InputMode::GamePad => PlayerInputMode::GamePad,
        InputMode::Touch => PlayerInputMode::Touch,
    }
}

const fn creative_reach(input_mode: PlayerInputMode) -> f64 {
    match input_mode {
        PlayerInputMode::Mouse => CREATIVE_MOUSE_REACH_BLOCKS,
        PlayerInputMode::GamePad => CREATIVE_GAMEPAD_REACH_BLOCKS,
        PlayerInputMode::Touch => CREATIVE_TOUCH_REACH_BLOCKS,
    }
}

fn sim_vec(value: bevy::prelude::Vec3) -> Vec3 {
    Vec3::new(f64::from(value.x), f64::from(value.y), f64::from(value.z))
}

#[cfg(test)]
mod tests;
