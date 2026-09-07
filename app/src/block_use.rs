//! Provisional empty-hand creative block use on a completed movement tick.
//!
//! This intentionally covers only an uncontested keyboard/mouse use edge with
//! a server-known empty selected slot. The server remains authoritative for
//! any resulting container or world state. Broader item-use cadence, optional
//! input-envelope fields, and non-empty-hand behavior remain outside this
//! bounded path.

use std::num::NonZeroU64;

use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Res, ResMut, Resource, Window, With},
    window::PrimaryWindow,
};
use protocol::{
    BlockItemInteraction, BlockUseRequest, NetworkItemStack, PlayerAuthInputInteractions,
    PlayerInputMode, VerifiedNetworkItemStack,
};
use semantic_input::{Action, InputMode};

use crate::{
    local_player::InteractionOriginSnapshot,
    menu::MenuRuntime,
    mining::{FrozenCreativeMining, FrozenMiningSelection, creative_observation},
    movement::{MovementTicker, PhysicsCollisionRegistries},
    runtime::world::ClientWorld,
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::{UiRuntime, inventory_ledger::PlayerInventorySlot},
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenEmptyHandBlockUse {
    pub(crate) observation: FrozenCreativeMining,
}

impl FrozenEmptyHandBlockUse {
    pub(crate) fn from_observation(observation: FrozenCreativeMining) -> Option<Self> {
        (observation.input_mode == PlayerInputMode::Mouse
            && observation.selection.item.network_id() == 0
            && observation.selection.item.count() == 0)
            .then_some(Self { observation })
    }

    fn still_authorized_by(&self, current: &Self) -> bool {
        self.observation.still_authorized_by(&current.observation)
    }

    pub(crate) fn into_tick_payload(self, player_position: [f32; 3]) -> QueuedBlockUseInteraction {
        let target = &self.observation.target;
        let interactions = PlayerAuthInputInteractions {
            block_actions: protocol::BlockActions::new(),
            block_interaction: Some(BlockItemInteraction::Use(BlockUseRequest {
                block_position: target.position,
                face: target.face,
                selected_slot: self.observation.selection.slot,
                selected_item: self.observation.selection.item.clone(),
                player_position,
                relative_hit: target.relative_hit,
                block_runtime_id: u64::from(target.runtime_id),
            })),
        };
        QueuedBlockUseInteraction {
            authority: self,
            interactions,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QueuedBlockUseInteraction {
    authority: FrozenEmptyHandBlockUse,
    pub(crate) interactions: PlayerAuthInputInteractions,
}

impl QueuedBlockUseInteraction {
    pub(crate) fn still_authorized_by(&self, current: &FrozenEmptyHandBlockUse) -> bool {
        self.authority.still_authorized_by(current)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PendingUsePress {
    authority: FrozenEmptyHandBlockUse,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct BlockUseRuntime {
    pending_press: Option<PendingUsePress>,
    position_authority: Option<(u64, u64)>,
}

impl BlockUseRuntime {
    fn synchronize_position_authority(&mut self, ticker: &mut MovementTicker) -> bool {
        let position_authority = ticker.interaction_authority_identity();
        let changed = self
            .position_authority
            .is_some_and(|previous| previous != position_authority);
        self.position_authority = Some(position_authority);
        if changed {
            self.pending_press = None;
            ticker.retain_block_use(None);
        }
        changed
    }

    fn update_press(
        &mut self,
        pressed: bool,
        input_authority_generation: NonZeroU64,
        current: Option<FrozenEmptyHandBlockUse>,
        ticker: &mut MovementTicker,
    ) -> Option<u64> {
        if self.synchronize_position_authority(ticker) {
            return None;
        }
        if self.pending_press.as_ref().is_some_and(|pending| {
            pending
                .authority
                .observation
                .frame
                .input_authority_generation
                != input_authority_generation
        }) {
            self.pending_press = None;
        }
        ticker.retain_block_use(current.as_ref());
        if !ticker.accepts_block_use() {
            self.pending_press = None;
            return None;
        }
        if pressed {
            self.pending_press = current.as_ref().map(|authority| PendingUsePress {
                authority: authority.clone(),
            });
        }
        let pending = self.pending_press.as_ref()?;
        if pending
            .authority
            .observation
            .frame
            .input_authority_generation
            != input_authority_generation
        {
            self.pending_press = None;
            return None;
        }
        let Some(current) = current else {
            self.pending_press = None;
            return None;
        };
        if !pending.authority.still_authorized_by(&current) {
            self.pending_press = None;
            return None;
        }
        let mut attachment = pending.authority.clone();
        attachment.observation.frame.physics_tick = current.observation.frame.physics_tick;
        let attached = ticker.attach_block_use(attachment);
        if attached.is_some() {
            self.pending_press = None;
        }
        attached
    }

    fn clear(&mut self, ticker: &mut MovementTicker) {
        self.pending_press = None;
        ticker.retain_block_use(None);
    }
}

#[derive(SystemParam)]
pub(crate) struct BlockUseContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    origin: Res<'w, InteractionOriginSnapshot>,
    ui: Res<'w, UiRuntime>,
    menu: Res<'w, MenuRuntime>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: Res<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
}

pub(crate) fn produce_empty_hand_block_use(
    context: BlockUseContext,
    mut runtime: ResMut<BlockUseRuntime>,
    mut movement: ResMut<MovementTicker>,
) {
    let position_authority_changed = runtime.synchronize_position_authority(&mut movement);
    if context.menu.is_visible() || !context.windows.single().is_ok_and(|window| window.focused) {
        runtime.clear(&mut movement);
        return;
    }
    let Some(input_snapshot) = context.input.snapshot() else {
        runtime.clear(&mut movement);
        return;
    };
    let use_pressed = !position_authority_changed && context.input.phase(Action::Use).pressed;
    let attack_pressed = context.input.phase(Action::Attack).pressed;
    if use_pressed && attack_pressed {
        runtime.clear(&mut movement);
        return;
    }
    if !use_pressed && runtime.pending_press.is_none() && !movement.has_queued_block_use() {
        return;
    }
    let position_authority_generation = movement.interaction_authority_identity().1;
    let current = block_use_observation(
        &context.origin,
        &context.ui,
        &context.client_world,
        &context.collisions,
        input_snapshot.input_mode,
        (
            input_snapshot.authority_generation,
            input_snapshot.frame_sequence,
        ),
        position_authority_generation,
    );
    let _ = runtime.update_press(
        block_use_edge_authorized(use_pressed, attack_pressed),
        input_snapshot.authority_generation,
        current,
        &mut movement,
    );
}

fn block_use_observation(
    origin: &InteractionOriginSnapshot,
    ui: &UiRuntime,
    client_world: &ClientWorld,
    collisions: &PhysicsCollisionRegistries,
    input_mode: InputMode,
    input_authority: (NonZeroU64, u64),
    position_authority_generation: u64,
) -> Option<FrozenEmptyHandBlockUse> {
    if input_mode != InputMode::KeyboardMouse {
        return None;
    }
    let selection = verified_empty_hand_selection(ui)?;
    let mut observation = creative_observation(
        origin,
        ui,
        client_world,
        collisions,
        input_mode,
        input_authority,
        position_authority_generation,
    )?;
    observation.selection = selection;
    FrozenEmptyHandBlockUse::from_observation(observation)
}

fn verified_empty_hand_selection(ui: &UiRuntime) -> Option<FrozenMiningSelection> {
    if ui.inventory_ledger().pending_request_id().is_some()
        || ui.inventory_ledger().resync_required()
        || ui.pending_hotbar_selection().is_some()
    {
        return None;
    }
    let selected = ui.selected_stack_snapshot()?;
    if !matches!(selected.state, PlayerInventorySlot::Empty) {
        return None;
    }
    let stack = NetworkItemStack::empty();
    let item = VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).ok()?;
    Some(FrozenMiningSelection {
        slot: selected.slot,
        item,
    })
}

pub(crate) const fn block_use_edge_authorized(use_pressed: bool, attack_pressed: bool) -> bool {
    use_pressed && !attack_pressed
}

pub(crate) const fn mining_edge_authorized(attack_pressed: bool, use_pressed: bool) -> bool {
    attack_pressed && !use_pressed
}

#[cfg(test)]
mod tests;
