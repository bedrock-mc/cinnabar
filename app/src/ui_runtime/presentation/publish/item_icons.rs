//! Stack-aware icons: native CrossbowItem::getAnimationFrame (09a137a0) feeds
//! getIcon (09a13640), whose nonzero frame N selects crossbow_pulling variant N-1.

use super::*;

#[derive(Default)]
pub(super) struct ItemIconFrames([Option<u32>; protocol::HOTBAR_SLOT_COUNT as usize]);

impl ItemIconFrames {
    pub(super) fn capture(
        player_runtime: &crate::player_runtime::PlayerRuntime,
        item_use: Option<(&crate::item_use::ItemUseRuntime, u64)>,
        stream: Option<&client_world::WorldStream>,
        runtime: &UiRuntime,
    ) -> Self {
        let Some(((item_use, tick), stream)) = item_use.zip(stream) else {
            return Self::default();
        };
        Self(std::array::from_fn(|slot| {
            item_use.inventory_animation_frame(player_runtime, stream, runtime, slot as u8, tick)
        }))
    }

    pub(super) fn slot(&self, slot: u8) -> Option<u32> {
        self.0.get(usize::from(slot)).copied().flatten()
    }
}

/// Loaded NBT applies to every cell, including storage, offhand and cursor.
/// Only the matching player slot receives the local charge/fire prediction.
pub(super) fn stack_icon(
    runtime: &UiRuntime,
    presentation: &UiPresentationRuntime,
    stack: &protocol::NetworkItemStack,
    identifier: &str,
    animation_frame: Option<u32>,
) -> Option<IconRef> {
    let projectile = protocol::item_charged_projectile(&stack.extra_data);
    let (icon_identifier, variant) = UiPresentationRuntime::item_icon_key(
        identifier,
        stack.metadata,
        projectile.as_deref(),
        animation_frame,
    );
    let icon = presentation.item_icon(icon_identifier, variant)?;
    Some(icon.with_glint(runtime.item_glint(stack, identifier)))
}

#[cfg(test)]
mod tests;
