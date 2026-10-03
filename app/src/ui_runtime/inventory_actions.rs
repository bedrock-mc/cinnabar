//! Dispatch of resolved inventory-screen gestures onto the ledger: cells,
//! screen widgets, result cells and the creative catalog.

use protocol::{CreativeGroup, CreativeItem, NetworkItemStack, RecipeHandle, WindowKind};

use super::UiRuntime;
use super::inventory_drag::PointerAction;
use super::inventory_ledger::{
    CellGesture, CraftSink, CreativeDestination, DistributeMode, InventoryGestureError,
    InventoryTarget, PlayerInventoryLedger, ScreenCraft,
};
use super::presentation::inventory_pointer::InventoryCellHit;
use super::presentation::screens::{
    BEACON_LEVEL_FOR, BOOK_CELLS, GRID_CELLS, GRID_COLUMNS, LOOM_COLUMNS, ReaderButton, Widget,
};
use super::screen_recipes::LOOM_PATTERNS;
use super::screen_state::{ScreenState, creative_entries};

type Outcome = Result<i32, InventoryGestureError>;

/// The ledger target a cell hit addresses; output, widget and catalog hits have none.
pub(super) const fn gesture_target(hit: InventoryCellHit) -> Option<InventoryTarget> {
    Some(match hit {
        InventoryCellHit::Player(slot) => InventoryTarget::Player(slot),
        InventoryCellHit::Storage(slot) => InventoryTarget::Storage(slot),
        InventoryCellHit::Armor(slot) => InventoryTarget::Armor(slot),
        InventoryCellHit::Offhand => InventoryTarget::Offhand,
        InventoryCellHit::Craft(slot) => InventoryTarget::Craft(slot),
        InventoryCellHit::CraftOutput
        | InventoryCellHit::Widget(_)
        | InventoryCellHit::CreativeGrid(_)
        | InventoryCellHit::CreativeTab(_)
        | InventoryCellHit::CreativeSearch
        | InventoryCellHit::RecipeBook(_) => return None,
    })
}

/// One entry of the recipe book: in creative a catalog item or a named group's
/// head, else a recipe the player can craft now.
pub(crate) enum BookEntry<'a> {
    Creative {
        item: &'a CreativeItem,
        /// Listed under an expanded group's head.
        grouped: bool,
    },
    Group {
        index: u32,
        group: &'a CreativeGroup,
        expanded: bool,
    },
    Recipe(RecipeHandle),
}

impl BookEntry<'_> {
    /// The stack the entry shows; a group head shows its icon.
    pub(crate) fn stack(&self) -> NetworkItemStack {
        match self {
            Self::Creative { item, .. } => item.stack.clone(),
            Self::Group { group, .. } => group.icon.clone().unwrap_or_default(),
            Self::Recipe(recipe) => {
                let output = recipe.output();
                NetworkItemStack {
                    network_id: output.network_id,
                    metadata: u32::from(output.aux),
                    count: u16::from(output.count),
                    block_runtime_id: i32::try_from(output.block_runtime_id).unwrap_or(0),
                    ..NetworkItemStack::empty()
                }
            }
        }
    }
}

/// What the engine-drawn recipe book lists on the current tab: the creative
/// catalog in creative (a named group folds into its head until expanded;
/// search lists flat), else the craftable recipes whose output the catalog
/// files under that tab (every one without a catalog), or matching the search.
pub(crate) fn recipe_book_entries<'a>(
    player_runtime: &'a crate::player_runtime::PlayerRuntime,
    runtime: &UiRuntime,
) -> Vec<BookEntry<'a>> {
    let ledger = runtime.inventory_ledger(player_runtime);
    let state = runtime.screen_state();
    if runtime.player_game_mode(player_runtime) == Some(protocol::PlayerGameMode::Creative)
        && let Some(catalog) = ledger.creative_catalog()
    {
        let items = visible_creative_entries(ledger, state);
        if state.creative_tab == super::presentation::screens::SEARCH_TAB {
            return items
                .into_iter()
                .map(|item| BookEntry::Creative {
                    item,
                    grouped: false,
                })
                .collect();
        }
        let mut entries = Vec::with_capacity(items.len());
        let mut open: Option<u32> = None;
        for item in items {
            let group = catalog
                .groups
                .get(item.group as usize)
                .filter(|group| !group.name.is_empty());
            let Some(group) = group else {
                open = None;
                entries.push(BookEntry::Creative {
                    item,
                    grouped: false,
                });
                continue;
            };
            let expanded = state.creative_expanded.contains(&item.group);
            if open != Some(item.group) {
                open = Some(item.group);
                entries.push(BookEntry::Group {
                    index: item.group,
                    group,
                    expanded,
                });
            }
            if expanded {
                entries.push(BookEntry::Creative {
                    item,
                    grouped: true,
                });
            }
        }
        return entries;
    }
    let shown: std::collections::HashSet<i32> = ledger
        .creative_catalog()
        .map(|catalog| {
            creative_entries(catalog, state.creative_tab, &state.search, |item| {
                item_name(ledger, item)
            })
            .iter()
            .map(|item| item.stack.network_id)
            .collect()
        })
        .unwrap_or_default();
    runtime
        .book_recipes(player_runtime, 0, usize::MAX)
        .into_iter()
        .filter(|recipe| {
            ledger.creative_catalog().is_none() || shown.contains(&recipe.output().network_id)
        })
        .map(BookEntry::Recipe)
        .collect()
}

fn item_name(ledger: &PlayerInventoryLedger, item: &CreativeItem) -> Option<String> {
    let entry = ledger.negotiated_item_entry(item.stack.network_id)?;
    let name = entry.identifier.strip_prefix("minecraft:")?;
    Some(name.replace('_', " "))
}

/// What a recipe book click lands on, detached from the runtime borrow.
enum Clicked {
    Item(u32),
    Group(u32),
    Recipe(RecipeHandle),
}

/// The catalog entries the creative screen currently lists, in grid order.
pub(crate) fn visible_creative_entries<'a>(
    ledger: &'a PlayerInventoryLedger,
    state: &ScreenState,
) -> Vec<&'a CreativeItem> {
    let Some(catalog) = ledger.creative_catalog() else {
        return Vec::new();
    };
    creative_entries(catalog, state.creative_tab, &state.search, |item| {
        item_name(ledger, item)
    })
}

impl UiRuntime {
    /// Runs one pointer action; refusals are ordinary (a busy or resyncing
    /// ledger) and simply drop the gesture.
    pub(crate) fn perform_pointer_action(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        action: PointerAction,
    ) {
        let _ = match action {
            PointerAction::Click(hit) => self.click_hit(player_runtime, hit),
            PointerAction::SecondaryClick(hit) => self.secondary_click_hit(player_runtime, hit),
            PointerAction::QuickMove(hit) => self.quick_move_hit(player_runtime, hit),
            PointerAction::Distribute { cells, one_each } => {
                let targets: Vec<_> = cells.into_iter().filter_map(gesture_target).collect();
                let mode = if one_each {
                    DistributeMode::One
                } else {
                    DistributeMode::Even
                };
                self.inventory_ledger_mut(player_runtime)
                    .begin_distribute(&targets, mode)
            }
            PointerAction::Gather => self.inventory_ledger_mut(player_runtime).begin_gather(),
        };
    }

    fn click_hit(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        hit: InventoryCellHit,
    ) -> Outcome {
        if hit != InventoryCellHit::Widget(Widget::AnvilName) {
            self.screen_state_mut().anvil_focused = false;
        }
        match hit {
            InventoryCellHit::Widget(widget) => self.activate_widget(player_runtime, widget),
            InventoryCellHit::CreativeTab(tab) => {
                self.screen_state_mut().select_tab(tab);
                Ok(0)
            }
            InventoryCellHit::CreativeSearch => {
                self.screen_state_mut()
                    .select_tab(super::presentation::screens::SEARCH_TAB);
                Ok(0)
            }
            InventoryCellHit::CreativeGrid(index) => {
                self.creative_click(player_runtime, index, false)
            }
            InventoryCellHit::RecipeBook(index) => {
                self.recipe_book_click(player_runtime, index, false)
            }
            InventoryCellHit::CraftOutput => self.output_click(player_runtime, false),
            InventoryCellHit::Storage(slot) if self.crafter_slot_disables(player_runtime, slot) => {
                self.set_crafter_slot(player_runtime, slot, true)
            }
            hit if self.bundle_insert_target(player_runtime, hit).is_some() => {
                let target = self
                    .bundle_insert_target(player_runtime, hit)
                    .expect("checked by the guard");
                self.inventory_ledger_mut(player_runtime)
                    .begin_bundle_insert(target)
            }
            hit => match gesture_target(hit) {
                Some(target) => self
                    .inventory_ledger_mut(player_runtime)
                    .begin_target_gesture(target, CellGesture::Click),
                None => Err(InventoryGestureError::InvalidRequest),
            },
        }
    }

    /// Whether a click on crafter slot `slot` disables it: an empty, enabled
    /// slot clicked with nothing held, as `CrafterScreenController::handleEvent`.
    fn crafter_slot_disables(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        slot: u8,
    ) -> bool {
        let ledger = player_runtime.inventory.ledger();
        ledger.window_kind() == Some(WindowKind::Crafter)
            && slot < 9
            && ledger.cursor_stack().is_none()
            && ledger
                .target_stack(InventoryTarget::Storage(slot))
                .is_none()
            && !self.screen_state().crafter.is_disabled(slot)
    }

    /// Shows crafter slot `slot` toggled at once and asks the server to follow.
    fn set_crafter_slot(
        &mut self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        slot: u8,
        disabled: bool,
    ) -> Outcome {
        let position = player_runtime
            .inventory
            .ledger()
            .window_position()
            .filter(|_| slot < 9)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let crafter = &mut self.screen_state_mut().crafter;
        let bit = 1 << slot;
        let shown = crafter.shown_disabled();
        let mask = if disabled { shown | bit } else { shown & !bit };
        crafter.pending = Some((mask, None));
        self.queue_client_packet(protocol::crafter_slot_toggle_packet(
            position, slot, disabled,
        ));
        Ok(0)
    }

    fn secondary_click_hit(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        hit: InventoryCellHit,
    ) -> Outcome {
        let Some(target) = gesture_target(hit) else {
            return Err(InventoryGestureError::InvalidRequest);
        };
        let ledger = player_runtime.inventory.ledger();
        if ledger.cursor_stack().is_none() && ledger.bundle_id_at(target).is_some() {
            return self
                .inventory_ledger_mut(player_runtime)
                .begin_bundle_extract(target);
        }
        let ledger = player_runtime.inventory.ledger();
        let gesture = match (ledger.cursor_stack(), ledger.target_stack(target)) {
            (Some(_), _) => CellGesture::PlaceCount(1),
            (None, Some(stack)) => CellGesture::TakeCount(stack.count.div_ceil(2)),
            (None, None) => return Err(InventoryGestureError::EmptyGesture),
        };
        self.inventory_ledger_mut(player_runtime)
            .begin_target_gesture(target, gesture)
    }

    /// Runs one book reader or editor button.
    fn reader_button(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        button: ReaderButton,
    ) {
        let Some(book) = self.screen_state_mut().book.as_mut() else {
            return;
        };
        match button {
            ReaderButton::Prev => {
                if book.turn(-1) {
                    self.report_lectern_page();
                }
            }
            ReaderButton::Next => {
                // The last page's arrow starts a new page in a writable book.
                let last = book.page + 1 == book.pages.len();
                if (last && book.add_page()) || (!last && book.turn(1)) {
                    self.report_lectern_page();
                }
            }
            ReaderButton::Done => self.finish_book(player_runtime, false),
            ReaderButton::Sign => book.signing = true,
            ReaderButton::Finalize => self.finish_book(player_runtime, true),
            ReaderButton::Cancel => book.signing = false,
            ReaderButton::PrevSpread => {
                if book.prev_spread() {
                    self.report_lectern_page();
                }
            }
            ReaderButton::NextSpread => {
                if book.next_spread() {
                    self.report_lectern_page();
                }
            }
            ReaderButton::EditPage(side) => {
                let at = book.spread() + usize::from(side);
                book.editing = (book.editing != Some(at)).then_some(at);
            }
            ReaderButton::InsertPage(side) => book.insert_page(book.spread() + usize::from(side)),
            ReaderButton::DeletePage(side) => book.delete_page(book.spread() + usize::from(side)),
            ReaderButton::SwapLeft(side) => {
                let at = book.spread() + usize::from(side);
                if let Some(with) = at.checked_sub(1) {
                    book.swap_pages(at, with);
                }
            }
            ReaderButton::SwapRight(side) => {
                let at = book.spread() + usize::from(side);
                book.swap_pages(at, at + 1);
            }
            ReaderButton::FocusPage(side) => {
                let at = book.spread() + usize::from(side);
                if at < book.pages.len() {
                    book.page = at;
                }
            }
        }
    }

    /// The bundle under `hit` when the cursor holds a non-bundle item to put in it.
    fn bundle_insert_target(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        hit: InventoryCellHit,
    ) -> Option<InventoryTarget> {
        let target = gesture_target(hit)?;
        let ledger = player_runtime.inventory.ledger();
        let held = ledger.cursor_stack()?;
        (protocol::item_bundle_id(&held.extra_data).is_none()
            && ledger.bundle_id_at(target).is_some())
        .then_some(target)
    }

    fn quick_move_hit(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        hit: InventoryCellHit,
    ) -> Outcome {
        match hit {
            InventoryCellHit::CraftOutput => self.output_click(player_runtime, true),
            InventoryCellHit::CreativeGrid(index) => {
                self.creative_click(player_runtime, index, true)
            }
            InventoryCellHit::RecipeBook(index) => {
                self.recipe_book_click(player_runtime, index, true)
            }
            hit => match gesture_target(hit) {
                Some(target) => self
                    .inventory_ledger_mut(player_runtime)
                    .begin_quick_move(target),
                None => Err(InventoryGestureError::InvalidRequest),
            },
        }
    }

    /// Takes a result: the grid's recipe on the personal and crafting-table
    /// screens, the derived or previewed output on the others.
    fn output_click(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        all: bool,
    ) -> Outcome {
        match player_runtime.inventory.ledger().window_kind() {
            None | Some(WindowKind::Workbench) => {
                if all {
                    self.begin_crafting_all(player_runtime)
                } else {
                    self.begin_crafting(player_runtime)
                }
            }
            Some(WindowKind::Anvil) => {
                let multi_recipe_id = self
                    .screen_catalog(player_runtime)
                    .and_then(|catalog| catalog.repair_multi_recipe_id())
                    .unwrap_or(0);
                let name = self.screen_state().anvil_name.trim();
                let rename = (!name.is_empty()).then(|| std::sync::Arc::from(name));
                self.inventory_ledger_mut(player_runtime)
                    .begin_screen_output(&ScreenCraft::Anvil {
                        rename,
                        multi_recipe_id,
                    })
            }
            Some(WindowKind::Grindstone) => self
                .inventory_ledger_mut(player_runtime)
                .begin_screen_output(&ScreenCraft::Grindstone {
                    recipe_network_id: 0,
                    repair_cost: 0,
                }),
            Some(WindowKind::Loom) => {
                let pattern = self
                    .screen_state()
                    .loom_pattern
                    .clone()
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                self.inventory_ledger_mut(player_runtime)
                    .begin_screen_output(&ScreenCraft::Loom { pattern })
            }
            Some(WindowKind::Stonecutter | WindowKind::Smithing | WindowKind::Cartography) => {
                let (recipe_network_id, output) = self
                    .active_screen_recipe(player_runtime)
                    .and_then(|recipe| Some((recipe.id, recipe.output?)))
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                self.inventory_ledger_mut(player_runtime)
                    .begin_screen_output(&ScreenCraft::Predicted {
                        recipe_network_id,
                        output,
                    })
            }
            Some(_) => Err(InventoryGestureError::InvalidRequest),
        }
    }

    fn activate_widget(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        widget: Widget,
    ) -> Outcome {
        match widget {
            Widget::EnchantOption(index) => {
                let id = player_runtime
                    .inventory
                    .ledger()
                    .enchant_options()
                    .and_then(|options| options.get(usize::from(index)))
                    .map(|option| option.network_id)
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                self.inventory_ledger_mut(player_runtime).begin_enchant(id)
            }
            Widget::BeaconEffect { id, secondary } => {
                let state = self.screen_state_mut();
                let unlocked = BEACON_LEVEL_FOR
                    .iter()
                    .find(|(effect, _)| *effect == id)
                    .is_some_and(|(_, needed)| {
                        state.beacon_level.is_none_or(|level| level >= *needed)
                    });
                if !unlocked {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                if secondary {
                    state.beacon.1 = id;
                } else {
                    state.beacon.0 = id;
                }
                Ok(0)
            }
            Widget::BeaconUpgrade => {
                let state = self.screen_state_mut();
                if state.beacon.0 == 0 || state.beacon_level.is_some_and(|level| level < 4) {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                state.beacon.1 = state.beacon.0;
                Ok(0)
            }
            Widget::BeaconConfirm => {
                let (primary, secondary) = self.screen_state().beacon;
                if primary == 0 {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                self.inventory_ledger_mut(player_runtime)
                    .begin_beacon_payment(primary, secondary)
            }
            Widget::StonecutterRecipe(index) => {
                let id = self
                    .stonecutter_options(player_runtime)
                    .get(usize::from(index))
                    .map(|recipe| recipe.id)
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                self.screen_state_mut().recipe_choice = Some(id);
                Ok(0)
            }
            Widget::LoomPattern(index) => {
                let position = self.screen_state().loom_row * LOOM_COLUMNS + usize::from(index);
                let pattern = LOOM_PATTERNS
                    .get(position)
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                self.screen_state_mut().loom_pattern = Some(std::sync::Arc::from(*pattern));
                Ok(0)
            }
            Widget::LoomPatternAt(index) => {
                let pattern = LOOM_PATTERNS
                    .get(usize::from(index))
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                self.screen_state_mut().loom_pattern = Some(std::sync::Arc::from(*pattern));
                Ok(0)
            }
            Widget::AnvilName => {
                self.screen_state_mut().anvil_focused = true;
                Ok(0)
            }
            Widget::Reader(button) => {
                self.reader_button(player_runtime, button);
                Ok(0)
            }
            Widget::BookToggle => {
                let state = self.screen_state_mut();
                state.book_open = !state.book_open;
                state.book_page = 0;
                Ok(0)
            }
            Widget::CrafterSlot(slot) => self.set_crafter_slot(player_runtime, slot, false),
            Widget::InventoryLayout(layout) => {
                let creative = player_runtime.facts.player_game_mode()
                    == Some(protocol::PlayerGameMode::Creative);
                let state = self.screen_state_mut();
                // The book shows when `book_open` differs from creative's default.
                state.book_open = (layout != 1) != creative;
                state.creative_wide = creative && layout == 3;
                state.book_page = 0;
                state.container_scroll.clear();
                Ok(0)
            }
            Widget::RecipeFilter => {
                let filtering = self.recipe_filtering(player_runtime);
                self.screen_state_mut().recipe_filtering = Some(!filtering);
                self.screen_state_mut().container_scroll.clear();
                Ok(0)
            }
            Widget::BookPage { next } => {
                let page = self.screen_state().book_page;
                let target = if next {
                    page + 1
                } else {
                    page.saturating_sub(1)
                };
                if next
                    && self
                        .book_recipes(player_runtime, target * BOOK_CELLS, 1)
                        .is_empty()
                {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                self.screen_state_mut().book_page = target;
                Ok(0)
            }
            Widget::BookRecipe(index) => {
                let skip = self.screen_state().book_page * BOOK_CELLS + usize::from(index);
                let recipe = self
                    .book_recipes(player_runtime, skip, 1)
                    .into_iter()
                    .next()
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                self.inventory_ledger_mut(player_runtime)
                    .begin_auto_craft(&recipe)
            }
        }
    }

    /// A click on a recipe book entry: a creative item as on the catalog grid,
    /// else auto-crafting the recipe into the grid.
    fn recipe_book_click(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        index: u16,
        into_inventory: bool,
    ) -> Outcome {
        let entry = recipe_book_entries(player_runtime, self)
            .into_iter()
            .nth(usize::from(index))
            .map(|entry| match entry {
                BookEntry::Creative { item, .. } => Clicked::Item(item.creative_network_id),
                BookEntry::Group { index, .. } => Clicked::Group(index),
                BookEntry::Recipe(recipe) => Clicked::Recipe(recipe),
            });
        match entry {
            Some(Clicked::Item(id)) => self.creative_take(player_runtime, id, into_inventory),
            // A head folds or unfolds its group; a held stack still deletes.
            Some(Clicked::Group(_))
                if player_runtime.inventory.ledger().cursor_stack().is_some() =>
            {
                self.inventory_ledger_mut(player_runtime)
                    .begin_destroy_cursor()
            }
            Some(Clicked::Group(index)) => {
                let expanded = &mut self.screen_state_mut().creative_expanded;
                if !expanded.remove(&index) {
                    expanded.insert(index);
                }
                Ok(0)
            }
            Some(Clicked::Recipe(recipe)) => self
                .inventory_ledger_mut(player_runtime)
                .begin_auto_craft(&recipe),
            None if player_runtime.inventory.ledger().cursor_stack().is_some() => self
                .inventory_ledger_mut(player_runtime)
                .begin_destroy_cursor(),
            None => Err(InventoryGestureError::EmptyGesture),
        }
    }

    /// A click on a catalog cell: take the item, or delete the held stack.
    fn creative_click(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        index: u8,
        into_inventory: bool,
    ) -> Outcome {
        if player_runtime.inventory.ledger().cursor_stack().is_some() {
            return self
                .inventory_ledger_mut(player_runtime)
                .begin_destroy_cursor();
        }
        let id = {
            let entries =
                visible_creative_entries(player_runtime.inventory.ledger(), self.screen_state());
            let position = self.screen_state().creative_row * GRID_COLUMNS + usize::from(index);
            if usize::from(index) >= GRID_CELLS {
                return Err(InventoryGestureError::InvalidRequest);
            }
            entries.get(position).map(|item| item.creative_network_id)
        };
        let id = id.ok_or(InventoryGestureError::EmptyGesture)?;
        self.creative_take(player_runtime, id, into_inventory)
    }

    /// Takes catalog item `id` to the cursor (or the first free inventory cell),
    /// or deletes the held stack.
    fn creative_take(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        id: u32,
        into_inventory: bool,
    ) -> Outcome {
        if player_runtime.inventory.ledger().cursor_stack().is_some() {
            return self
                .inventory_ledger_mut(player_runtime)
                .begin_destroy_cursor();
        }
        let destination = if into_inventory {
            let ledger = player_runtime.inventory.ledger();
            (0..protocol::PLAYER_INVENTORY_SLOTS)
                .find(|slot| {
                    ledger
                        .target_stack(InventoryTarget::Player(*slot))
                        .is_none()
                        && matches!(
                            ledger.slot_state(*slot),
                            Some(super::inventory_ledger::PlayerInventorySlot::Empty)
                        )
                })
                .map_or(CreativeDestination::Cursor, CreativeDestination::Player)
        } else {
            CreativeDestination::Cursor
        };
        self.inventory_ledger_mut(player_runtime)
            .begin_creative_take(id, destination)
    }

    /// Crafts the grid's recipe into hotbar `slot` while the pointer is over the result.
    pub(crate) fn craft_into_hotbar(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        slot: u8,
    ) -> Outcome {
        self.begin_crafting_into(player_runtime, CraftSink::Player(slot))
    }
}
