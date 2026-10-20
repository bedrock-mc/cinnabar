//! Trading layouts and the server-owned offer collections they bind.

use crate::container_actions::Widget;
use crate::container_kinds::{Cell, ContainerKind};
use crate::container_source::{ContainerFrame, ContainerSource};
use inventory::inventory_ledger::TradeState;
use json_ui::vanilla::{Binding as B, Button, Collection as C, Reference as R, TextKey};
use json_ui::{CollectionItem, DataSource, HitRegion, Scalar};
use ui::IconRef;

/// Supplies validity for the actual input and output slot collections of either layout.
pub(crate) fn slot_valid(
    ledger: &inventory::PlayerInventoryLedger,
    collection: &str,
) -> Option<bool> {
    ledger.trading()?;
    if [
        C::Trade2Ingredient1Item.as_str(),
        C::TradeIngredient1Items.as_str(),
    ]
    .contains(&collection)
    {
        Some(ledger.trade_input_valid(0))
    } else if [
        C::Trade2Ingredient2Item.as_str(),
        C::TradeIngredient2Items.as_str(),
    ]
    .contains(&collection)
    {
        Some(ledger.trade_input_valid(1))
    } else if [C::Trade2ResultItem.as_str(), C::TradeResultItem.as_str()].contains(&collection) {
        Some(ledger.trade_output().is_some())
    } else {
        None
    }
}

/// Chooses the screen and its exact UI-inventory inputs from the server's screen flag.
pub fn kind(modern: bool) -> &'static ContainerKind {
    const MODERN: ContainerKind = ContainerKind {
        screen: R::Trade2TradeScreen.as_str(),
        title_key: TextKey::TradeTrade.as_str(),
        flags: &[],
        collections: &[
            (
                C::Trade2Ingredient1Item.as_str(),
                &[Cell::Ui(protocol::MODERN_TRADE_SLOTS[0])],
            ),
            (
                C::Trade2Ingredient2Item.as_str(),
                &[Cell::Ui(protocol::MODERN_TRADE_SLOTS[1])],
            ),
            (C::Trade2ResultItem.as_str(), &[Cell::Output]),
        ],
    };
    const LEGACY: ContainerKind = ContainerKind {
        screen: R::TradeTradeScreen.as_str(),
        title_key: TextKey::TradeTrade.as_str(),
        flags: &[],
        collections: &[
            (
                C::TradeIngredient1Items.as_str(),
                &[Cell::Ui(protocol::LEGACY_TRADE_SLOTS[0])],
            ),
            (
                C::TradeIngredient2Items.as_str(),
                &[Cell::Ui(protocol::LEGACY_TRADE_SLOTS[1])],
            ),
            (C::TradeResultItem.as_str(), &[Cell::Output]),
        ],
    };
    if modern { &MODERN } else { &LEGACY }
}

/// Identifies an offer item renderer independently of the inventory icon table.
fn item_key(index: usize) -> i64 {
    -10_000 - index as i64
}

/// Supplies the offer icons in the same input/input/result order as the ledger projection.
pub(crate) fn icons(frame: &ContainerFrame<'_>) -> Vec<(i64, IconRef)> {
    frame
        .recipe_icons
        .iter()
        .enumerate()
        .filter_map(|(index, icon)| Some((item_key(index), (*icon)?)))
        .collect()
}

/// Makes one offer preview available to both collection and controller bindings.
fn item(trade: &TradeState, offer: usize, column: usize) -> CollectionItem {
    let resolved = &trade.offers[offer];
    let stack = if column == 2 {
        resolved.output.as_ref()
    } else {
        resolved.inputs[column].as_ref()
    };
    let count = stack.map_or(0, |stack| stack.count);
    [
        (B::ItemIdAux, Scalar::Int(item_key(offer * 3 + column))),
        (
            B::ItemStackCount,
            Scalar::Text(if count > 1 {
                count.to_string()
            } else {
                String::new()
            }),
        ),
        (B::TradeItemCount, Scalar::Text(count.to_string())),
        (B::ItemValid, Scalar::Bool(stack.is_some())),
        (
            B::HoverText,
            Scalar::Text(if column == 2 {
                resolved.offer.output.identifier.to_string()
            } else {
                resolved.offer.inputs[column]
                    .as_ref()
                    .map_or_else(String::new, |item| item.identifier.to_string())
            }),
        ),
    ]
    .into_iter()
    .fold(CollectionItem::default(), |item, (key, value)| {
        item.with(key.as_str(), value.clone())
            .with_controller_value(key.as_str(), value)
    })
}

/// Populates tier groups, recipe previews and selection callbacks for both trading screens.
pub(crate) fn data(source: &dyn ContainerSource, data: &mut DataSource) {
    let ledger = source.player().inventory.ledger();
    let Some(trade) = ledger.trading() else {
        return;
    };
    let tiers = trade.tiers();
    let can_trade = ledger.trade_output().is_some();
    for (key, value) in [
        (B::TradeSelectorTotal, Scalar::Int(tiers.len() as i64)),
        (
            B::NameLabel,
            Scalar::Text(
                source
                    .translation(&trade.title)
                    .map_or_else(|| trade.title.to_string(), |title| title.to_string()),
            ),
        ),
        (B::ShowLevel, Scalar::Bool(tiers.len() > 1)),
        (B::ExpBarVisible, Scalar::Bool(false)),
        (B::TradeButtonEnabled, Scalar::Bool(can_trade)),
        (B::CrossOutIcon, Scalar::Bool(!can_trade)),
        (
            B::ShowSecondTradeSlot,
            Scalar::Bool(
                trade
                    .selected
                    .is_some_and(|index| trade.offers[index].offer.inputs[1].is_some()),
            ),
        ),
        (
            B::LeftButtonEnabled,
            Scalar::Bool(trade.selected.is_some_and(|index| index > 0)),
        ),
        (
            B::RightButtonEnabled,
            Scalar::Bool(trade.selected.is_some_and(|index| {
                trade
                    .offers
                    .get(index + 1)
                    .is_some_and(|offer| offer.offer.tier <= trade.tier)
            })),
        ),
    ] {
        data.set_global(key.as_str(), value);
    }
    let mut headers = Vec::new();
    for (tier_index, tier) in tiers.iter().enumerate() {
        let level_key = TextKey::trade_level(*tier);
        headers.push(
            CollectionItem::default()
                .with(
                    B::TierName.as_str(),
                    Scalar::Text(
                        source
                            .translation(&level_key)
                            .map_or(level_key, |name| name.to_string()),
                    ),
                )
                .with(B::TierVisible.as_str(), Scalar::Bool(true))
                .with(
                    B::IsTierUnlocked.as_str(),
                    Scalar::Bool(*tier <= trade.tier),
                ),
        );
        let indices: Vec<_> = trade
            .offers
            .iter()
            .enumerate()
            .filter_map(|(index, offer)| (offer.offer.tier == *tier).then_some(index))
            .collect();
        let rows: Vec<_> = indices
            .iter()
            .map(|index| {
                let offer = &trade.offers[*index];
                let unlocked = offer.offer.tier <= trade.tier;
                let exhausted = offer.offer.uses >= offer.offer.max_uses;
                [
                    (B::TradeToggleState, trade.selected == Some(*index)),
                    (B::TradeToggleEnabled, unlocked),
                    (B::TradePossible, unlocked && !exhausted),
                    (B::TradeCrossOutVisible, exhausted),
                    (B::PaddingAroundSellItem, offer.offer.inputs[1].is_some()),
                ]
                .into_iter()
                .fold(
                    CollectionItem::default().with(
                        B::TradeTierTotal.as_str(),
                        Scalar::Int(indices.len() as i64),
                    ),
                    |row, (key, value)| {
                        row.with_controller_value(key.as_str(), Scalar::Bool(value))
                    },
                )
            })
            .collect();
        data.set_scoped_collection(C::TradeTiers.as_str(), tier_index, C::Trades.as_str(), rows);
        let parent = json_ui::scoped_key(C::TradeTiers.as_str(), tier_index, C::Trades.as_str());
        for (row, index) in indices.iter().enumerate() {
            for (column, collection) in [C::TradeItem1, C::TradeItem2, C::SellItem]
                .iter()
                .enumerate()
            {
                data.set_scoped_collection(
                    &parent,
                    row,
                    collection.as_str(),
                    vec![item(trade, *index, column)],
                );
            }
        }
    }
    data.set_collection(C::TradeTiers.as_str(), headers);
    if let Some(index) = trade.selected {
        for (column, collection) in [C::TradeItemA, C::TradeItemB, C::SellItem]
            .iter()
            .enumerate()
        {
            data.set_collection(collection.as_str(), vec![item(trade, index, column)]);
        }
    }
}

/// Resolves a nested offer row or classic-screen cycle button without flattening tier indices.
pub(crate) fn hit(screen: &str, region: &HitRegion) -> Option<Widget> {
    if ![R::TradeTradeScreen.as_str(), R::Trade2TradeScreen.as_str()].contains(&screen) {
        return None;
    }
    match region.pressed.as_deref() {
        Some(button) if button == Button::CycleRecipeLeft.as_str() => {
            return Some(Widget::TradeCycle { next: false });
        }
        Some(button) if button == Button::CycleRecipeRight.as_str() => {
            return Some(Widget::TradeCycle { next: true });
        }
        Some(button) if button == Button::Trade.as_str() => return Some(Widget::Trade),
        _ => {}
    }
    let tier = region
        .collections
        .iter()
        .find(|(name, _)| name == C::TradeTiers.as_str())?
        .1;
    let row = region
        .collections
        .iter()
        .find(|(name, _)| name == C::Trades.as_str())?
        .1;
    Some(Widget::TradeOffer { tier, row })
}
