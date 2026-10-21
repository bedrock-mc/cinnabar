//! Text and artwork projected for the active container screen.

use super::item_icons::ItemResolver;
use crate::ui_runtime::{
    UiRuntime,
    presentation::{self, UiPresentationRuntime},
};
use presentation::{
    hud_layout::{WindowIcons, WindowText},
    inventory_pointer::InventoryScreen,
};

/// Resolves the active screen after any lectern observation, then captures its text and artwork.
pub(super) fn capture(
    player_runtime: &player_state::PlayerState,
    items: &ItemResolver<'_>,
    stream: Option<&chunk_pipeline::WorldStream>,
    window_icons: &mut WindowIcons,
) -> WindowText {
    let runtime = items.runtime;
    let inventory_screen = InventoryScreen::of_runtime(player_runtime, runtime);
    let mut window_text = WindowText::default();
    if runtime.inventory_open() {
        capture_titles(
            player_runtime,
            runtime,
            stream,
            inventory_screen,
            &mut window_text,
        );
        super::recipes::recipe_entries(player_runtime, items, inventory_screen, window_icons);
        super::recipes::creative_page(player_runtime, items, inventory_screen, window_icons);
        super::recipes::screen_outputs(player_runtime, items, window_icons);
        super::recipes::book_page(
            player_runtime,
            items,
            inventory_screen,
            &mut window_text,
            window_icons,
        );
        capture_tooltip(player_runtime, items, &mut window_text);
    }
    window_text
}

/// Captures untranslated custom titles and the screen's localized labels.
fn capture_titles(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    stream: Option<&chunk_pipeline::WorldStream>,
    inventory_screen: InventoryScreen,
    window_text: &mut WindowText,
) {
    let ledger = runtime.inventory_ledger(player_runtime);
    // Actor-backed server menus carry their raw title on the owning actor.
    // Keep formatting markers intact: resource-pack layouts use them.
    let stated_title = ledger
        .trading()
        .map(|trade| {
            runtime
                .translation(&trade.title)
                .map_or_else(|| trade.title.to_string(), |title| title.to_string())
        })
        .or_else(|| {
            ledger
                .window_actor()
                .and_then(|actor| stream?.authority().actor_name_tag(actor))
                .filter(|name| !name.is_empty())
                .map(|name| name.to_string())
                .or_else(|| {
                    ledger
                        .window_position()
                        .and_then(|position| stream?.block_entity_custom_name(position))
                })
        });
    window_text.custom_title.clone_from(&stated_title);
    window_text.block_entity = ledger
        .window_position()
        .and_then(|position| stream?.block_entity_compound(position))
        .and_then(|nbt| nbt.string("id").map(str::to_owned));
    window_text.inventory_label = runtime
        .translation("container.inventory")
        .map(|text| text.to_string());
    window_text.title = match inventory_screen {
        InventoryScreen::Window(kind, _) => stated_title.or_else(|| {
            runtime
                .translation(presentation::hud_layout::title_key(kind))
                .map(|text| text.to_string())
        }),
        InventoryScreen::Storage(count) => stated_title.or_else(|| {
            runtime
                .translation(if count == 54 {
                    "container.chestDouble"
                } else {
                    "container.chest"
                })
                .map(|text| text.to_string())
        }),
        InventoryScreen::Workbench => stated_title.or_else(|| {
            runtime
                .translation("container.crafting")
                .map(|text| text.to_string())
        }),
        InventoryScreen::Creative => {
            let key = match runtime.creative_tab(player_runtime) {
                0 => "itemGroup.name.construction",
                1 => "itemGroup.name.nature",
                2 => "itemGroup.name.equipment",
                3 => "itemGroup.name.items",
                _ => "itemGroup.name.search",
            };
            runtime.translation(key).map(|text| text.to_string())
        }
        InventoryScreen::Personal | InventoryScreen::Book => None,
    };
    if matches!(
        inventory_screen,
        InventoryScreen::Window(protocol::WindowKind::Beacon, _)
    ) {
        window_text.effect_names = [
            (1, "effect.moveSpeed"),
            (3, "effect.digSpeed"),
            (11, "effect.resistance"),
            (8, "effect.jump"),
            (5, "effect.damageBoost"),
            (10, "effect.regeneration"),
        ]
        .iter()
        .map(|(id, key)| {
            let name = runtime
                .translation(key)
                .map_or_else(|| (*key).to_owned(), |text| text.to_string());
            (*id, name)
        })
        .collect();
    }
}

/// Adds the hovered item's tooltip, optional nutrition, and bundle contents in display order.
fn capture_tooltip(
    player_runtime: &player_state::PlayerState,
    items: &ItemResolver<'_>,
    window_text: &mut WindowText,
) {
    let runtime = items.runtime;
    let presentation = items.presentation;
    // The tooltip follows the hovered cell's stack.
    let hovered = super::hovered_stack(player_runtime, runtime);
    if let Some((stack, name)) = hovered {
        let identifier = items.identifier(&stack);
        window_text.tooltip = presentation::inventory_tooltip::tooltip_lines(
            runtime,
            &stack,
            identifier.as_deref(),
            name.as_deref(),
        );
        if presentation
            .mod_ui()
            .mod_hunger_overlay()
            .is_some_and(|style| style.tooltips)
            && let Some(food) = identifier
                .as_deref()
                .and_then(|identifier| runtime.food_properties(identifier))
        {
            window_text
                .tooltip
                .push(presentation::hud_layout::TooltipLine {
                    text: format!(
                        "Nutrition: {}  Saturation: {:.1}",
                        food.nutrition(),
                        food.nutrition() as f32 * food.saturation_modifier() * 2.
                    ),
                    color: [255, 170, 0, 255],
                });
        }
        if let Some(contents) = protocol::item_bundle_id(&stack.extra_data)
            .and_then(|id| runtime.inventory_ledger(player_runtime).bundle_contents(id))
        {
            for held in contents.iter().filter(|held| !held.is_empty()).take(8) {
                let item_name = items
                    .identifier(held)
                    .map_or_else(|| "?".to_owned(), |id| runtime.localized_item_name(&id));
                window_text
                    .tooltip
                    .push(presentation::hud_layout::TooltipLine {
                        text: format!("{}x {item_name}", held.count),
                        color: [200, 200, 200, 255],
                    });
            }
        }
    }
}

/// Resolves empty-slot artwork only while an inventory screen is open.
pub(super) fn ghost_icons(
    presentation: &UiPresentationRuntime,
    inventory_open: bool,
) -> WindowIcons {
    let mut window_icons = WindowIcons::default();
    if inventory_open {
        for (row, name) in ["helmet", "chestplate", "leggings", "boots"]
            .iter()
            .enumerate()
        {
            window_icons.ghost_armor[row] =
                presentation.item_icon(&format!("minecraft:empty_armor_slot_{name}"), 0);
        }
        window_icons.ghost_shield = presentation.item_icon("minecraft:empty_armor_slot_shield", 0);
        window_icons.ghost_template =
            presentation.item_icon("minecraft:empty_slot_smithing_template", 0);
    }
    window_icons
}
