//! Stack-aware icons: a crossbow's nonzero animation frame N selects
//! crossbow_pulling variant N-1.

use {super::*, ui::IconRef};

#[derive(Default)]
pub struct ItemIconFrames(pub [Option<u32>; protocol::HOTBAR_SLOT_COUNT as usize]);

impl ItemIconFrames {
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

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use {super::*, ui::IconRef};
    /// Resolves a stack icon for app item-use integration tests.
    pub fn stack_icon(
        runtime: &UiRuntime,
        presentation: &UiPresentationRuntime,
        stack: &protocol::NetworkItemStack,
        identifier: &str,
        animation_frame: Option<u32>,
    ) -> Option<IconRef> {
        super::stack_icon(runtime, presentation, stack, identifier, animation_frame)
    }
}
