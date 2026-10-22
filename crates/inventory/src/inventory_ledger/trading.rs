//! The open trader's offers, selection and item identities.

use super::{Cell, InventoryGestureError, PlayerInventoryLedger, ScreenCraft};

#[cfg(test)]
mod tests;
use protocol::{NetworkItemStack, TradeEvent, TradeItem, TradeOffer, WindowKind};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// An offer with item names resolved once against the negotiated registry.
#[derive(Debug, Clone)]
pub struct ResolvedTrade {
    pub offer: TradeOffer,
    pub inputs: [Option<NetworkItemStack>; 2],
    pub output: Option<NetworkItemStack>,
    stock: TradeStock,
}

/// The server-owned trading window and the player's selected offer.
#[derive(Debug, Clone)]
pub struct TradeState {
    pub title: Arc<str>,
    pub tier: i32,
    pub offers: Arc<[ResolvedTrade]>,
    pub selected: Option<usize>,
}

/// Server-reported uses that already account for unanswered purchases.
#[derive(Debug, Clone, Default)]
struct TradeStock {
    credits: usize,
    through_request: Option<i32>,
}

impl TradeStock {
    /// Keeps credits on requests that existed when their stock was observed.
    fn includes(&self, request_id: i32) -> bool {
        self.through_request
            .is_some_and(|through| request_id >= through)
    }

    /// Reconciles a new authoritative count with the current in-flight request cohort.
    fn observe(&mut self, uses: i32, previous: i32, admitted: &[i32]) {
        self.credits = self
            .credits
            .min(admitted.iter().filter(|id| self.includes(**id)).count());
        let added = (i64::from(uses) - i64::from(previous)).max(0) as usize;
        if added != 0
            && let Some(through) = admitted.iter().min()
        {
            // Local request IDs decrease; newer purchases stay outside this observation.
            self.through_request = Some(
                self.through_request
                    .map_or(*through, |old| old.min(*through)),
            );
            self.credits = self.credits.saturating_add(added).min(admitted.len());
        }
    }

    /// Consumes a reported use only for an accepted member of its request cohort.
    fn accept(&mut self, request_id: i32) -> bool {
        if self.credits != 0 && self.includes(request_id) {
            self.credits -= 1;
            true
        } else {
            false
        }
    }
}

impl PlayerInventoryLedger {
    /// Selects a row within the sorted server-authored tier groups.
    pub fn select_trade_row(
        &mut self,
        tier_index: usize,
        row: usize,
    ) -> Result<(), InventoryGestureError> {
        let trade = self
            .trading()
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let index = trade
            .row_index(tier_index, row)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        self.select_trade(index)
    }

    /// Moves the classic screen's selector to the adjacent unlocked offer.
    pub fn cycle_trade(&mut self, next: bool) -> Result<(), InventoryGestureError> {
        let trade = self
            .trading()
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let index = trade
            .adjacent_offer(next)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        self.select_trade(index)
    }
    /// Exposes offers only while the owning trader remains the active window.
    pub fn trading(&self) -> Option<&TradeState> {
        self.storage.as_ref()?.trade.as_ref()
    }

    /// Resolves a saved trade item without changing its metadata, count or item tags.
    fn resolve_trade_item(&self, item: &TradeItem) -> Option<NetworkItemStack> {
        let entry = self
            .item_registry
            .as_ref()?
            .values()
            .find(|entry| entry.identifier == item.identifier)?;
        let extra_data =
            if item.identifier.as_ref() == assets::facts::VanillaItem::Shield.identifier() {
                item.extra_data_with_blocking_tick(0)
            } else {
                Arc::clone(&item.extra_data)
            };
        let nbt_digest = Sha256::digest(&extra_data).into();
        Some(NetworkItemStack {
            network_id: entry.network_id,
            metadata: item.metadata,
            count: item.count,
            extra_data,
            nbt_digest,
            ..NetworkItemStack::empty()
        })
    }

    /// Resolves every item of an offer through the current session's item registry.
    fn resolve_trade(&self, offer: &TradeOffer) -> ResolvedTrade {
        ResolvedTrade {
            inputs: std::array::from_fn(|index| {
                offer.inputs[index]
                    .as_ref()
                    .and_then(|item| self.resolve_trade_item(item))
            }),
            output: self.resolve_trade_item(&offer.output),
            offer: offer.clone(),
            stock: TradeStock::default(),
        }
    }

    /// Rebuilds cached offer identities when a server replaces its item registry.
    pub(super) fn refresh_trade_items(&mut self) {
        let Some(offers) = self.trading().map(|trade| Arc::clone(&trade.offers)) else {
            return;
        };
        let resolved = offers
            .iter()
            .map(|trade| {
                let mut resolved = self.resolve_trade(&trade.offer);
                resolved.stock = trade.stock.clone();
                resolved
            })
            .collect();
        self.storage
            .as_mut()
            .and_then(|window| window.trade.as_mut())
            .expect("retained trader")
            .offers = resolved;
    }

    /// Opens from UpdateTrade itself and refreshes repeated offers without retiring item moves.
    pub(super) fn apply_trade(&mut self, event: &TradeEvent) {
        let kind = WindowKind::Trade {
            modern: event.modern,
        };
        let same = self.storage.as_ref().is_some_and(|window| {
            Some(window.window_id) == event.container.window_id
                && window.kind == kind
                && window.actor_unique_id == event.actor_unique_id
        });
        if same && self.storage.as_ref().is_some_and(|window| window.closing) {
            return;
        }
        let previous = same
            .then(|| self.trading())
            .flatten()
            .map(|trade| Arc::clone(&trade.offers));
        let selected_id = same
            .then(|| self.trading())
            .flatten()
            .and_then(|trade| trade.offers.get(trade.selected?))
            .map(|trade| trade.offer.network_id);
        if !same {
            self.apply_open(protocol::ContainerOpenEvent {
                container: event.container,
                window_type: protocol::WINDOW_TYPE_TRADE,
                position: [0; 3],
                runtime_entity_id: event.actor_unique_id,
            });
        }
        if !self.storage.as_ref().is_some_and(|window| {
            Some(window.window_id) == event.container.window_id
                && window.actor_unique_id == event.actor_unique_id
        }) {
            return;
        }
        let generation = self.storage.as_ref().expect("admitted trader").generation;
        let offers: Arc<[ResolvedTrade]> = event
            .offers
            .iter()
            .map(|offer| {
                let mut resolved = self.resolve_trade(offer);
                if let Some(previous) = previous.as_ref().and_then(|offers| {
                    offers
                        .iter()
                        .find(|previous| previous.offer.network_id == offer.network_id)
                }) {
                    resolved.stock = previous.stock.clone();
                    let admitted: Vec<_> = self
                        .trade_requests(generation, offer.network_id)
                        .filter(|request| {
                            request.state == super::InventoryPendingState::AwaitingResponse
                        })
                        .map(|request| request.request_id)
                        .collect();
                    resolved
                        .stock
                        .observe(offer.uses, previous.offer.uses, &admitted);
                }
                resolved
            })
            .collect();
        let selected = selected_id
            .and_then(|id| offers.iter().position(|trade| trade.offer.network_id == id))
            .or_else(|| {
                offers
                    .iter()
                    .position(|trade| trade.offer.tier <= event.tier)
            });
        let previous_selection = previous.as_ref().and_then(|offers| {
            offers
                .iter()
                .find(|trade| Some(trade.offer.network_id) == selected_id)
        });
        let next_selection = selected.and_then(|index| offers.get(index));
        let changed = previous_selection
            .map(|trade| (&trade.offer.network_id, &trade.offer.output))
            != next_selection.map(|trade| (&trade.offer.network_id, &trade.offer.output));
        if changed {
            self.confirmed.clear_result();
            self.refold();
        }
        let window = self.storage.as_mut().expect("admitted trader");
        window.kind = kind;
        window.trade = Some(TradeState {
            title: Arc::clone(&event.title),
            tier: event.tier,
            offers,
            selected,
        });
    }

    /// Selects an offered recipe without sending an item mutation or discarding inputs.
    pub fn select_trade(&mut self, index: usize) -> Result<(), InventoryGestureError> {
        let trade = self
            .storage
            .as_mut()
            .and_then(|window| window.trade.as_mut())
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let offer = trade
            .offers
            .get(index)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        if offer.offer.tier > trade.tier {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let changed = trade.selected != Some(index);
        trade.selected = Some(index);
        if changed {
            self.confirmed.clear_result();
            self.refold();
        }
        Ok(())
    }

    /// Returns the current result only when each required input and the offer are usable.
    pub fn trade_output(&self) -> Option<&NetworkItemStack> {
        let state = self.trading()?;
        let trade = state.offers.get(state.selected?)?;
        if !self.trade_offer_available(state.selected?) {
            return None;
        }
        self.trade_consumption(trade)?;
        let expected = trade.output.as_ref()?;
        if trade.offer.output.block_descriptor.is_some() {
            let result = self.created_output_stack()?;
            return (result.network_id == expected.network_id
                && result.metadata == expected.metadata
                && result.count == expected.count
                && protocol::item_user_data_equal(&result.extra_data, &expected.extra_data)
                    == Some(true))
            .then_some(result);
        }
        Some(expected)
    }

    /// Iterates purchase reservations owned by one current window and offer.
    fn trade_requests(
        &self,
        generation: u64,
        network_id: u32,
    ) -> impl Iterator<Item = &super::queue::PendingRequest> {
        self.pending_requests().filter(move |request| {
            request.storage_generation == Some(generation)
                && request.trade_purchase == Some(network_id)
        })
    }

    /// Whether an unlocked offer has a use left after unanswered purchase reservations.
    pub fn trade_offer_available(&self, index: usize) -> bool {
        let Some(window) = self.storage.as_ref() else {
            return false;
        };
        let Some(state) = window.trade.as_ref() else {
            return false;
        };
        let Some(trade) = state.offers.get(index) else {
            return false;
        };
        let (pending, eligible) = self
            .trade_requests(window.generation, trade.offer.network_id)
            .fold((0_usize, 0_usize), |(pending, eligible), request| {
                (
                    pending + 1,
                    eligible
                        + usize::from(
                            request.state == super::InventoryPendingState::AwaitingResponse
                                && trade.stock.includes(request.request_id),
                        ),
                )
            });
        let used =
            i64::from(trade.offer.uses) + pending as i64 - trade.stock.credits.min(eligible) as i64;
        trade.offer.tier <= state.tier && used < i64::from(trade.offer.max_uses)
    }

    /// Commits an accepted purchase unless a server stock observation already included it.
    pub(super) fn accept_trade_purchase(&mut self, request: &super::queue::PendingRequest) {
        if !self.request_is_current(request) {
            return;
        }
        let Some(id) = request.trade_purchase else {
            return;
        };
        if let Some(state) = self
            .storage
            .as_mut()
            .and_then(|window| window.trade.as_mut())
            && let Some(trade) = Arc::make_mut(&mut state.offers)
                .iter_mut()
                .find(|offer| offer.offer.network_id == id)
            && !trade.stock.accept(request.request_id)
        {
            trade.offer.uses = trade.offer.uses.saturating_add(1);
        }
    }

    /// Whether an occupied input belongs to a complete payment for the selected offer.
    pub fn trade_input_valid(&self, index: usize) -> bool {
        let Some(state) = self.trading() else {
            return false;
        };
        let Some(trade) = state
            .selected
            .and_then(|selected| state.offers.get(selected))
        else {
            return false;
        };
        let Some(WindowKind::Trade { modern }) = self.window_kind() else {
            return false;
        };
        let slots = if modern {
            protocol::MODERN_TRADE_SLOTS
        } else {
            protocol::LEGACY_TRADE_SLOTS
        };
        slots
            .get(index)
            .and_then(|slot| self.view_stack(Cell::Craft(*slot)))
            .is_some()
            && self.trade_consumption(trade).is_some()
    }

    /// Allocates one recipe's payment, allowing either ingredient in either input cell.
    fn trade_consumption(&self, trade: &ResolvedTrade) -> Option<[u16; 2]> {
        let WindowKind::Trade { modern } = self.window_kind()? else {
            return None;
        };
        let slots = if modern {
            protocol::MODERN_TRADE_SLOTS
        } else {
            protocol::LEGACY_TRADE_SLOTS
        };
        let held = slots.map(|slot| self.view_stack(Cell::Craft(slot)));
        let first = trade.inputs[0].as_ref()?;
        if trade.offer.inputs[1].is_some() {
            let second = trade.inputs[1].as_ref()?;
            let pays = |stack: &NetworkItemStack, wanted: &NetworkItemStack| {
                trade_item_matches(stack, wanted) && stack.count >= wanted.count
            };
            // Either cell may hold either ingredient.
            if pays(held[0]?, first) && pays(held[1]?, second) {
                return Some([first.count, second.count]);
            }
            return (pays(held[0]?, second) && pays(held[1]?, first))
                .then_some([second.count, first.count]);
        }
        if let [Some(a), Some(b)] = held
            && !Self::same_item(a, b)
        {
            return None;
        }
        let mut needed = first.count;
        let mut consumed = [0; 2];
        for (index, stack) in held.iter().enumerate() {
            if let Some(stack) = stack {
                if !trade_item_matches(stack, first) {
                    return None;
                }
                consumed[index] = needed.min(stack.count);
                needed -= consumed[index];
            }
        }
        (needed == 0).then_some(consumed)
    }

    /// Purchases the selected result through the same stamped request lifecycle as other screens.
    pub fn begin_trade(&mut self) -> Result<i32, InventoryGestureError> {
        let output = self
            .trade_output()
            .cloned()
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let state = self.trading().expect("validated trader");
        let trade = &state.offers[state.selected.expect("validated selection")];
        let recipe_network_id = trade.offer.network_id;
        let craft = ScreenCraft::Trade {
            recipe_network_id,
            output,
            consumed: self
                .trade_consumption(trade)
                .ok_or(InventoryGestureError::InvalidRequest)?,
        };
        let id = self.begin_screen_output(&craft)?;
        self.queue
            .iter_mut()
            .find(|request| request.request_id == id)
            .expect("submitted purchase")
            .trade_purchase = Some(recipe_network_id);
        Ok(id)
    }
}

/// Compares the item identity and auxiliary value, including a recipe's wildcard.
fn trade_item_matches(stack: &NetworkItemStack, required: &NetworkItemStack) -> bool {
    stack.network_id == required.network_id
        && (required.metadata == u32::from(protocol::RECIPE_ANY_AUX)
            || stack.metadata == required.metadata)
}

impl TradeState {
    /// Finds the next unlocked offer in one direction without wrapping at an edge.
    pub fn adjacent_offer(&self, next: bool) -> Option<usize> {
        let current = self.selected?;
        let mut candidates = self.offers.iter().enumerate().filter(|(index, offer)| {
            (if next {
                *index > current
            } else {
                *index < current
            }) && offer.offer.tier <= self.tier
        });
        (if next {
            candidates.next()
        } else {
            candidates.next_back()
        })
        .map(|(index, _)| index)
    }

    /// Locates a row in the sorted tier groups used by selection and item previews.
    pub fn row_index(&self, tier_index: usize, row: usize) -> Option<usize> {
        let tier = *self.tiers().get(tier_index)?;
        self.offers
            .iter()
            .enumerate()
            .filter(|(_, offer)| offer.offer.tier == tier)
            .nth(row)
            .map(|(index, _)| index)
    }

    /// Borrows one complete offer preview, retaining all of its item tags.
    pub fn preview(&self, index: usize, column: u8) -> Option<&NetworkItemStack> {
        let offer = self.offers.get(index)?;
        match column {
            0 | 1 => offer.inputs[usize::from(column)].as_ref(),
            2 => offer.output.as_ref(),
            _ => None,
        }
    }
    /// Derives tier groups from the offers, without a separate fixed tier limit.
    pub fn tiers(&self) -> Vec<i32> {
        self.offers
            .iter()
            .map(|trade| trade.offer.tier)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}
