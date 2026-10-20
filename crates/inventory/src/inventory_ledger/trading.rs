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
}

/// The server-owned trading window and the player's selected offer.
#[derive(Debug, Clone)]
pub struct TradeState {
    pub title: Arc<str>,
    pub tier: i32,
    pub offers: Arc<[ResolvedTrade]>,
    pub selected: Option<usize>,
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
        let tier = *trade
            .tiers()
            .get(tier_index)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let index = trade
            .offers
            .iter()
            .enumerate()
            .filter(|(_, offer)| offer.offer.tier == tier)
            .nth(row)
            .map(|(index, _)| index)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        self.select_trade(index)
    }

    /// Moves the classic screen's selector to the adjacent unlocked offer.
    pub fn cycle_trade(&mut self, next: bool) -> Result<(), InventoryGestureError> {
        let trade = self
            .trading()
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let current = trade
            .selected
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let index = if next {
            current.checked_add(1)
        } else {
            current.checked_sub(1)
        }
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
        Some(NetworkItemStack {
            network_id: entry.network_id,
            metadata: item.metadata,
            count: item.count,
            extra_data: Arc::clone(&item.extra_data),
            nbt_digest: Sha256::digest(&item.extra_data).into(),
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
        }
    }

    /// Rebuilds cached offer identities when a server replaces its item registry.
    pub(super) fn refresh_trade_items(&mut self) {
        let Some(offers) = self.trading().map(|trade| Arc::clone(&trade.offers)) else {
            return;
        };
        let resolved = offers
            .iter()
            .map(|trade| self.resolve_trade(&trade.offer))
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
            !window.closing
                && Some(window.window_id) == event.container.window_id
                && window.kind == kind
                && window.actor_unique_id == event.actor_unique_id
        });
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
        let offers: Arc<[ResolvedTrade]> = event
            .offers
            .iter()
            .map(|offer| self.resolve_trade(offer))
            .collect();
        let selected = selected_id
            .and_then(|id| offers.iter().position(|trade| trade.offer.network_id == id))
            .or_else(|| {
                offers
                    .iter()
                    .position(|trade| trade.offer.tier <= event.tier)
            });
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
        if trade.offer.tier > state.tier || trade.offer.uses >= trade.offer.max_uses {
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

    /// Whether one input has the selected offer's matching item and required count.
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
        let Some(stack) = slots
            .get(index)
            .and_then(|slot| self.view_stack(Cell::Craft(*slot)))
        else {
            return false;
        };
        trade
            .inputs
            .get(index)
            .and_then(Option::as_ref)
            .or_else(|| {
                trade.inputs[0]
                    .as_ref()
                    .filter(|_| trade.offer.inputs[1].is_none())
            })
            .is_some_and(|required| {
                trade_item_matches(stack, required) && stack.count >= required.count
            })
    }

    /// Allocates one recipe's payment, allowing a single ingredient in either input cell.
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
            return (trade_item_matches(held[0]?, first)
                && held[0]?.count >= first.count
                && trade_item_matches(held[1]?, second)
                && held[1]?.count >= second.count)
                .then_some([first.count, second.count]);
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
        let craft = ScreenCraft::Trade {
            recipe_network_id: trade.offer.network_id,
            output,
            consumed: self
                .trade_consumption(trade)
                .ok_or(InventoryGestureError::InvalidRequest)?,
        };
        self.begin_screen_output(&craft)
    }
}

/// Compares the item identity and auxiliary value, including a recipe's wildcard.
fn trade_item_matches(stack: &NetworkItemStack, required: &NetworkItemStack) -> bool {
    stack.network_id == required.network_id
        && (required.metadata == u32::from(protocol::RECIPE_ANY_AUX)
            || stack.metadata == required.metadata)
}

impl TradeState {
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
