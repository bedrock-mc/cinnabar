//! Projects local player authority into a bounded read-only component payload.

use client_ui::ui_runtime::UiRuntime;
use client_world::WorldAuthority;
use inventory::{
    PlayerInventorySlot,
    inventory_ledger::{InventoryTarget, PLAYER_INVENTORY_SLOT_COUNT, StackResponseOverlay},
};
use mod_host::{PlayerStateEffect, PlayerStateItem, PlayerStateSlot, PlayerStateSnapshot};

/// Capture one connected session; no data survives an owner/session mismatch.
pub(super) fn snapshot(
    authority: &WorldAuthority,
    session: u64,
    player: &player_state::PlayerState,
    ui: &UiRuntime,
    now_millis: u64,
) -> Option<PlayerStateSnapshot> {
    if session == 0 || ui.session_id() != session || player.facts.session_id() != session {
        return None;
    }
    let ledger = player.inventory.ledger();
    let selected = player.selected_stack_snapshot();
    let inventory = (0..PLAYER_INVENTORY_SLOT_COUNT)
        .map(|slot| {
            let state = selected
                .as_ref()
                .filter(|selected| usize::from(selected.slot) == slot)
                .map_or_else(
                    || ledger.slot_state(slot as u8).unwrap(),
                    |selected| selected.state,
                );
            project_slot(
                authority,
                ledger,
                state,
                ledger.presented_slot_overlay(slot as u8),
            )
        })
        .collect();
    let armor = (0..4)
        .map(|slot| {
            let target = InventoryTarget::Armor(slot);
            project_slot(
                authority,
                ledger,
                ledger.gear_slot_state(target).unwrap(),
                ledger.presented_target_overlay(target),
            )
        })
        .collect();
    let offhand_state = ledger.gear_slot_state(InventoryTarget::Offhand).unwrap();
    let offhand_state = match offhand_state {
        PlayerInventorySlot::Unknown => match ui.gameplay_hud().offhand_is_empty() {
            Some(true) => PlayerInventorySlot::Empty,
            Some(false) => PlayerInventorySlot::Present(ui.gameplay_hud().offhand_stack()?),
            None => PlayerInventorySlot::Unknown,
        },
        state => state,
    };
    let offhand = project_slot(
        authority,
        ledger,
        offhand_state,
        ledger.presented_target_overlay(InventoryTarget::Offhand),
    );
    let now_tick = ui.estimated_server_tick(now_millis);
    let mut effects: Vec<_> = ui
        .gameplay_hud()
        .effects()
        .iter()
        .filter(|effect| effect.visible_at_tick(now_tick))
        .map(|effect| PlayerStateEffect {
            effect_id: effect.effect_id,
            amplifier: effect.amplifier,
            remaining_ticks: effect.remaining_ticks(now_tick),
            ambient: effect.ambient,
            particles: effect.particles,
        })
        .collect();
    effects.sort_by_key(|effect| effect.effect_id);
    effects.truncate(mod_api::MAX_PLAYER_STATE_EFFECTS);
    Some(PlayerStateSnapshot {
        session,
        dimension: authority.current_dimension(),
        selected_slot: player.selected_hotbar_slot(),
        inventory,
        armor,
        offhand,
        effects,
    })
}

fn project_slot(
    authority: &WorldAuthority,
    ledger: &inventory::PlayerInventoryLedger,
    state: PlayerInventorySlot<'_>,
    overlay: Option<&StackResponseOverlay>,
) -> PlayerStateSlot {
    match state {
        PlayerInventorySlot::Unknown => PlayerStateSlot {
            known: false,
            item: None,
        },
        PlayerInventorySlot::Empty => PlayerStateSlot {
            known: true,
            item: None,
        },
        PlayerInventorySlot::Present(stack) => {
            let item = project_item(authority, ledger, stack, overlay);
            PlayerStateSlot {
                known: item.is_some(),
                item,
            }
        }
    }
}

fn project_item(
    authority: &WorldAuthority,
    ledger: &inventory::PlayerInventoryLedger,
    stack: &protocol::NetworkItemStack,
    overlay: Option<&StackResponseOverlay>,
) -> Option<PlayerStateItem> {
    let canonical = authority.canonical_item_stack(stack)?;
    let identifier = canonical.identifier.filter(|identifier| {
        identifier.len() <= mod_api::MAX_ITEM_IDENTIFIER_BYTES
            && !identifier.chars().any(char::is_control)
    });
    let max_durability = ledger
        .negotiated_item_entry(stack.network_id)
        .is_none_or(|entry| !entry.component_based)
        .then(|| {
            identifier
                .as_deref()
                .and_then(client_world::vanilla_max_durability)
        })
        .flatten();
    let damage = overlay
        .and_then(|overlay| overlay.durability_correction)
        .and_then(|damage| u32::try_from(damage).ok())
        .or(canonical.damage);
    Some(PlayerStateItem {
        identifier: identifier.map(|identifier| identifier.to_string()),
        network_id: stack.network_id,
        metadata: stack.metadata,
        count: stack.count,
        block: matches!(
            canonical.visual,
            assets::ItemVisualRoute::BlockItem(_) | assets::ItemVisualRoute::RetainedBlock { .. }
        ),
        damage,
        max_durability,
    })
}

#[cfg(test)]
mod tests;
