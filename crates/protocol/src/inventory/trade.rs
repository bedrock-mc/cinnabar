//! Server-owned offers that initiate a villager's trading window.

use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::UpdateTrade;

use super::ContainerIdentity;

mod nbt;
use nbt::Compound;

/// A named item in a trade, before the session registry resolves its numeric identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeItem {
    pub identifier: Arc<str>,
    pub metadata: u32,
    pub count: u16,
    /// Saved block-state compound payload, retained until an authoritative result resolves it.
    pub block_descriptor: Option<Arc<[u8]>>,
    /// Complete item user data, including arbitrary server-supplied tags.
    pub extra_data: Arc<[u8]>,
}

/// One server recipe, in the order it appears in the trader's offer list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeOffer {
    pub network_id: u32,
    pub inputs: [Option<TradeItem>; 2],
    pub output: TradeItem,
    pub tier: i32,
    pub uses: i32,
    pub max_uses: i32,
}

/// Opens or refreshes a trader's window independently of ContainerOpen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeEvent {
    pub container: ContainerIdentity,
    pub actor_unique_id: i64,
    pub title: Arc<str>,
    pub modern: bool,
    pub tier: i32,
    pub offers: Arc<[TradeOffer]>,
}

/// Retains usable offers and skips unfamiliar entries without dropping the screen.
pub(crate) fn normalize_trade(packet: &UpdateTrade) -> Option<TradeEvent> {
    if packet.type_.0 != protocol_ids::WindowType::Trade.wire_value() as u8 {
        return None;
    }
    let root = Compound::root(&packet.data.0)?;
    let offers = root
        .field("Recipes")
        .and_then(|value| value.compounds())
        .unwrap_or_default();
    Some(TradeEvent {
        container: ContainerIdentity::window(super::raw_window_id(packet.container_id.0).ok()?),
        actor_unique_id: packet.entity_unique_id.0,
        title: Arc::from(packet.display_name.to_str().ok()?),
        modern: packet.use_new_trade_screen,
        tier: packet.trader_tier.0,
        offers: offers.iter().filter_map(offer).collect(),
    })
}

/// Reads one complete offer; missing required items leave just that offer unavailable.
fn offer(value: &Compound<'_>) -> Option<TradeOffer> {
    let first = item(value, "buyA")??;
    Some(TradeOffer {
        network_id: value
            .field("netId")
            .and_then(|value| value.int32())
            .unwrap_or_default() as u32,
        inputs: [Some(first), item(value, "buyB")?],
        output: item(value, "sell")??,
        tier: value.number("tier", 0.0) as i32,
        uses: value.number("uses", 0.0) as i32,
        max_uses: value.number("maxUses", i32::MAX as f64) as i32,
    })
}

/// Preserves saved item data; outer None is malformed, inner None is absent or empty.
fn item(offer: &Compound<'_>, name: &str) -> Option<Option<TradeItem>> {
    let Some(value) = offer.field(name) else {
        return Some(None);
    };
    let value = value.compound()?;
    let count = value.field("Count")?.count()?;
    if count == 0 {
        return Some(None);
    }
    Some(Some(TradeItem {
        identifier: Arc::from(value.field("Name")?.string()?),
        metadata: u32::from(value.number("Damage", 0.0) as i16 as u16),
        count,
        block_descriptor: match value.field("Block") {
            Some(block) => Some(block.compound_bytes()?.into()),
            None => None,
        },
        extra_data: value.item_data()?.into(),
    }))
}
