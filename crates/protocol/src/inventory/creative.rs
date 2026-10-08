//! The server's creative inventory catalog.

use std::sync::Arc;

use valentine::bedrock::version::v1_26_51::{
    CreativeContentPacket, EnumsSharedTypesCreativeItemCategory as Category,
};

use super::{InventoryEvent, InventoryPacketError, make_stack, validate_item_user_data};
use crate::item::NetworkItemStack;

/// Client policy bound on retained creative entries.
pub const MAX_CREATIVE_ITEMS: usize = 16_384;
/// Client policy bound on retained creative groups.
pub const MAX_CREATIVE_GROUPS: usize = 4_096;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CreativeCategory {
    Construction,
    Nature,
    Equipment,
    Items,
    CommandOnly,
    Unknown(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreativeGroup {
    pub category: CreativeCategory,
    pub name: Arc<str>,
    /// The item a collapsed named group shows; `None` when absent or malformed.
    pub icon: Option<NetworkItemStack>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreativeItem {
    pub creative_network_id: u32,
    /// The advertised stack; servers commonly advertise a count of one.
    pub stack: NetworkItemStack,
    pub group: u32,
}

/// One complete creative publication; it replaces any earlier one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreativeContentEvent {
    pub groups: Arc<[CreativeGroup]>,
    /// Sorted by creative network id.
    pub items: Arc<[CreativeItem]>,
    /// Well-formed entries that were not retained (empty or duplicate ids).
    pub skipped: usize,
}

impl CreativeContentEvent {
    #[must_use]
    pub fn item(&self, creative_network_id: u32) -> Option<&CreativeItem> {
        let index = self
            .items
            .binary_search_by_key(&creative_network_id, |item| item.creative_network_id)
            .ok()?;
        self.items.get(index)
    }
}

/// Normalizes a creative publication. Oversized catalogs are refused as a
/// whole; odd entries are skipped and counted.
pub fn normalize_creative_content(
    packet: CreativeContentPacket,
) -> Result<Option<InventoryEvent>, InventoryPacketError> {
    if packet.entries.len() > MAX_CREATIVE_ITEMS || packet.groups.len() > MAX_CREATIVE_GROUPS {
        return Ok(None);
    }
    let groups = packet
        .groups
        .into_iter()
        .map(|group| CreativeGroup {
            category: match group.creative_category {
                Category::Construction => CreativeCategory::Construction,
                Category::Nature => CreativeCategory::Nature,
                Category::Equipment => CreativeCategory::Equipment,
                Category::Items => CreativeCategory::Items,
                Category::Itemcommandonly => CreativeCategory::CommandOnly,
                Category::Unknown(code) => CreativeCategory::Unknown(code),
            },
            name: Arc::from(group.name),
            icon: make_stack(
                group.group_icon_item.id,
                i32::from_ne_bytes(group.group_icon_item.auxvalue.to_ne_bytes()),
                -1,
                group.group_icon_item.stacksize,
                group.group_icon_item.block_runtime_id,
                group.group_icon_item.user_data_buffer,
            )
            .ok(),
        })
        .collect();
    let mut skipped = 0;
    let mut items = Vec::with_capacity(packet.entries.len());
    for entry in packet.entries {
        let item = entry.item_instance;
        match validate_item_user_data(&item.user_data_buffer) {
            Err(InventoryPacketError::UnsupportedItemNbtVersion(_)) => {
                skipped += 1;
                continue;
            }
            result => result?,
        }
        let Ok(stack) = make_stack(
            item.id,
            i32::from_ne_bytes(item.auxvalue.to_ne_bytes()),
            -1,
            item.stacksize,
            item.block_runtime_id,
            item.user_data_buffer,
        ) else {
            skipped += 1;
            continue;
        };
        items.push(CreativeItem {
            creative_network_id: entry.creative_net_id.id,
            stack,
            group: entry.group_index,
        });
    }
    items.sort_by_key(|item| item.creative_network_id);
    let before = items.len();
    items.dedup_by_key(|item| item.creative_network_id);
    skipped += before - items.len();
    Ok(Some(InventoryEvent::Creative(CreativeContentEvent {
        groups,
        items: Arc::from(items),
        skipped,
    })))
}

#[cfg(test)]
mod tests {
    use valentine::bedrock::version::v1_26_51::{
        CerealizerNetworkItemInstanceDescriptorSerializedData, CreativeGroupInfoPayload,
        CreativeItemEntryPayload, TypedServerNetIdstructCreativeItemNetIdTag,
    };

    use super::*;

    fn entry(id: u32, network_id: i32) -> CreativeItemEntryPayload {
        CreativeItemEntryPayload {
            creative_net_id: TypedServerNetIdstructCreativeItemNetIdTag { id },
            item_instance: CerealizerNetworkItemInstanceDescriptorSerializedData {
                id: network_id,
                stacksize: 1,
                auxvalue: 0,
                block_runtime_id: 0,
                user_data_buffer: Vec::new(),
            },
            group_index: 0,
        }
    }

    /// Entries sort by id; empty items and duplicate ids are skipped, not fatal.
    #[test]
    fn creative_catalog_is_sorted_and_lenient_about_odd_entries() {
        let packet = CreativeContentPacket {
            groups: vec![CreativeGroupInfoPayload {
                creative_category: Category::Construction,
                name: "itemGroup.name.planks".into(),
                ..Default::default()
            }],
            entries: vec![entry(9, 5), entry(2, 1), entry(4, 0), entry(9, 6)],
        };
        let Some(InventoryEvent::Creative(catalog)) = normalize_creative_content(packet).unwrap()
        else {
            panic!("a creative publication");
        };
        assert_eq!(catalog.items.len(), 2);
        assert_eq!(catalog.skipped, 2);
        assert_eq!(catalog.item(9).unwrap().stack.network_id, 5);
        assert_eq!(catalog.item(2).unwrap().stack.count, 1);
        assert!(catalog.item(4).is_none());
        assert_eq!(catalog.groups[0].category, CreativeCategory::Construction);
    }

    #[test]
    fn review_unsupported_item_nbt_skips_only_its_creative_entry() {
        let mut unsupported = entry(2, 1);
        unsupported.item_instance.user_data_buffer = vec![255, 255, 2];
        let packet = CreativeContentPacket {
            groups: Vec::new(),
            entries: vec![entry(1, 1), unsupported],
        };
        let Some(InventoryEvent::Creative(catalog)) = normalize_creative_content(packet).unwrap()
        else {
            panic!("catalog")
        };
        assert_eq!(catalog.items.len(), 1);
        assert_eq!(catalog.skipped, 1);
        let mut truncated = entry(2, 1);
        truncated.item_instance.user_data_buffer = vec![255, 255];
        assert!(
            normalize_creative_content(CreativeContentPacket {
                groups: Vec::new(),
                entries: vec![truncated]
            })
            .is_err()
        );
    }

    #[test]
    fn oversized_creative_catalogs_are_refused_whole() {
        let packet = CreativeContentPacket {
            groups: Vec::new(),
            entries: (0..=MAX_CREATIVE_ITEMS as u32)
                .map(|id| entry(id, 1))
                .collect(),
        };
        assert_eq!(normalize_creative_content(packet).unwrap(), None);
    }
}
