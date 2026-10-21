//! Artwork for the open container and its recipe pages.

use super::item_icons::ItemResolver;
use crate::ui_runtime::presentation::{
    self,
    hud_layout::{WindowIcons, WindowText},
    inventory_pointer::InventoryScreen,
};

/// Captures the engine recipe list with its existing cache generation.
pub(super) fn recipe_entries(
    player_runtime: &player_state::PlayerState,
    items: &ItemResolver<'_>,
    inventory_screen: InventoryScreen,
    window_icons: &mut WindowIcons,
) {
    let runtime = items.runtime;
    let presentation = items.presentation;
    if matches!(
        inventory_screen,
        InventoryScreen::Personal
            | InventoryScreen::Workbench
            | InventoryScreen::Creative
            | InventoryScreen::Window(
                protocol::WindowKind::Furnace
                    | protocol::WindowKind::BlastFurnace
                    | protocol::WindowKind::Smoker,
                _
            )
    ) && presentation::forms::recipe_book_shown(player_runtime, runtime)
    {
        // Entries resolve icons like slots do, so enchanted and foil items glint here too.
        let icon = |stack: &protocol::NetworkItemStack| items.icon(stack, None);
        if ui_screens::furnace_book::active(player_runtime) {
            window_icons.furnace_entries = presentation::forms::furnace_book::icons(
                player_runtime,
                runtime,
                presentation.session_icon_generation(),
                icon,
            );
        } else {
            window_icons.book_entries =
                presentation::forms::recipe_book_icons(player_runtime, runtime, icon);
        }
    }
}

/// Captures the visible creative cells and tab artwork.
pub(super) fn creative_page(
    player_runtime: &player_state::PlayerState,
    items: &ItemResolver<'_>,
    inventory_screen: InventoryScreen,
    window_icons: &mut WindowIcons,
) {
    let runtime = items.runtime;
    let presentation = items.presentation;
    if inventory_screen == InventoryScreen::Creative {
        let entries =
            crate::ui_runtime::inventory_actions::visible_creative_entries(player_runtime, runtime);
        let first = runtime.screen_state().creative_row * presentation::screens::GRID_COLUMNS;
        for (cell, item) in entries
            .iter()
            .skip(first)
            .take(presentation::screens::GRID_CELLS)
            .enumerate()
        {
            window_icons.creative[cell] = items.icon(&item.stack, None);
        }
        for (tab, id) in [
            assets::facts::UiItemIcon::Construction.identifier(),
            assets::facts::UiItemIcon::Nature.identifier(),
            assets::facts::UiItemIcon::Equipment.identifier(),
            assets::facts::UiItemIcon::Items.identifier(),
            assets::facts::UiItemIcon::Search.identifier(),
        ]
        .iter()
        .enumerate()
        {
            window_icons.creative_tabs[tab] = presentation.item_icon(id, 0);
        }
    }
}

/// Captures stonecutter choices and the predicted container output.
pub(super) fn screen_outputs(
    player_runtime: &player_state::PlayerState,
    items: &ItemResolver<'_>,
    window_icons: &mut WindowIcons,
) {
    let runtime = items.runtime;
    if let Some(kind) = runtime.inventory_ledger(player_runtime).window_kind() {
        if let Some(trade) = runtime.inventory_ledger(player_runtime).trading() {
            window_icons.recipe = trade
                .offers
                .iter()
                .flat_map(|offer| {
                    offer
                        .inputs
                        .iter()
                        .chain(std::iter::once(&offer.output))
                        .map(|stack| {
                            stack.as_ref().and_then(|stack| {
                                if stack.metadata == u32::from(protocol::RECIPE_ANY_AUX) {
                                    let mut preview = stack.clone();
                                    preview.metadata = 0;
                                    items.icon(&preview, None)
                                } else {
                                    items.icon(stack, None)
                                }
                            })
                        })
                })
                .collect();
            if let Some(stack) = runtime.inventory_ledger(player_runtime).trade_output() {
                window_icons.recipe_output = Some(ui_screens::container_source::StationOutput {
                    stack: stack.clone(),
                    icon: items.icon(stack, None),
                    durability: items.durability(stack, None),
                });
            }
            return;
        }
        let output_stack = |output: protocol::RecipeOutput| protocol::NetworkItemStack {
            network_id: output.network_id,
            metadata: u32::from(output.aux),
            count: u16::from(output.count),
            block_runtime_id: i32::from_ne_bytes(output.block_runtime_id.to_ne_bytes()),
            ..protocol::NetworkItemStack::empty()
        };
        if kind == protocol::WindowKind::Stonecutter {
            let outputs: Vec<_> = runtime
                .stonecutter_options(player_runtime)
                .iter()
                .take(ui_screens::container_actions::STONECUTTER_CELLS)
                .map(|recipe| recipe.output)
                .collect();
            for (cell, output) in outputs.into_iter().enumerate() {
                if let Some(output) = output {
                    let stack = output_stack(output);
                    window_icons.recipe[cell] = items.icon(&stack, None);
                }
            }
        }
        let no_server_output = runtime
            .inventory_ledger(player_runtime)
            .created_output_stack()
            .is_none();
        if kind == protocol::WindowKind::Anvil
            && no_server_output
            && let Some(stack) = runtime.anvil_output(player_runtime)
        {
            let icon = items.icon(&stack, None);
            let durability = items.durability(&stack, None);
            window_icons.recipe_output = Some(ui_screens::container_source::StationOutput {
                stack,
                icon,
                durability,
            });
        } else if no_server_output
            && let Some(output) = runtime.predicted_screen_output(player_runtime)
        {
            let stack = output_stack(output);
            let icon = items.icon(&stack, None);
            let durability = items.durability(&stack, None);
            window_icons.recipe_output = Some(ui_screens::container_source::StationOutput {
                stack,
                icon,
                durability,
            });
        }
    }
}

/// Captures the personal recipe page and its next-page indicator.
pub(super) fn book_page(
    player_runtime: &player_state::PlayerState,
    items: &ItemResolver<'_>,
    inventory_screen: InventoryScreen,
    window_text: &mut WindowText,
    window_icons: &mut WindowIcons,
) {
    let runtime = items.runtime;
    let presentation = items.presentation;
    if matches!(
        inventory_screen,
        InventoryScreen::Personal | InventoryScreen::Workbench
    ) {
        window_icons.book_button =
            presentation.item_icon(assets::facts::UiItemIcon::Book.identifier(), 0);
        window_text.book_title = runtime
            .translation("recipe.book.title")
            .map(|text| text.to_string());
        if crate::ui_runtime::presentation::forms::recipe_book_shown(player_runtime, runtime) {
            let first = runtime.screen_state().book_page * presentation::screens::BOOK_CELLS;
            let page =
                runtime.book_recipes(player_runtime, first, presentation::screens::BOOK_CELLS + 1);
            window_icons.book_more = page.len() > presentation::screens::BOOK_CELLS;
            for (cell, recipe) in page
                .iter()
                .take(presentation::screens::BOOK_CELLS)
                .enumerate()
            {
                let output = recipe.recipe.output();
                let stack = protocol::NetworkItemStack {
                    network_id: output.network_id,
                    metadata: u32::from(output.aux),
                    count: u16::from(output.count),
                    block_runtime_id: i32::from_ne_bytes(output.block_runtime_id.to_ne_bytes()),
                    ..protocol::NetworkItemStack::empty()
                };
                window_icons.book[cell] = items.icon(&stack, None);
            }
        }
    }
}
