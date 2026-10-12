//! Item-use requirements and selected-stack classification.
use assets::item_use::ItemUseRequirement as Needs;
use chunk_pipeline::WorldStream;
use client_ui::ui_runtime::UiRuntime;
use gameplay::item_use::{AirUse, QUICK_CHARGE_ENCHANTMENT_ID, classify};

/// The selected stack's authoritative air use, if supported.
pub fn selected_air_use(
    player_runtime: &client_state::PlayerRuntime,
    stream: &WorldStream,
) -> Option<AirUse> {
    selected_air_use_with_projectile(player_runtime, stream, None)
}

/// Resolves the selected item with an optional predicted projectile override.
pub fn selected_air_use_with_projectile(
    player_runtime: &client_state::PlayerRuntime,
    stream: &WorldStream,
    projectile_override: Option<Option<assets::item_use::ItemProjectileRole>>,
) -> Option<AirUse> {
    let stack = player_runtime.selected_stack()?;
    let canonical = stream.authority().canonical_item_stack(stack)?;
    let quick_charge =
        protocol::item_enchantment_level(&stack.extra_data, QUICK_CHARGE_ENCHANTMENT_ID)
            .unwrap_or(0);
    let profile = stream.authority().item_use_profile(stack.network_id);
    classify(
        profile,
        projectile_override.map_or(canonical.charged_projectile.is_some(), |projectile| {
            projectile.is_some()
        }),
        quick_charge,
    )
}

/// The use duration of the selected stack when vanilla animates its use as eating or drinking.
pub fn consume_ticks(
    player_runtime: &client_state::PlayerRuntime,
    stream: &WorldStream,
) -> Option<u32> {
    let stack = player_runtime.selected_stack()?;
    let profile = stream.authority().item_use_profile(stack.network_id);
    match selected_air_use(player_runtime, stream)? {
        AirUse::Hold { max_ticks, .. }
            if matches!(
                profile.consumption,
                assets::item_use::ItemConsumption::Eat
                    | assets::item_use::ItemConsumption::Drink
                    | assets::item_use::ItemConsumption::Honey
            ) =>
        {
            Some(max_ticks)
        }
        _ => None,
    }
}

/// Whether the known state meets `needs`.
pub fn needs_met(
    player_runtime: &client_state::PlayerRuntime,
    stream: &WorldStream,
    ui: &UiRuntime,
    needs: Needs,
) -> bool {
    let is = |stack: &protocol::NetworkItemStack, role: assets::item_use::ItemProjectileRole| {
        !stack.is_empty()
            && stream
                .authority()
                .item_use_profile(stack.network_id)
                .projectile
                == role
    };
    let arrow_in_inventory = || {
        let ledger = ui.inventory_ledger(player_runtime);
        (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter_map(|slot| ledger.displayed_stack(slot))
            .chain(ui.gameplay_hud().offhand_stack())
            .any(|stack| is(stack, assets::item_use::ItemProjectileRole::Arrow))
    };
    match needs {
        Needs::Nothing => true,
        Needs::Arrow => arrow_in_inventory(),
        Needs::ArrowOrOffhandRocket => {
            arrow_in_inventory()
                || ui
                    .gameplay_hud()
                    .offhand_stack()
                    .is_some_and(|stack| is(stack, assets::item_use::ItemProjectileRole::Firework))
        }
        // Peaceful difficulty's always-edible rule is not modeled.
        Needs::Appetite => ui
            .hud()
            .hunger()
            .is_none_or(|hunger| hunger.current() < hunger.maximum()),
    }
}

/// Release checks the offhand for either projectile first, then inventory
/// arrows, and synthesizes an arrow only in creative.
pub fn loading_projectile(
    player_runtime: &client_state::PlayerRuntime,
    stream: &chunk_pipeline::WorldStream,
    ui: &UiRuntime,
    creative: bool,
) -> Option<assets::item_use::ItemProjectileRole> {
    let role = |stack: &protocol::NetworkItemStack| {
        if stack.is_empty() {
            assets::item_use::ItemProjectileRole::None
        } else {
            stream
                .authority()
                .item_use_profile(stack.network_id)
                .projectile
        }
    };
    if let Some(offhand) = ui.gameplay_hud().offhand_stack() {
        match role(offhand) {
            assets::item_use::ItemProjectileRole::Firework => {
                return Some(assets::item_use::ItemProjectileRole::Firework);
            }
            assets::item_use::ItemProjectileRole::Arrow => {
                return Some(assets::item_use::ItemProjectileRole::Arrow);
            }
            _ => {}
        }
    }
    (creative
        || (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter_map(|slot| ui.inventory_ledger(player_runtime).displayed_stack(slot))
            .any(|stack| role(stack) == assets::item_use::ItemProjectileRole::Arrow))
    .then_some(assets::item_use::ItemProjectileRole::Arrow)
}
