//! The server's creative inventory catalog.

use std::{collections::HashMap, sync::Arc};

use bedrock_protocol_1_26_51::enums::CreativeItemCategory as Category;
use bedrock_protocol_1_26_51::packets::CreativeContent;

use super::{InventoryEvent, InventoryPacketError, make_stack, validate_item_user_data};
use crate::item::NetworkItemStack;

/// Client policy bound on retained creative entries.
pub const MAX_CREATIVE_ITEMS: usize = 16_384;
/// Client policy bound on retained creative groups.
pub const MAX_CREATIVE_GROUPS: usize = 4_096;

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub enum CreativeCategory {
    Construction,
    Nature,
    Equipment,
    Items,
    CommandOnly,
    Unknown(u8),
}

impl CreativeCategory {
    /// Converts the legacy `item_properties.creative_category` value to the
    /// category used by the CreativeContent packet.
    #[must_use]
    pub const fn from_legacy_value(value: u8) -> Self {
        match value {
            1 => Self::Construction,
            2 => Self::Nature,
            3 => Self::Equipment,
            4 => Self::Items,
            5 => Self::CommandOnly,
            value => Self::Unknown(value),
        }
    }
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
    packet: CreativeContent,
) -> Result<Option<InventoryEvent>, InventoryPacketError> {
    if packet.entries.len() > MAX_CREATIVE_ITEMS || packet.groups.len() > MAX_CREATIVE_GROUPS {
        return Ok(None);
    }
    let mut named_groups = HashMap::with_capacity(packet.groups.len());
    let mut group_indices = Vec::with_capacity(packet.groups.len());
    let mut groups = Vec::with_capacity(packet.groups.len());
    for group in packet.groups {
        let group = CreativeGroup {
            category: match group.creative_category {
                Category::Construction => CreativeCategory::Construction,
                Category::Nature => CreativeCategory::Nature,
                Category::Equipment => CreativeCategory::Equipment,
                Category::Items => CreativeCategory::Items,
                Category::Itemcommandonly => CreativeCategory::CommandOnly,
                Category::Unknown(code) => CreativeCategory::Unknown(code),
            },
            name: Arc::from(group.name.to_string_lossy()),
            icon: make_stack(
                group.group_icon_item.id.0,
                i32::from_ne_bytes(group.group_icon_item.aux_value.0.to_ne_bytes()),
                -1,
                group.group_icon_item.stack_size.0,
                group.group_icon_item.block_runtime_id.0,
                group.group_icon_item.user_data_buffer.to_vec(),
            )
            .ok(),
        };
        let index = if group.name.is_empty() {
            // Anonymous groups delimit catalog order and may intentionally recur.
            groups.push(group);
            groups.len() - 1
        } else if let Some(index) = named_groups.get(&(group.category, group.name.clone())) {
            *index
        } else {
            let index = groups.len();
            named_groups.insert((group.category, group.name.clone()), index);
            groups.push(group);
            index
        };
        group_indices.push(u32::try_from(index).expect("creative group index fits in u32"));
    }
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
            item.id.0,
            i32::from_ne_bytes(item.aux_value.0.to_ne_bytes()),
            -1,
            item.stack_size.0,
            item.block_runtime_id.0,
            item.user_data_buffer.to_vec(),
        ) else {
            skipped += 1;
            continue;
        };
        let group = usize::try_from(entry.group_index.0)
            .ok()
            .and_then(|index| group_indices.get(index).copied())
            .unwrap_or(entry.group_index.0);
        items.push(CreativeItem {
            creative_network_id: entry.creative_net_id.0,
            stack,
            group,
        });
    }
    items.sort_by_key(|item| item.creative_network_id);
    let before = items.len();
    items.dedup_by_key(|item| item.creative_network_id);
    skipped += before - items.len();
    Ok(Some(InventoryEvent::Creative(CreativeContentEvent {
        groups: groups.into(),
        items: Arc::from(items),
        skipped,
    })))
}

#[cfg(test)]
mod tests {
    use bedrock_protocol_1_26_51::types::{
        CreativeGroupInfo, CreativeItemEntry, CreativeItemNetID,
        NetworkItemInstanceDescriptorSerializedData,
    };
    use bedrock_protocol_1_26_51::wire;

    use super::*;

    fn entry_in_group(id: u32, network_id: i32, group: u32) -> CreativeItemEntry {
        CreativeItemEntry {
            creative_net_id: CreativeItemNetID(id),
            item_instance: NetworkItemInstanceDescriptorSerializedData {
                id: wire::ZigZag32(network_id),
                stack_size: wire::U16LE(1),
                aux_value: wire::VarUInt(0),
                block_runtime_id: wire::ZigZag32(0),
                user_data_buffer: bytes::Bytes::new(),
            },
            group_index: wire::VarUInt(group),
        }
    }

    fn entry(id: u32, network_id: i32) -> CreativeItemEntry {
        entry_in_group(id, network_id, 0)
    }

    /// Entries sort by id; empty items and duplicate ids are skipped, not fatal.
    #[test]
    fn creative_catalog_is_sorted_and_lenient_about_odd_entries() {
        let packet = CreativeContent {
            groups: vec![CreativeGroupInfo {
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
        unsupported.item_instance.user_data_buffer = vec![255, 255, 2].into();
        let packet = CreativeContent {
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
        truncated.item_instance.user_data_buffer = vec![255, 255].into();
        assert!(
            normalize_creative_content(CreativeContent {
                groups: Vec::new(),
                entries: vec![truncated]
            })
            .is_err()
        );
    }

    #[test]
    fn oversized_creative_catalogs_are_refused_whole() {
        let packet = CreativeContent {
            groups: Vec::new(),
            entries: (0..=MAX_CREATIVE_ITEMS as u32)
                .map(|id| entry(id, 1))
                .collect(),
        };
        assert_eq!(normalize_creative_content(packet).unwrap(), None);
    }

    #[test]
    fn duplicate_named_groups_share_one_index_and_preserve_anonymous_boundaries() {
        let group = |category: Category, name: &str| CreativeGroupInfo {
            creative_category: category,
            name: name.into(),
            ..Default::default()
        };
        let packet = CreativeContent {
            groups: vec![
                group(Category::Construction, "itemGroup.name.stairs"),
                group(Category::Construction, "itemGroup.name.fence"),
                group(Category::Construction, "itemGroup.name.stairs"),
                group(Category::Nature, "itemGroup.name.stairs"),
                group(Category::Construction, ""),
                group(Category::Construction, ""),
            ],
            entries: vec![
                entry_in_group(1, 1, 0),
                entry_in_group(2, 2, 2),
                entry_in_group(3, 3, 1),
                entry_in_group(4, 4, 3),
                entry_in_group(5, 5, 4),
                entry_in_group(6, 6, 5),
            ],
        };
        let Some(InventoryEvent::Creative(catalog)) = normalize_creative_content(packet).unwrap()
        else {
            panic!("creative catalog")
        };

        assert_eq!(catalog.groups.len(), 5);
        assert_eq!(catalog.groups[0].name.as_ref(), "itemGroup.name.stairs");
        assert_eq!(catalog.groups[1].name.as_ref(), "itemGroup.name.fence");
        assert_eq!(catalog.groups[2].category, CreativeCategory::Nature);
        assert!(catalog.groups[3].name.is_empty());
        assert!(catalog.groups[4].name.is_empty());
        assert_eq!(catalog.item(1).unwrap().group, 0);
        assert_eq!(catalog.item(2).unwrap().group, 0);
        assert_eq!(catalog.item(3).unwrap().group, 1);
        assert_eq!(catalog.item(4).unwrap().group, 2);
        assert_eq!(catalog.item(5).unwrap().group, 3);
        assert_eq!(catalog.item(6).unwrap().group, 4);
    }
}
