//! One retained anvil prediction shared by cost, icon, hover and take consumers.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use inventory::inventory_ledger::{
    ANVIL_MATERIAL_SLOT, ANVIL_TARGET_SLOT, InventoryTarget, StackResponseOverlay,
};
use protocol::{ItemRegistryEntry, NetworkItemStack, PlayerGameMode, WindowKind};

use super::{AnvilPreview, UiRuntime};

/// The occupied cell's canonical stack and authoritative presentation correction.
#[derive(Debug, PartialEq, Eq)]
struct Input {
    stack: NetworkItemStack,
    overlay: Option<StackResponseOverlay>,
}

/// One prediction retained with every input that can change its calculation.
#[derive(Debug)]
struct Cached {
    inputs: [Option<Input>; 2],
    registry: Option<Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    name: String,
    names_generation: u64,
    creative: bool,
    preview: Option<Arc<AnvilPreview>>,
}

/// Keeps the last prediction, including a result that cannot be calculated.
#[derive(Debug, Default)]
pub(crate) struct PreviewCache(Mutex<Option<Cached>>);

impl Clone for PreviewCache {
    /// Cloned runtimes own independent caches because their language and input state can diverge.
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl UiRuntime {
    /// Whether the player can pay for `preview` now: it is not too expensive, and the player
    /// is in creative or has the levels.
    #[must_use]
    pub fn anvil_affordable(
        &self,
        player_runtime: &player_state::PlayerState,
        preview: &AnvilPreview,
    ) -> bool {
        if preview.outcome.too_expensive {
            return false;
        }
        player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative)
            || self
                .hud
                .experience()
                .is_some_and(|xp| i64::from(xp.level) >= i64::from(preview.outcome.cost))
    }

    /// The open anvil's shared result, recalculated only when its inputs or name change.
    #[must_use]
    pub fn anvil_preview(&self, player: &player_state::PlayerState) -> Option<Arc<AnvilPreview>> {
        let ledger = self.inventory_ledger(player);
        let mut cache = self
            .anvil_cache
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ledger.window_kind() != Some(WindowKind::Anvil) {
            *cache = None;
            return None;
        }
        let inputs = [ANVIL_TARGET_SLOT, ANVIL_MATERIAL_SLOT].map(|slot| {
            let target = InventoryTarget::Craft(slot);
            ledger
                .target_stack(target)
                .filter(|stack| stack.count > 0)
                .map(|stack| Input {
                    stack: stack.clone(),
                    overlay: ledger.presented_target_overlay(target).cloned(),
                })
        });
        let registry = ledger.item_registry_snapshot();
        let name = self.screen_state().anvil_name.as_str();
        let creative = player.facts.player_game_mode() == Some(PlayerGameMode::Creative);
        let names_generation = self.item_name_generation();
        if let Some(cached) = cache.as_ref()
            && cached.inputs == inputs
            && same_registry(cached.registry.as_ref(), registry)
            && cached.name == name
            && cached.names_generation == names_generation
            && cached.creative == creative
        {
            return cached.preview.clone();
        }
        let preview = self.compute_anvil_preview(player).map(Arc::new);
        *cache = Some(Cached {
            inputs,
            registry: registry.cloned(),
            name: name.to_owned(),
            names_generation,
            creative,
            preview: preview.clone(),
        });
        preview
    }
}

/// Registry resends retain their allocation until admitted registry facts change.
fn same_registry(
    left: Option<&Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    right: Option<&Arc<BTreeMap<i32, ItemRegistryEntry>>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}
