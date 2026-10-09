//! Category cooldown values for the published hotbar.

use crate::player_runtime::PlayerRuntime;
use chunk_pipeline::WorldStream;
use client_ui::ui_runtime::UiRuntime;
use gameplay::item_use::ItemUseRuntime;

/// Resolves each displayed hotbar stack's remaining category cooldown.
pub(super) fn hotbar_cooldowns(
    player_runtime: &PlayerRuntime,
    runtime: &UiRuntime,
    stream: Option<&WorldStream>,
    item_use: &ItemUseRuntime,
    tick: u64,
) -> [f32; protocol::HOTBAR_SLOT_COUNT as usize] {
    std::array::from_fn(|slot| {
        let Some(stream) = stream else {
            return 0.0;
        };
        let Some(cooldown) = runtime
            .inventory_ledger(player_runtime)
            .displayed_stack(slot as u8)
            .and_then(|stack| stream.authority().canonical_item_stack(stack))
            .and_then(|item| item.identifier)
            .and_then(|identifier| gameplay::item_use::classify::item_cooldown(&identifier))
        else {
            return 0.0;
        };
        item_use.cooldown_progress(cooldown, tick)
    })
}

#[cfg(test)]
mod tests;
