//! Dispatch of resolved inventory-screen gestures onto the ledger: cells,
//! screen widgets, result cells and the creative catalog.

use protocol::{CreativeItem, NetworkItemStack, RecipeHandle, WindowKind};

use super::UiRuntime;
use super::inventory_drag::PointerAction;
use super::presentation::inventory_pointer::InventoryScreen;
use super::presentation::screens::{BOOK_CELLS, GRID_CELLS, GRID_COLUMNS, LOOM_COLUMNS};
use inventory::inventory_ledger::{
    CellGesture, CraftSink, CreativeDestination, DistributeMode, InventoryGestureError,
    InventoryTarget, PlayerInventoryLedger, ScreenCraft,
};
use ui_screens::container_actions::{
    BEACON_LEVEL_FOR, InventoryCellHit, LOOM_PATTERNS, ReaderButton, SEARCH_TAB, Widget,
};
use ui_screens::recipe_book::BookEntry;

type Outcome = Result<i32, InventoryGestureError>;

/// The held stack and target stack a click starts from.
struct PlaceSnapshot {
    target: InventoryTarget,
    cursor: NetworkItemStack,
    stack: Option<NetworkItemStack>,
}

fn place_snapshot(
    player_runtime: &player_state::PlayerState,
    hit: InventoryCellHit,
) -> Option<PlaceSnapshot> {
    let target = gesture_target(hit)?;
    let ledger = player_runtime.inventory.ledger();
    Some(PlaceSnapshot {
        target,
        cursor: ledger.cursor_stack()?.clone(),
        stack: ledger.target_stack(target).cloned(),
    })
}

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

/// What the engine-drawn recipe book lists on the current tab: the creative
/// catalog in creative (a named group folds into its head until expanded;
/// search lists flat), else the craftable recipes whose output the catalog
/// files under that tab (every one without a catalog), or matching the search.
pub fn recipe_book_entries<'a>(
    player_runtime: &'a player_state::PlayerState,
    runtime: &UiRuntime,
) -> Vec<BookEntry<'a>> {
    if ui_screens::furnace_book::active(player_runtime) {
        return super::presentation::forms::furnace_book::entries(player_runtime, runtime)
            .iter()
            .map(BookEntry::Furnace)
            .collect();
    }
    let ledger = runtime.inventory_ledger(player_runtime);
    let state = runtime.screen_state();
    if player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative)
        && let Some(catalog) = ledger.creative_catalog()
    {
        let items = visible_creative_entries(player_runtime, runtime);
        if runtime.creative_tab(player_runtime) == SEARCH_TAB {
            return items
                .iter()
                .map(|item| BookEntry::Creative {
                    item,
                    grouped: false,
                })
                .collect();
        }
        let mut entries = Vec::with_capacity(items.len());
        let mut open: Option<u32> = None;
        for item in items.iter() {
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
    // The retained creative filter answers the same tab and search without renaming every item.
    let shown: std::collections::HashSet<i32> = visible_creative_entries(player_runtime, runtime)
        .iter()
        .map(|item| item.stack.network_id)
        .collect();
    runtime
        .book_recipes(player_runtime, 0, usize::MAX)
        .into_iter()
        .filter(|recipe| {
            ledger.creative_catalog().is_none()
                || shown.contains(&recipe.recipe.output().network_id)
        })
        .map(BookEntry::Recipe)
        .collect()
}

/// Projects the same component and localized display name used by item presentation.
fn item_name(
    ledger: &PlayerInventoryLedger,
    runtime: &UiRuntime,
    item: &CreativeItem,
) -> Option<String> {
    let entry = ledger.negotiated_item_entry(item.stack.network_id)?;
    Some(runtime.localized_item_name(&entry.identifier))
}

/// What a recipe book click lands on, detached from the runtime borrow.
enum Clicked {
    Item(u32),
    Group(u32),
    Recipe(RecipeHandle),
    Furnace(protocol::ScreenRecipe),
}

/// The catalog entries the creative screen currently lists, in grid order.
pub fn visible_creative_entries<'a>(
    player: &'a player_state::PlayerState,
    runtime: &UiRuntime,
) -> super::screen_state::CreativeEntries<'a> {
    let ledger = runtime.inventory_ledger(player);
    runtime.screen_state().matching_creative_entries(
        ledger,
        runtime.creative_tab(player),
        runtime.item_name_generation(),
        |item| item_name(ledger, runtime, item),
    )
}

impl UiRuntime {
    /// Releases the keyboard when the active inventory renderer cannot show search.
    pub fn observe_inventory_search_focus(
        &mut self,
        player: &player_state::PlayerState,
        engine_container_shown: bool,
    ) {
        // The fallback creative catalog keeps Search visible for every saved layout.
        let creative_fallback = !engine_container_shown
            && InventoryScreen::of_runtime(player, self) == InventoryScreen::Creative;
        if self.screen_state().book.is_some()
            || (!creative_fallback && !super::presentation::forms::recipe_book_shown(player, self))
        {
            self.screen_state_mut().search_focused = false;
        }
    }

    /// Drops the hovered catalog entry without moving it through the cursor.
    pub(super) fn drop_creative_hit(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        hit: InventoryCellHit,
        whole_stack: bool,
    ) -> Option<Outcome> {
        if player_runtime.facts.player_game_mode() != Some(protocol::PlayerGameMode::Creative) {
            return None;
        }
        let id = match hit {
            InventoryCellHit::CreativeGrid(index) => {
                let entries = visible_creative_entries(player_runtime, self);
                let position = self.screen_state().creative_row * GRID_COLUMNS + usize::from(index);
                entries.get(position).map(|item| item.creative_network_id)
            }
            InventoryCellHit::RecipeBook(index) => {
                if ui_screens::furnace_book::active(player_runtime) {
                    return None;
                }
                match recipe_book_entries(player_runtime, self).get(usize::from(index))? {
                    BookEntry::Creative { item, .. } => Some(item.creative_network_id),
                    BookEntry::Group { index, group, .. } => self
                        .inventory_ledger(player_runtime)
                        .creative_catalog()?
                        .items
                        .iter()
                        .find(|item| {
                            item.group == *index
                                && group.icon.as_ref().is_some_and(|icon| {
                                    icon.network_id == item.stack.network_id
                                        && icon.metadata == item.stack.metadata
                                        && icon.block_runtime_id == item.stack.block_runtime_id
                                        && icon.extra_data == item.stack.extra_data
                                })
                        })
                        .map(|item| item.creative_network_id),
                    BookEntry::Recipe(_) | BookEntry::Furnace(_) => return None,
                }
            }
            _ => return None,
        };
        Some(
            id.ok_or(InventoryGestureError::EmptyGesture)
                .and_then(|id| {
                    self.inventory_ledger_mut(player_runtime)
                        .begin_creative_take(
                            id,
                            CreativeDestination::Drop,
                            (!whole_stack).then_some(1),
                        )
                }),
        )
    }

    /// Runs one pointer action; refusals are ordinary (a busy or resyncing
    /// ledger) and simply drop the gesture.
    pub fn perform_pointer_action(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        action: PointerAction,
    ) {
        let _ = match action {
            PointerAction::Click(hit) => {
                let before = place_snapshot(player_runtime, hit);
                let result = self.click_hit(player_runtime, hit);
                self.seed_split(player_runtime, before, DistributeMode::Even);
                result
            }
            PointerAction::SecondaryClick(hit) => {
                let before = place_snapshot(player_runtime, hit);
                let result = self.secondary_click_hit(player_runtime, hit);
                self.seed_split(player_runtime, before, DistributeMode::One);
                result
            }
            PointerAction::QuickMove(hit) => self.quick_move_hit(player_runtime, hit),
            PointerAction::Distribute { cells, one_each } => {
                let targets: Vec<_> = cells.into_iter().filter_map(gesture_target).collect();
                let mode = if one_each {
                    DistributeMode::One
                } else {
                    DistributeMode::Even
                };
                let mut distribution = self.screen_state_mut().pointer.distribution.take();
                let result = self
                    .inventory_ledger_mut(player_runtime)
                    .advance_distribute(&mut distribution, &targets, mode);
                if result.is_ok() {
                    self.screen_state_mut().pointer.distribution = distribution;
                } else {
                    self.screen_state_mut().pointer.reset();
                }
                result.map(|request| request.unwrap_or(0))
            }
            PointerAction::EndDistribute => {
                self.screen_state_mut().pointer.distribution = None;
                Ok(0)
            }
            PointerAction::Gather => self.inventory_ledger_mut(player_runtime).begin_gather(),
        };
    }

    /// Lets a drag that continues from a press-time placement rebalance it.
    fn seed_split(
        &mut self,
        player_runtime: &player_state::PlayerState,
        before: Option<PlaceSnapshot>,
        mode: DistributeMode,
    ) {
        let Some(PlaceSnapshot {
            target,
            cursor,
            stack,
        }) = before
        else {
            return;
        };
        self.screen_state_mut().pointer.distribution = player_runtime
            .inventory
            .ledger()
            .distribution_after_place(&cursor, target, stack.as_ref(), mode);
    }

    fn click_hit(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        hit: InventoryCellHit,
    ) -> Outcome {
        if hit != InventoryCellHit::Widget(Widget::AnvilName) {
            self.screen_state_mut().anvil_focused = false;
        }
        match hit {
            InventoryCellHit::Widget(widget) => self.activate_widget(player_runtime, widget),
            InventoryCellHit::CreativeTab(tab) => self.select_inventory_tab(player_runtime, tab),
            InventoryCellHit::CreativeSearch
                if ui_screens::furnace_book::active(player_runtime) =>
            {
                self.screen_state_mut().search_focused = true;
                Ok(0)
            }
            InventoryCellHit::CreativeSearch => {
                self.select_inventory_tab(player_runtime, SEARCH_TAB)
            }
            InventoryCellHit::CreativeGrid(index) => {
                self.creative_click(player_runtime, index, false, None)
            }
            InventoryCellHit::RecipeBook(index) => {
                self.recipe_book_click(player_runtime, index, false, None)
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
    /// slot clicked with nothing held, as in vanilla's crafter screen.
    fn crafter_slot_disables(&self, player_runtime: &player_state::PlayerState, slot: u8) -> bool {
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
        player_runtime: &player_state::PlayerState,
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
        player_runtime: &mut player_state::PlayerState,
        hit: InventoryCellHit,
    ) -> Outcome {
        if hit == InventoryCellHit::CraftOutput {
            return self.output_click(player_runtime, false);
        }
        if let InventoryCellHit::CreativeGrid(index) = hit {
            return self.creative_click(player_runtime, index, false, Some(1));
        }
        if let InventoryCellHit::RecipeBook(index) = hit
            && player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative)
        {
            return self.recipe_book_click(player_runtime, index, false, Some(1));
        }
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
        player_runtime: &mut player_state::PlayerState,
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
        player_runtime: &player_state::PlayerState,
        hit: InventoryCellHit,
    ) -> Option<InventoryTarget> {
        let target = gesture_target(hit)?;
        let ledger = player_runtime.inventory.ledger();
        let held = ledger.cursor_stack()?;
        (protocol::item_bundle_id(&held.extra_data).is_none()
            && ledger.bundle_id_at(target).is_some())
        .then_some(target)
    }

    /// Shift-clicks a hit using the current mount equipment admission.
    fn quick_move_hit(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        hit: InventoryCellHit,
    ) -> Outcome {
        match hit {
            InventoryCellHit::CraftOutput => self.output_click(player_runtime, true),
            InventoryCellHit::CreativeGrid(index) => {
                self.creative_click(player_runtime, index, true, None)
            }
            InventoryCellHit::RecipeBook(index) => {
                self.recipe_book_click(player_runtime, index, true, None)
            }
            // Shift does not stop the close button closing the screen.
            InventoryCellHit::Widget(Widget::Close) => {
                self.activate_widget(player_runtime, Widget::Close)
            }
            hit => match gesture_target(hit) {
                Some(target) => {
                    let equipment = self.screen_state().mount_equipment;
                    let ledger = self.inventory_ledger_mut(player_runtime);
                    if ledger.window_kind() == Some(WindowKind::Horse) {
                        ledger.begin_quick_move_with_storage_filter(target, |slot| {
                            equipment.permits_inventory_slot(slot)
                        })
                    } else {
                        ledger.begin_quick_move(target)
                    }
                }
                None => Err(InventoryGestureError::InvalidRequest),
            },
        }
    }

    /// Takes a result: the grid's recipe on the personal and crafting-table
    /// screens, the derived or previewed output on the others.
    fn output_click(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
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
                let (rename, output, material_used) = if self
                    .inventory_ledger(player_runtime)
                    .created_output_stack()
                    .is_some()
                {
                    (self.anvil_rename(player_runtime), None, 0)
                } else {
                    let preview = self
                        .anvil_preview(player_runtime)
                        .ok_or(InventoryGestureError::EmptyGesture)?;
                    if !self.anvil_affordable(player_runtime, &preview) {
                        return Err(InventoryGestureError::InvalidRequest);
                    }
                    // Clearing a custom name sends empty text, so the server clears it too.
                    let rename = preview
                        .outcome
                        .custom_name
                        .clone()
                        .map(|name| std::sync::Arc::from(name.unwrap_or_default()));
                    (
                        rename,
                        Some(preview.stack.clone()),
                        preview.outcome.material_used,
                    )
                };
                self.inventory_ledger_mut(player_runtime)
                    .begin_screen_output(&ScreenCraft::Anvil {
                        rename,
                        multi_recipe_id,
                        output,
                        material_used,
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
            Some(WindowKind::Trade { .. }) => {
                self.inventory_ledger_mut(player_runtime).begin_trade()
            }
            Some(_) => Err(InventoryGestureError::InvalidRequest),
        }
    }

    fn activate_widget(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        widget: Widget,
    ) -> Outcome {
        match widget {
            Widget::TradeOffer { tier, row, .. } => {
                self.inventory_ledger_mut(player_runtime)
                    .select_trade_row(tier, row)?;
                Ok(0)
            }
            Widget::TradeCycle { next } => {
                self.inventory_ledger_mut(player_runtime)
                    .cycle_trade(next)?;
                Ok(0)
            }
            Widget::Trade => self.inventory_ledger_mut(player_runtime).begin_trade(),
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
            Widget::Close => {
                if self.inventory_open() {
                    self.toggle_inventory(player_runtime, None);
                }
                Ok(0)
            }
            Widget::BookToggle => {
                let shown = super::presentation::forms::recipe_book_shown(player_runtime, self);
                self.select_inventory_layout(player_runtime, if shown { 1 } else { 2 })
            }
            Widget::CrafterSlot(slot) => self.set_crafter_slot(player_runtime, slot, false),
            Widget::InventoryLayout(layout) => {
                if ui_screens::furnace_book::active(player_runtime) {
                    let layout = protocol::FurnaceLayout::from_index(i32::from(layout));
                    if !matches!(
                        layout,
                        protocol::FurnaceLayout::InventoryOnly
                            | protocol::FurnaceLayout::RecipeBook
                    ) {
                        return Err(InventoryGestureError::InvalidRequest);
                    }
                    let result = self
                        .change_furnace_options(player_runtime, |options| options.layout = layout);
                    if result.is_ok() && layout != protocol::FurnaceLayout::RecipeBook {
                        self.screen_state_mut().search_focused = false;
                    }
                    return result;
                }
                self.select_inventory_layout(player_runtime, layout)
            }
            Widget::RecipeFilter => {
                if ui_screens::furnace_book::active(player_runtime) {
                    return self.change_furnace_options(player_runtime, |options| {
                        options.filtering = !options.filtering
                    });
                }
                self.change_inventory_options(player_runtime, true, |controller| {
                    controller.options.filtering = !controller.options.filtering
                })
            }
            Widget::InventoryRightTab(tab) => self.select_inventory_right_tab(player_runtime, tab),
            Widget::FurnaceTab(tab) => {
                let tab = protocol::FurnaceTab::from_index(i32::from(tab));
                if matches!(
                    tab,
                    protocol::FurnaceTab::None | protocol::FurnaceTab::Unknown(_)
                ) {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                let result =
                    self.change_furnace_options(player_runtime, |options| options.left_tab = tab);
                if result.is_ok() {
                    self.screen_state_mut().search_focused = tab == protocol::FurnaceTab::Search;
                }
                result
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
                player_runtime.inventory.begin_book_craft(&recipe.recipe)
            }
            Widget::FurnaceClearRecipe => self
                .inventory_ledger_mut(player_runtime)
                .begin_clear_furnace_recipe(),
        }
    }

    /// A click on a recipe book entry: a creative item as on the catalog grid,
    /// else auto-crafting the recipe into the grid.
    fn recipe_book_click(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        index: u16,
        into_inventory: bool,
        requested: Option<u8>,
    ) -> Outcome {
        if ui_screens::furnace_book::active(player_runtime) {
            let recipe = super::presentation::forms::furnace_book::entries(player_runtime, self)
                .get(usize::from(index))
                .cloned()
                .ok_or(InventoryGestureError::InvalidRequest)?;
            return self
                .inventory_ledger_mut(player_runtime)
                .begin_furnace_recipe(&recipe);
        }
        let entry = recipe_book_entries(player_runtime, self)
            .into_iter()
            .nth(usize::from(index))
            .map(|entry| match entry {
                BookEntry::Creative { item, .. } => Clicked::Item(item.creative_network_id),
                BookEntry::Group { index, .. } => Clicked::Group(index),
                BookEntry::Recipe(recipe) => Clicked::Recipe(recipe.recipe),
                BookEntry::Furnace(recipe) => Clicked::Furnace(recipe.clone()),
            });
        match entry {
            Some(Clicked::Item(id)) => {
                self.creative_take(player_runtime, id, into_inventory, requested)
            }
            // A head folds or unfolds its group; a held stack still deletes.
            Some(Clicked::Group(_))
                if player_runtime.inventory.ledger().cursor_stack().is_some() =>
            {
                self.inventory_ledger_mut(player_runtime)
                    .begin_destroy_cursor(requested)
            }
            Some(Clicked::Group(index)) => {
                let expanded = &mut self.screen_state_mut().creative_expanded;
                if !expanded.remove(&index) {
                    expanded.insert(index);
                }
                Ok(0)
            }
            Some(Clicked::Recipe(recipe)) => player_runtime.inventory.begin_book_craft(&recipe),
            Some(Clicked::Furnace(recipe)) => self
                .inventory_ledger_mut(player_runtime)
                .begin_furnace_recipe(&recipe),
            None if player_runtime.facts.player_game_mode()
                == Some(protocol::PlayerGameMode::Creative)
                && player_runtime.inventory.ledger().cursor_stack().is_some() =>
            {
                self.inventory_ledger_mut(player_runtime)
                    .begin_destroy_cursor(requested)
            }
            None => Err(InventoryGestureError::EmptyGesture),
        }
    }

    /// A click on a catalog cell: take the item, or delete the held stack.
    fn creative_click(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        index: u8,
        into_inventory: bool,
        requested: Option<u8>,
    ) -> Outcome {
        if player_runtime.facts.player_game_mode() != Some(protocol::PlayerGameMode::Creative) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        if player_runtime.inventory.ledger().cursor_stack().is_some() {
            return self
                .inventory_ledger_mut(player_runtime)
                .begin_destroy_cursor(requested);
        }
        let id = {
            let entries = visible_creative_entries(player_runtime, self);
            let position = self.screen_state().creative_row * GRID_COLUMNS + usize::from(index);
            if usize::from(index) >= GRID_CELLS {
                return Err(InventoryGestureError::InvalidRequest);
            }
            entries.get(position).map(|item| item.creative_network_id)
        };
        let id = id.ok_or(InventoryGestureError::EmptyGesture)?;
        self.creative_take(player_runtime, id, into_inventory, requested)
    }

    /// Takes catalog item `id` to the cursor (or the first free inventory cell),
    /// or deletes the held stack.
    fn creative_take(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        id: u32,
        into_inventory: bool,
        requested: Option<u8>,
    ) -> Outcome {
        if player_runtime.facts.player_game_mode() != Some(protocol::PlayerGameMode::Creative) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        if player_runtime.inventory.ledger().cursor_stack().is_some() {
            return self
                .inventory_ledger_mut(player_runtime)
                .begin_destroy_cursor(requested);
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
                            Some(inventory::inventory_ledger::PlayerInventorySlot::Empty)
                        )
                })
                .map_or(CreativeDestination::Cursor, CreativeDestination::Player)
        } else {
            CreativeDestination::Cursor
        };
        self.inventory_ledger_mut(player_runtime)
            .begin_creative_take(id, destination, requested)
    }

    /// Crafts the grid's recipe into hotbar `slot` while the pointer is over the result.
    pub fn craft_into_hotbar(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        slot: u8,
    ) -> Outcome {
        self.begin_crafting_into(player_runtime, CraftSink::Player(slot))
    }
}

#[cfg(test)]
#[path = "inventory_actions/tests.rs"]
mod tests;
