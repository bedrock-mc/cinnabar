use {super::*, ui::IconRef};

pub mod commit;
pub mod item_icons;
use item_icons::{ItemIconFrames, ItemResolver};

mod container;
pub(super) mod inventory_cache;
mod observations;
mod player_items;
mod recipes;

/// Captures HUD display values and applies its required local authority observations.
pub fn capture_hud_frame(
    player_runtime: &player_state::PlayerState,
    runtime: &mut UiRuntime,
    presentation: &mut UiPresentationRuntime,
    stream: Option<&chunk_pipeline::WorldStream>,
    perspective: semantic_input::PerspectiveMode,
    now_millis: u64,
    icon_frames: ItemIconFrames,
) -> (Option<IconRef>, Option<IconRef>) {
    presentation.capture_local_player_identity(stream, runtime.session_id());
    runtime.sync_anvil_name(player_runtime);
    // Inventory capture precedes lectern opening and screen selection. Selected-item
    // and sleep observations run after item capture, immediately before publication.
    let armor = observations::armor(
        player_runtime,
        runtime,
        stream.map(|stream| stream.authority()),
    );
    runtime.set_derived_armor(armor);
    let mount_health = runtime.gameplay_hud().mount_unique_id().and_then(|unique| {
        stream.and_then(|stream| stream.authority().actor_health_by_unique(unique))
    });
    let mut inventory_cache = presentation
        .inventory_cache
        .take()
        .expect("inventory cache retained");
    let items = ItemResolver {
        runtime,
        presentation,
        authority: stream.map(|stream| stream.authority()),
    };
    let projections = inventory_cache.capture(player_runtime, &items, &icon_frames);
    presentation.inventory_cache = Some(inventory_cache);
    let mut window_icons = container::ghost_icons(presentation, runtime.inventory_open());
    window_icons.ui = projections
        .decorations
        .as_ref()
        .map_or(presentation.hud_frame.window_icons.ui, |(_, icons)| *icons);
    observations::open_lectern(player_runtime, runtime, stream);
    let items = ItemResolver {
        runtime,
        presentation,
        authority: stream.map(|stream| stream.authority()),
    };
    let window_text = container::capture(player_runtime, &items, stream, &mut window_icons);
    let cursor_icon = runtime
        .inventory_ledger(player_runtime)
        .cursor_stack()
        .and_then(|stack| items.icon(stack, None));
    let ledger = runtime.inventory_ledger(player_runtime);
    let cursor_durability = ledger.cursor_stack().and_then(|stack| {
        items.durability(
            stack,
            ledger
                .presented_cursor_overlay()
                .and_then(|overlay| overlay.durability_correction),
        )
    });
    let selected = player_items::SelectedItem::capture(player_runtime);
    let hotbar = player_items::hotbar(
        runtime.inventory_ledger(player_runtime),
        &selected,
        &items,
        &icon_frames,
    );
    presentation.note_hotbar(hotbar.logged);
    let equipment = player_items::equipment(
        player_runtime,
        &selected,
        &ItemResolver {
            runtime,
            presentation,
            authority: stream.map(|stream| stream.authority()),
        },
        &icon_frames,
    );
    let mount_jump = runtime.gameplay_hud().mount_unique_id().and_then(|unique| {
        stream
            .filter(|stream| {
                stream
                    .authority()
                    .actor_has_attribute_by_unique(unique, "minecraft:horse.jump_strength")
            })
            .map(|_| runtime.mount_jump_charge(now_millis))
    });
    let first_person = perspective == semantic_input::PerspectiveMode::FirstPerson;
    let player_preview_icon = presentation.player_preview_icon();
    let (left_hand_icon, right_hand_icon) = presentation.player_hand_icons();
    runtime.observe_selected_item_identity_value(selected.identity(), now_millis);
    let sleeping = stream
        .and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()))
        .is_some_and(|actor| actor.is_sleeping());
    runtime.set_local_sleeping(sleeping);
    let frame = presentation.hud_frame_mut();
    frame.sleep.observe(sleeping, now_millis);
    frame.first_person = first_person;
    frame.mount_health = mount_health;
    frame.hotbar_durability = hotbar.durability;
    frame.hotbar_stacks = hotbar.stacks;
    frame.offhand_durability = equipment.offhand_durability;
    frame.hotbar_icons = hotbar.icons;
    if let Some(contents) = projections.contents {
        frame.inventory_icons = contents.inventory_icons;
        frame.storage_icons = contents.storage_icons;
        frame.crafting = contents.crafting;
    }
    if let Some(names) = projections.item_names {
        frame.item_names = names;
    }
    frame.window_icons = window_icons;
    if let Some((durability, _)) = projections.decorations {
        frame.durability = durability;
    }
    frame.window_text = window_text;
    frame.cursor_icon = cursor_icon;
    frame.cursor_durability = cursor_durability;
    frame.armor_icons = equipment.armor_icons;
    frame.armor_durability = equipment.armor_durability;
    frame.offhand_icon = equipment.offhand_icon;
    frame.offhand_viewmodel_icon = None;
    frame.held_item_icon = None;
    frame.player_preview = player_preview_icon;
    frame.left_hand = left_hand_icon;
    frame.right_hand = right_hand_icon;
    frame.viewmodel_pitch_degrees = stream
        .and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()))
        .map_or(0.0, |actor| actor.pitch);
    frame.selected_food = selected
        .stack
        .and_then(|stack| stream?.authority().canonical_item_stack(stack)?.identifier)
        .and_then(|identifier| runtime.food_properties(&identifier));
    frame.selected_item_name = equipment.selected_item_name;
    frame.holding_filled_map = equipment.holding_filled_map;
    frame.mount_jump = mount_jump;
    frame.attack_indicator_charge = Some(1.0);
    observations::note_diagnostics(runtime, presentation);
    (equipment.held_item_icon, equipment.offhand_icon)
}

/// The hovered container cell's stack and its custom name: what the tooltip shows, and what a
/// player mod's key reports as the slot under the pointer (JEI's `getSlotUnderMouse`).
pub fn hovered_stack(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> Option<(protocol::NetworkItemStack, Option<std::sync::Arc<str>>)> {
    runtime.screen_state().hover.and_then(|hit| {
        use ui_screens::container_actions::InventoryCellHit as Hit;
        let ledger = runtime.inventory_ledger(player_runtime);
        let (stack, name) = match hit {
            Hit::Player(slot) => (
                ledger.displayed_stack(slot),
                ledger
                    .presented_slot_overlay(slot)
                    .and_then(|overlay| overlay.custom_name.clone()),
            ),
            Hit::Storage(slot) => (ledger.storage_stack(slot), None),
            Hit::Craft(slot) => (
                ledger.target_stack(inventory::inventory_ledger::InventoryTarget::Craft(slot)),
                None,
            ),
            Hit::Armor(row) => (
                ledger.target_stack(inventory::inventory_ledger::InventoryTarget::Armor(row)),
                None,
            ),
            Hit::Offhand => (
                ledger.target_stack(inventory::inventory_ledger::InventoryTarget::Offhand),
                None,
            ),
            Hit::CraftOutput if ledger.window_kind() == Some(protocol::WindowKind::Anvil) => {
                return runtime
                    .anvil_output(player_runtime)
                    .map(|stack| (stack, None));
            }
            Hit::CraftOutput => (ledger.created_output_stack(), None),
            Hit::CreativeGrid(index) => {
                let position = runtime.screen_state().creative_row * super::screens::GRID_COLUMNS
                    + usize::from(index);
                let entries = crate::ui_runtime::inventory_actions::visible_creative_entries(
                    player_runtime,
                    runtime,
                );
                (entries.get(position).map(|item| &item.stack), None)
            }
            Hit::Widget(ui_screens::container_actions::Widget::BookRecipe(index)) => {
                let skip = runtime.screen_state().book_page * super::screens::BOOK_CELLS
                    + usize::from(index);
                let output = runtime
                    .book_recipes(player_runtime, skip, 1)
                    .first()
                    .map(|entry| entry.recipe.output());
                return output.map(|output| {
                    let stack = protocol::NetworkItemStack {
                        network_id: output.network_id,
                        metadata: u32::from(output.aux),
                        count: u16::from(output.count),
                        block_runtime_id: i32::try_from(output.block_runtime_id).unwrap_or(0),
                        ..protocol::NetworkItemStack::empty()
                    };
                    (stack, None)
                });
            }
            Hit::RecipeBook(index) => {
                return super::forms::recipe_book_hover(player_runtime, runtime, index);
            }
            Hit::Widget(_) | Hit::CreativeTab(_) | Hit::CreativeSearch => (None, None),
        };
        stack.map(|stack| (stack.clone(), name))
    })
}
