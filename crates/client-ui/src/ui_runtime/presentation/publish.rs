use super::*;

pub mod commit;
pub mod item_icons;
use item_icons::{ItemIconFrames, stack_icon};

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
    let resolve_identifier = |stack: &protocol::NetworkItemStack| {
        stream.and_then(|stream| stream.authority().canonical_item_stack(stack)?.identifier)
    };
    let worn = runtime.local_armor(player_runtime);
    let worn = [&worn.helmet, &worn.chestplate, &worn.leggings, &worn.boots];
    let identifiers = worn.map(|stack| {
        (!stack.is_empty())
            .then(|| resolve_identifier(stack))
            .flatten()
    });
    runtime.set_derived_armor(Some(item_facts::total_armor_points(
        identifiers.iter().map(|id| id.as_deref()),
    )));
    let mount_health = runtime.gameplay_hud().mount_unique_id().and_then(|unique| {
        stream.and_then(|stream| stream.authority().actor_health_by_unique(unique))
    });
    let mut hotbar_durability = [None; 9];
    let mut hotbar_icons = [None; 9];
    let mut hotbar_stacks: [Option<protocol::NetworkItemStack>; 9] = Default::default();
    let mut logged_hotbar: [Option<(Arc<str>, bool)>; 9] = Default::default();
    let mut inventory_icons = super::hud_layout::InventoryIcons::default();
    for (slot, icon) in inventory_icons.0.iter_mut().enumerate() {
        if let Some(stack) = runtime
            .inventory_ledger(player_runtime)
            .displayed_stack(slot as u8)
        {
            *icon = resolve_identifier(stack).as_deref().and_then(|id| {
                stack_icon(
                    runtime,
                    presentation,
                    stack,
                    id,
                    icon_frames.slot(slot as u8),
                )
            });
        }
    }
    let mut storage_icons = super::hud_layout::StorageIcons::default();
    for (slot, icon) in storage_icons.0.iter_mut().enumerate() {
        if let Some(stack) = runtime
            .inventory_ledger(player_runtime)
            .furnace_visual_stack(slot as u8)
        {
            *icon = resolve_identifier(stack)
                .as_deref()
                .and_then(|id| stack_icon(runtime, presentation, stack, id, None));
        }
    }
    let mut crafting = super::hud_layout::CraftingFrame::default();
    if runtime.inventory_open() {
        let ledger = runtime.inventory_ledger(player_runtime);
        for (icon, slot) in crafting
            .icons
            .iter_mut()
            .zip(ledger.crafting_grid().slots())
        {
            let target = crate::ui_runtime::inventory_ledger::InventoryTarget::Craft(slot);
            if let Some(stack) = ledger.target_stack(target) {
                *icon = resolve_identifier(stack)
                    .as_deref()
                    .and_then(|id| stack_icon(runtime, presentation, stack, id, None));
            }
        }
        if let inventory::CraftGridMatch::Unique(recipe) = runtime.crafting_match(player_runtime) {
            let output = recipe.output();
            let stack = protocol::NetworkItemStack {
                network_id: output.network_id,
                metadata: u32::from(output.aux),
                count: u16::from(output.count),
                block_runtime_id: output.block_runtime_id as i32,
                ..protocol::NetworkItemStack::empty()
            };
            let icon = resolve_identifier(&stack)
                .as_deref()
                .and_then(|id| stack_icon(runtime, presentation, &stack, id, None));
            crafting.output = Some((icon, stack));
        }
    }
    // Hover names for the open container's cells (JSON-UI tooltips).
    let item_names = if runtime.inventory_open() {
        let ledger = runtime.inventory_ledger(player_runtime);
        (0..36u8)
            .filter_map(|slot| ledger.displayed_stack(slot))
            .chain((0..54u8).filter_map(|slot| ledger.furnace_visual_stack(slot)))
            .filter_map(|stack| {
                let name = runtime.localized_item_name(&resolve_identifier(stack)?);
                Some(((stack.network_id, stack.metadata), Arc::from(name)))
            })
            .collect()
    } else {
        Default::default()
    };
    let mut durability = super::hud_layout::Durability::default();
    let mut window_icons = super::hud_layout::WindowIcons::default();
    if runtime.inventory_open() {
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
    {
        use crate::ui_runtime::inventory_ledger::InventoryTarget;
        let ledger = runtime.inventory_ledger(player_runtime);
        let fraction = |stack: &protocol::NetworkItemStack, correction: Option<i32>| {
            let maximum = runtime.item_max_durability(resolve_identifier(stack).as_deref());
            item_facts::cell_durability_fraction(stack, maximum, correction)
        };
        for (slot, bar) in durability.player.iter_mut().enumerate() {
            if let Some(stack) = ledger.displayed_stack(slot as u8) {
                let correction = ledger
                    .presented_slot_overlay(slot as u8)
                    .and_then(|overlay| overlay.durability_correction);
                *bar = fraction(stack, correction);
            }
        }
        for (slot, bar) in durability.storage.iter_mut().enumerate() {
            if let Some(stack) = ledger.storage_stack(slot as u8) {
                *bar = fraction(stack, None);
            }
        }
        for slot in 0..protocol::UI_SLOT_COUNT as u8 {
            let stack = if slot == protocol::CREATED_OUTPUT_SLOT {
                ledger.created_output_stack()
            } else if protocol::ui_slot_container_name(slot).is_some() {
                ledger.target_stack(InventoryTarget::Craft(slot))
            } else {
                None
            };
            if let Some(stack) = stack {
                durability.ui[usize::from(slot)] = fraction(stack, None);
                window_icons.ui[usize::from(slot)] = resolve_identifier(stack)
                    .as_deref()
                    .and_then(|id| stack_icon(runtime, presentation, stack, id, None));
            }
        }
    }
    if runtime.inventory_open()
        && runtime.inventory_ledger(player_runtime).window_kind()
            == Some(protocol::WindowKind::Lectern)
        && runtime.screen_state().book.is_none()
        && let Some(position) = runtime.inventory_ledger(player_runtime).window_position()
        && let Some(nbt) = stream.and_then(|stream| stream.block_entity_compound(position))
    {
        let pages: Vec<String> = nbt
            .compound("book")
            .and_then(|book| book.compound("tag"))
            .and_then(|tag| tag.list("pages"))
            .map(|pages| {
                pages
                    .iter()
                    .map(|page| match page {
                        world::NbtValue::Compound(page) => {
                            page.string("text").unwrap_or_default().to_owned()
                        }
                        _ => String::new(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut state = crate::ui_runtime::book_screen::BookState::new(
            crate::ui_runtime::book_screen::BookSource::Lectern(position),
            pages,
            false,
            String::new(),
            String::new(),
        );
        state.page = nbt
            .integer("page")
            .and_then(|page| usize::try_from(page).ok())
            .map_or(0, |page| page.min(state.pages.len() - 1));
        runtime.open_book(state);
    }
    let inventory_screen =
        super::inventory_pointer::InventoryScreen::of_runtime(player_runtime, runtime);
    let mut window_text = super::hud_layout::WindowText::default();
    if runtime.inventory_open() {
        let ledger = runtime.inventory_ledger(player_runtime);
        // Actor-backed server menus carry their raw title on the owning actor.
        // Keep formatting markers intact: resource-pack layouts use them.
        let stated_title = ledger
            .window_actor()
            .and_then(|actor| stream?.authority().actor_name_tag(actor))
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string())
            .or_else(|| {
                ledger
                    .window_position()
                    .and_then(|position| stream?.block_entity_custom_name(position))
            });
        window_text.custom_title.clone_from(&stated_title);
        window_text.block_entity = runtime
            .inventory_ledger(player_runtime)
            .window_position()
            .and_then(|position| stream?.block_entity_compound(position))
            .and_then(|nbt| nbt.string("id").map(str::to_owned));
        window_text.inventory_label = runtime
            .translation("container.inventory")
            .map(|text| text.to_string());
        window_text.title = match inventory_screen {
            super::inventory_pointer::InventoryScreen::Window(kind, _) => {
                stated_title.or_else(|| {
                    runtime
                        .translation(super::hud_layout::title_key(kind))
                        .map(|text| text.to_string())
                })
            }
            super::inventory_pointer::InventoryScreen::Storage(count) => {
                stated_title.or_else(|| {
                    runtime
                        .translation(if count == 54 {
                            "container.chestDouble"
                        } else {
                            "container.chest"
                        })
                        .map(|text| text.to_string())
                })
            }
            super::inventory_pointer::InventoryScreen::Workbench => stated_title.or_else(|| {
                runtime
                    .translation("container.crafting")
                    .map(|text| text.to_string())
            }),
            super::inventory_pointer::InventoryScreen::Creative => {
                let key = match runtime.screen_state().creative_tab {
                    0 => "itemGroup.name.construction",
                    1 => "itemGroup.name.nature",
                    2 => "itemGroup.name.equipment",
                    3 => "itemGroup.name.items",
                    _ => "itemGroup.name.search",
                };
                runtime.translation(key).map(|text| text.to_string())
            }
            super::inventory_pointer::InventoryScreen::Personal
            | super::inventory_pointer::InventoryScreen::Book => None,
        };
        if matches!(
            inventory_screen,
            super::inventory_pointer::InventoryScreen::Window(protocol::WindowKind::Beacon, _)
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
        if matches!(
            inventory_screen,
            super::inventory_pointer::InventoryScreen::Personal
                | super::inventory_pointer::InventoryScreen::Workbench
                | super::inventory_pointer::InventoryScreen::Creative
                | super::inventory_pointer::InventoryScreen::Window(
                    protocol::WindowKind::Furnace
                        | protocol::WindowKind::BlastFurnace
                        | protocol::WindowKind::Smoker,
                    _
                )
        ) && super::forms::recipe_book_shown(player_runtime, runtime)
        {
            let icon = |stack: &protocol::NetworkItemStack| {
                resolve_identifier(stack)
                    .as_deref()
                    .and_then(|id| presentation.item_icon(id, stack.metadata))
            };
            if super::forms::furnace_book::active(player_runtime) {
                window_icons.furnace_entries = super::forms::furnace_book::icons(
                    player_runtime,
                    runtime,
                    presentation.session_icon_generation(),
                    icon,
                );
            } else {
                window_icons.book_entries =
                    super::forms::recipe_book_icons(player_runtime, runtime, icon);
            }
        }
        if inventory_screen == super::inventory_pointer::InventoryScreen::Creative {
            let entries = crate::ui_runtime::inventory_actions::visible_creative_entries(
                runtime.inventory_ledger(player_runtime),
                runtime.screen_state(),
            );
            let first = runtime.screen_state().creative_row * super::screens::GRID_COLUMNS;
            for (cell, item) in entries
                .iter()
                .skip(first)
                .take(super::screens::GRID_CELLS)
                .enumerate()
            {
                window_icons.creative[cell] = resolve_identifier(&item.stack)
                    .as_deref()
                    .and_then(|id| stack_icon(runtime, presentation, &item.stack, id, None));
            }
            for (tab, id) in [
                "minecraft:brick",
                "minecraft:oak_sapling",
                "minecraft:iron_sword",
                "minecraft:stick",
                "minecraft:compass",
            ]
            .iter()
            .enumerate()
            {
                window_icons.creative_tabs[tab] = presentation.item_icon(id, 0);
            }
        }
        if let Some(kind) = runtime.inventory_ledger(player_runtime).window_kind() {
            let output_stack = |output: protocol::RecipeOutput| protocol::NetworkItemStack {
                network_id: output.network_id,
                metadata: u32::from(output.aux),
                count: u16::from(output.count),
                block_runtime_id: i32::try_from(output.block_runtime_id).unwrap_or(0),
                ..protocol::NetworkItemStack::empty()
            };
            if kind == protocol::WindowKind::Stonecutter {
                let outputs: Vec<_> = runtime
                    .stonecutter_options(player_runtime)
                    .iter()
                    .take(super::screens::STONECUTTER_CELLS)
                    .map(|recipe| recipe.output)
                    .collect();
                for (cell, output) in outputs.into_iter().enumerate() {
                    if let Some(output) = output {
                        let stack = output_stack(output);
                        window_icons.recipe[cell] = resolve_identifier(&stack)
                            .as_deref()
                            .and_then(|id| stack_icon(runtime, presentation, &stack, id, None));
                    }
                }
            }
            if runtime
                .inventory_ledger(player_runtime)
                .created_output_stack()
                .is_none()
                && let Some(output) = runtime.predicted_screen_output(player_runtime)
            {
                let stack = output_stack(output);
                let icon = resolve_identifier(&stack)
                    .as_deref()
                    .and_then(|id| stack_icon(runtime, presentation, &stack, id, None));
                window_icons.recipe_output = Some((icon, stack));
            }
        }
        if matches!(
            inventory_screen,
            super::inventory_pointer::InventoryScreen::Personal
                | super::inventory_pointer::InventoryScreen::Workbench
        ) {
            window_icons.book_button = presentation.item_icon("minecraft:book", 0);
            window_text.book_title = runtime
                .translation("recipe.book.title")
                .map(|text| text.to_string());
            if runtime.screen_state().book_open {
                let first = runtime.screen_state().book_page * super::screens::BOOK_CELLS;
                let page =
                    runtime.book_recipes(player_runtime, first, super::screens::BOOK_CELLS + 1);
                window_icons.book_more = page.len() > super::screens::BOOK_CELLS;
                for (cell, recipe) in page.iter().take(super::screens::BOOK_CELLS).enumerate() {
                    let output = recipe.output();
                    let stack = protocol::NetworkItemStack {
                        network_id: output.network_id,
                        metadata: u32::from(output.aux),
                        count: u16::from(output.count),
                        block_runtime_id: i32::try_from(output.block_runtime_id).unwrap_or(0),
                        ..protocol::NetworkItemStack::empty()
                    };
                    window_icons.book[cell] = resolve_identifier(&stack)
                        .as_deref()
                        .and_then(|id| stack_icon(runtime, presentation, &stack, id, None));
                }
            }
        }
        // The tooltip follows the hovered cell's stack.
        let hovered = runtime.screen_state().hover.and_then(|hit| {
            use super::inventory_pointer::InventoryCellHit as Hit;
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
                    ledger.target_stack(
                        crate::ui_runtime::inventory_ledger::InventoryTarget::Craft(slot),
                    ),
                    None,
                ),
                Hit::Armor(row) => (
                    ledger.target_stack(
                        crate::ui_runtime::inventory_ledger::InventoryTarget::Armor(row),
                    ),
                    None,
                ),
                Hit::Offhand => (
                    ledger.target_stack(
                        crate::ui_runtime::inventory_ledger::InventoryTarget::Offhand,
                    ),
                    None,
                ),
                Hit::CraftOutput => (ledger.created_output_stack(), None),
                Hit::CreativeGrid(index) => {
                    let position = runtime.screen_state().creative_row
                        * super::screens::GRID_COLUMNS
                        + usize::from(index);
                    let entries = crate::ui_runtime::inventory_actions::visible_creative_entries(
                        ledger,
                        runtime.screen_state(),
                    );
                    (entries.get(position).map(|item| &item.stack), None)
                }
                Hit::Widget(super::screens::Widget::BookRecipe(index)) => {
                    let skip = runtime.screen_state().book_page * super::screens::BOOK_CELLS
                        + usize::from(index);
                    let output = runtime
                        .book_recipes(player_runtime, skip, 1)
                        .first()
                        .map(protocol::RecipeHandle::output);
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
        });
        if let Some((stack, name)) = hovered {
            let identifier = resolve_identifier(&stack);
            window_text.tooltip = super::inventory_tooltip::tooltip_lines(
                runtime,
                &stack,
                identifier.as_deref(),
                name.as_deref(),
            );
            if let Some(contents) = protocol::item_bundle_id(&stack.extra_data)
                .and_then(|id| runtime.inventory_ledger(player_runtime).bundle_contents(id))
            {
                for held in contents.iter().filter(|held| !held.is_empty()).take(8) {
                    let item_name = resolve_identifier(held)
                        .map_or_else(|| "?".to_owned(), |id| runtime.localized_item_name(&id));
                    window_text.tooltip.push(super::hud_layout::TooltipLine {
                        text: format!("{}x {item_name}", held.count),
                        color: [200, 200, 200, 255],
                    });
                }
            }
        }
    }
    let cursor_icon = runtime
        .inventory_ledger(player_runtime)
        .cursor_stack()
        .and_then(|stack| {
            resolve_identifier(stack)
                .as_deref()
                .and_then(|id| stack_icon(runtime, presentation, stack, id, None))
        });
    let selected_snapshot = player_runtime.selected_stack_snapshot();
    let selected_slot = selected_snapshot.map(|snapshot| snapshot.slot);
    let selected_stack = selected_snapshot.and_then(|snapshot| match snapshot.state {
        crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Present(stack) => Some(stack),
        crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Unknown
        | crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Empty => None,
    });
    for (slot, durability) in hotbar_durability.iter_mut().enumerate() {
        let slot = slot as u8;
        let stack = if selected_slot == Some(slot) {
            selected_stack
        } else {
            runtime
                .inventory_ledger(player_runtime)
                .displayed_stack(slot)
        };
        if let Some(stack) = stack {
            let identifier = resolve_identifier(stack);
            // Every hotbar cell — selected or not — derives from one
            // ledger-snapshot authority revision: the travelling predicted
            // overlay beside its predicted stack while a gesture is in
            // flight, otherwise the committed overlay. One accepted sparse
            // response therefore refreshes the whole presented row at once.
            let overlay = runtime
                .inventory_ledger(player_runtime)
                .presented_slot_overlay(slot);
            *durability = item_facts::cell_durability_fraction(
                stack,
                runtime.item_max_durability(identifier.as_deref()),
                overlay.and_then(|overlay| overlay.durability_correction),
            );
            hotbar_icons[usize::from(slot)] = identifier.as_deref().and_then(|id| {
                stack_icon(runtime, presentation, stack, id, icon_frames.slot(slot))
            });
            hotbar_stacks[usize::from(slot)] = Some(stack.clone());
            logged_hotbar[usize::from(slot)] = Some((
                identifier
                    .unwrap_or_else(|| Arc::from(format!("<network id {}>", stack.network_id))),
                hotbar_icons[usize::from(slot)].is_some(),
            ));
        }
    }
    presentation.note_hotbar(logged_hotbar);
    let offhand_durability = runtime.gameplay_hud().offhand_stack().and_then(|stack| {
        let maximum = runtime.item_max_durability(resolve_identifier(stack).as_deref());
        item_facts::durability_fraction(stack, maximum)
    });
    let offhand_icon = runtime.gameplay_hud().offhand_stack().and_then(|stack| {
        let identifier = resolve_identifier(stack);
        identifier
            .as_deref()
            .and_then(|id| stack_icon(runtime, presentation, stack, id, None))
    });
    let armor_icons = {
        let armor = runtime.local_armor(player_runtime);
        [
            &armor.helmet,
            &armor.chestplate,
            &armor.leggings,
            &armor.boots,
        ]
        .map(|stack| {
            resolve_identifier(stack)
                .as_deref()
                .and_then(|id| stack_icon(runtime, presentation, stack, id, None))
        })
    };
    let held_item_icon = selected_stack.and_then(|stack| {
        resolve_identifier(stack).as_deref().and_then(|id| {
            stack_icon(
                runtime,
                presentation,
                stack,
                id,
                selected_slot.and_then(|slot| icon_frames.slot(slot)),
            )
        })
    });
    let selected_item_name = selected_stack.and_then(|stack| {
        let identifier = resolve_identifier(stack);
        let stated_name = player_runtime.selected_stack_custom_name();
        let display = protocol::item_display(&stack.extra_data);
        super::inventory_tooltip::name_line(
            runtime,
            identifier.as_deref(),
            stated_name.as_deref(),
            &display,
        )
        .map(|line| Arc::from(line.text))
    });
    let selected_identity = selected_stack
        .zip(selected_slot)
        .map(|(stack, slot)| (slot, stack.network_id, stack.metadata));
    let holding_filled_map = selected_stack
        .and_then(resolve_identifier)
        .is_some_and(|id| &*id == "minecraft:filled_map");
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
    runtime.observe_selected_item_identity_value(selected_identity, now_millis);
    let sleeping = stream
        .and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()))
        .is_some_and(|actor| actor.is_sleeping());
    runtime.set_local_sleeping(sleeping);
    let frame = presentation.hud_frame_mut();
    frame.sleep.observe(sleeping, now_millis);
    frame.first_person = first_person;
    frame.mount_health = mount_health;
    frame.hotbar_durability = hotbar_durability;
    frame.hotbar_stacks = hotbar_stacks;
    frame.offhand_durability = offhand_durability;
    frame.hotbar_icons = hotbar_icons;
    frame.inventory_icons = inventory_icons;
    frame.storage_icons = storage_icons;
    frame.crafting = crafting;
    frame.window_icons = window_icons;
    frame.durability = durability;
    frame.window_text = window_text;
    frame.cursor_icon = cursor_icon;
    frame.armor_icons = armor_icons;
    frame.offhand_icon = offhand_icon;
    frame.offhand_viewmodel_icon = None;
    frame.held_item_icon = None;
    frame.player_preview = player_preview_icon;
    frame.left_hand = left_hand_icon;
    frame.right_hand = right_hand_icon;
    frame.viewmodel_pitch_degrees = stream
        .and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()))
        .map_or(0.0, |actor| actor.pitch);
    frame.selected_item_name = selected_item_name;
    frame.holding_filled_map = holding_filled_map;
    frame.item_names = item_names;
    frame.mount_jump = mount_jump;
    frame.attack_indicator_charge = Some(1.0);
    let diagnostics = runtime.gameplay_hud().diagnostics();
    if diagnostics != presentation.last_hud_diagnostics {
        bevy::log::debug!(
            skipped_effect_actions = diagnostics.skipped_effect_actions,
            evicted_effects = diagnostics.evicted_effects,
            odd_metadata_values = diagnostics.odd_metadata_values,
            dropped_inventory_events = diagnostics.dropped_inventory_events,
            unknown_container_events = diagnostics.unknown_container_events,
            odd_attribute_values = diagnostics.odd_attribute_values,
            odd_hud_packets = diagnostics.odd_hud_packets,
            oversized_chat_rows = diagnostics.oversized_chat_rows,
            unknown_effect_ids = diagnostics.unknown_effect_ids,
            "gameplay HUD skipped odd remote data"
        );
        presentation.last_hud_diagnostics = diagnostics;
    }
    (held_item_icon, offhand_icon)
}

impl UiPresentationRuntime {
    /// Returns the previous frame when only the revision would differ, so the
    /// renderer keeps its accepted publication and skips re-uploading.
    pub(super) fn stabilize_revision(&mut self, mut input: UiRenderInput) -> UiRenderInput {
        if let Some(previous) = &self.last_input {
            input.revision = previous.revision;
            if *previous == input {
                return previous.clone();
            }
        }
        self.revision = self.revision.saturating_add(1);
        input.revision = self.revision;
        self.last_input = Some(input.clone());
        input
    }
}
