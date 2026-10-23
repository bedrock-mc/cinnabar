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
    item: client_world::CanonicalItemStack,
    animation_frame: Option<u32>,
) -> Option<IconRef> {
    let identifier = item.identifier.as_deref()?;
    let (icon_identifier, variant) = presentation
        .dial_icon_key(identifier, stack.metadata)
        .unwrap_or_else(|| {
            UiPresentationRuntime::item_icon_key(
                identifier,
                stack.metadata,
                item.use_animation,
                item.charged_projectile_role,
                animation_frame,
            )
        });
    let icon = runtime
        .stack_facts(stack)
        .custom_color
        .and_then(|rgb| presentation.color_mask_icon(icon_identifier, variant, rgb))
        .or_else(|| presentation.item_icon(icon_identifier, variant))?;
    Some(icon.with_glint(runtime.item_glint(stack, item.glint)))
}

/// Borrows the authorities needed to resolve one frame's item visuals.
pub(super) struct ItemResolver<'a> {
    pub runtime: &'a UiRuntime,
    pub presentation: &'a UiPresentationRuntime,
    pub authority: Option<&'a client_world::WorldAuthority>,
}

impl ItemResolver<'_> {
    /// Resolves a stack against the admitted item registry.
    pub fn item(
        &self,
        stack: &protocol::NetworkItemStack,
    ) -> Option<client_world::CanonicalItemStack> {
        self.authority?.canonical_item_stack(stack)
    }

    /// Returns the stack's canonical identifier when the registry knows it.
    pub fn identifier(&self, stack: &protocol::NetworkItemStack) -> Option<Arc<str>> {
        self.item(stack)?.identifier
    }

    /// Resolves stack artwork with its glint and optional local animation frame.
    pub fn icon(
        &self,
        stack: &protocol::NetworkItemStack,
        animation: Option<u32>,
    ) -> Option<IconRef> {
        self.item(stack)
            .and_then(|item| stack_icon(self.runtime, self.presentation, stack, item, animation))
    }

    /// Combines registry durability with the stack's latest display correction.
    pub fn durability(
        &self,
        stack: &protocol::NetworkItemStack,
        correction: Option<i32>,
    ) -> Option<f32> {
        let maximum = self
            .authority
            .and_then(|authority| authority.item_durability(stack.network_id).maximum());
        self.runtime.stack_durability(stack, maximum, correction)
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use {super::*, ui::IconRef};
    /// Resolves a stack icon for app item-use integration tests.
    pub fn stack_icon(
        runtime: &UiRuntime,
        presentation: &UiPresentationRuntime,
        stack: &protocol::NetworkItemStack,
        item: client_world::CanonicalItemStack,
        animation_frame: Option<u32>,
    ) -> Option<IconRef> {
        super::stack_icon(runtime, presentation, stack, item, animation_frame)
    }
}
