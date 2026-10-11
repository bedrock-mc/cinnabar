//! Item-use requirements and selected-stack classification.
use chunk_pipeline::WorldStream;
use client_ui::ui_runtime::UiRuntime;
use gameplay::item_use::{AirUse, Needs, QUICK_CHARGE_ENCHANTMENT_ID, classify};

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
    projectile_override: Option<Option<&str>>,
) -> Option<AirUse> {
    let stack = player_runtime.selected_stack()?;
    let canonical = stream.authority().canonical_item_stack(stack)?;
    let identifier = canonical.identifier.as_deref()?;
    let quick_charge =
        protocol::item_enchantment_level(&stack.extra_data, QUICK_CHARGE_ENCHANTMENT_ID)
            .unwrap_or(0);
    let pack_ticks = stream
        .authority()
        .item_max_use_ticks(identifier)
        .or_else(|| {
            gameplay::item_use::classify::pack_identifier(identifier)
                .and_then(|pack| stream.authority().item_max_use_ticks(pack))
        });
    classify(
        identifier,
        projectile_override.map_or(canonical.charged_projectile.is_some(), |projectile| {
            projectile.is_some()
        }),
        quick_charge,
        pack_ticks,
    )
}

/// The use duration of the selected stack when vanilla animates its use as eating or drinking.
pub fn consume_ticks(
    player_runtime: &client_state::PlayerRuntime,
    stream: &WorldStream,
) -> Option<u32> {
    let canonical = stream
        .authority()
        .canonical_item_stack(player_runtime.selected_stack()?)?;
    match selected_air_use(player_runtime, stream)? {
        AirUse::Hold { max_ticks, .. }
            if gameplay::item_use::classify::is_consumed(canonical.identifier.as_deref()?) =>
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
    let is = |stack: &protocol::NetworkItemStack, identifier: &str| {
        !stack.is_empty()
            && stream
                .authority()
                .item_identifier(stack.network_id)
                .is_some_and(|name| &*name == identifier)
    };
    let arrow_in_inventory = || {
        let ledger = ui.inventory_ledger(player_runtime);
        (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter_map(|slot| ledger.displayed_stack(slot))
            .chain(ui.gameplay_hud().offhand_stack())
            .any(|stack| is(stack, "minecraft:arrow"))
    };
    match needs {
        Needs::Nothing => true,
        Needs::Arrow => arrow_in_inventory(),
        Needs::ArrowOrOffhandRocket => {
            arrow_in_inventory()
                || ui
                    .gameplay_hud()
                    .offhand_stack()
                    .is_some_and(|stack| is(stack, "minecraft:firework_rocket"))
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
) -> Option<&'static str> {
    let name = |stack: &protocol::NetworkItemStack| {
        (!stack.is_empty())
            .then(|| stream.authority().item_identifier(stack.network_id))
            .flatten()
    };
    if let Some(offhand) = ui.gameplay_hud().offhand_stack().and_then(name) {
        match &*offhand {
            "minecraft:firework_rocket" => return Some("minecraft:firework_rocket"),
            "minecraft:arrow" => return Some("minecraft:arrow"),
            _ => {}
        }
    }
    (creative
        || (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter_map(|slot| ui.inventory_ledger(player_runtime).displayed_stack(slot))
            .filter_map(name)
            .any(|identifier| &*identifier == "minecraft:arrow"))
    .then_some("minecraft:arrow")
}
