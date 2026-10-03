use std::collections::BTreeMap;

use protocol::{ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, NetworkItemStack};
use sha2::{Digest, Sha256};

use super::{Cell, PlayerInventoryLedger};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OccupiedStackRelation {
    Compatible { capacity: u16 },
    Incompatible,
    Unsupported,
}

impl PlayerInventoryLedger {
    pub fn negotiated_item_entry(&self, network_id: i32) -> Option<&ItemRegistryEntry> {
        self.item_registry.as_ref()?.get(&network_id)
    }

    pub fn apply_registry(&mut self, event: &ItemRegistryEvent) {
        let Some(next) = registry_map(event) else {
            return;
        };
        let Some(previous) = self.item_registry.as_ref() else {
            self.item_registry = Some(std::sync::Arc::new(next));
            return;
        };
        if previous.as_ref() == &next {
            return;
        }

        let affected_cells = self.registry_affected_cells(previous, &next);
        let affected_requests: Vec<i32> = self
            .queue
            .iter()
            .filter(|pending| {
                pending
                    .predicted
                    .iter()
                    .filter_map(|prediction| prediction.held.as_ref())
                    .map(|held| held.stack.network_id)
                    .any(|network_id| {
                        registry_identity_changed(previous, &next, network_id)
                            || (pending.registry_bound_merge
                                && registry_merge_rule_changed(previous, &next, network_id))
                    })
            })
            .map(|pending| pending.request_id)
            .collect();
        self.abandon_requests(|pending| affected_requests.contains(&pending.request_id));
        for cell in affected_cells {
            self.mark_cell_recovery(cell);
        }
        self.item_registry = Some(std::sync::Arc::new(next));
        self.refold();
    }

    pub(super) fn occupied_stack_relation(
        &self,
        source: &NetworkItemStack,
        destination: &NetworkItemStack,
    ) -> OccupiedStackRelation {
        let distinct_network_ids = source.network_id != destination.network_id;
        // Native ItemStackBase::matchesItem compares block/aux identity; a
        // nonzero block identity is an ordinary block stack, not an unknown
        // stack shape. Count and sparse/server stack ids are not item identity.
        if source.metadata != destination.metadata
            || source.block_runtime_id != destination.block_runtime_id
        {
            return OccupiedStackRelation::Incompatible;
        }
        if !plain_stack(source) || !plain_stack(destination) {
            return if distinct_network_ids {
                OccupiedStackRelation::Incompatible
            } else {
                OccupiedStackRelation::Unsupported
            };
        }
        if (source.stack_network_id <= 0
            && !self
                .queue
                .iter()
                .any(|request| request.request_id == source.stack_network_id))
            || (destination.stack_network_id <= 0
                && !self
                    .queue
                    .iter()
                    .any(|request| request.request_id == destination.stack_network_id))
            || (source.stack_network_id > 0
                && source.stack_network_id == destination.stack_network_id)
        {
            return OccupiedStackRelation::Unsupported;
        }
        let Some(registry) = self.item_registry.as_ref() else {
            return if distinct_network_ids {
                OccupiedStackRelation::Incompatible
            } else {
                OccupiedStackRelation::Unsupported
            };
        };
        let Some(source_entry) = registry.get(&source.network_id) else {
            return if distinct_network_ids {
                OccupiedStackRelation::Incompatible
            } else {
                OccupiedStackRelation::Unsupported
            };
        };
        let Some(destination_entry) = registry.get(&destination.network_id) else {
            return if distinct_network_ids {
                OccupiedStackRelation::Incompatible
            } else {
                OccupiedStackRelation::Unsupported
            };
        };
        if source_entry.identifier != destination_entry.identifier {
            return OccupiedStackRelation::Incompatible;
        }
        let Some(source_capacity) = entry_capacity(source_entry) else {
            return OccupiedStackRelation::Unsupported;
        };
        let Some(destination_capacity) = entry_capacity(destination_entry) else {
            return OccupiedStackRelation::Unsupported;
        };
        if source_capacity != destination_capacity {
            return OccupiedStackRelation::Unsupported;
        }
        OccupiedStackRelation::Compatible {
            capacity: u16::from(destination_capacity),
        }
    }

    fn registry_affected_cells(
        &self,
        previous: &BTreeMap<i32, ItemRegistryEntry>,
        next: &BTreeMap<i32, ItemRegistryEntry>,
    ) -> Vec<Cell> {
        self.confirmed
            .occupied()
            .filter(|(_, held)| registry_identity_changed(previous, next, held.stack.network_id))
            .map(|(cell, _)| cell)
            .collect()
    }
}

fn registry_map(event: &ItemRegistryEvent) -> Option<BTreeMap<i32, ItemRegistryEntry>> {
    if event.entries.len() > protocol::MAX_ITEM_REGISTRY_ENTRIES {
        return None;
    }
    let mut entries = BTreeMap::new();
    for entry in event.entries.iter() {
        if entries.insert(entry.network_id, entry.clone()).is_some() {
            return None;
        }
    }
    Some(entries)
}

fn registry_identity_changed(
    previous: &BTreeMap<i32, ItemRegistryEntry>,
    next: &BTreeMap<i32, ItemRegistryEntry>,
    network_id: i32,
) -> bool {
    match (previous.get(&network_id), next.get(&network_id)) {
        (Some(previous), Some(next)) => previous.identifier != next.identifier,
        (Some(_), None) => true,
        (None, Some(_)) | (None, None) => false,
    }
}

fn registry_merge_rule_changed(
    previous: &BTreeMap<i32, ItemRegistryEntry>,
    next: &BTreeMap<i32, ItemRegistryEntry>,
    network_id: i32,
) -> bool {
    effective_merge_binding(previous.get(&network_id))
        != effective_merge_binding(next.get(&network_id))
}

fn effective_merge_binding(entry: Option<&ItemRegistryEntry>) -> Option<(&str, Option<u8>)> {
    let entry = entry?;
    Some((entry.identifier.as_ref(), entry_capacity(entry)))
}

pub(super) fn entry_capacity(entry: &ItemRegistryEntry) -> Option<u8> {
    let vanilla = protocol::vanilla_item_capacity(&entry.identifier, 0);
    if matches!(entry.version, ItemRegistryVersion::Unknown(_)) {
        return None;
    }
    // A component item's declared stack size binds it as it binds the vanilla client.
    if let Some(capacity) = entry.negotiated_max_stack_size
        && (vanilla.is_some() || entry.component_based)
    {
        return Some(capacity);
    }
    // An item vanilla does not know that the server defines no item of its own is a server
    // block's BlockItem (Dragonfly sends it without components or an item version): the
    // block registers it and it keeps Item's default stack size.
    if vanilla.is_none()
        && !entry.component_based
        && matches!(entry.version, ItemRegistryVersion::None)
    {
        return Some(protocol::ITEM_DEFAULT_MAX_STACK_SIZE);
    }
    vanilla.filter(|_| !entry.component_based && entry.canonical_empty_component_data)
}

pub(super) fn plain_stack(stack: &NetworkItemStack) -> bool {
    let digest: [u8; 32] = Sha256::digest(&stack.extra_data).into();
    // Plainness describes user data, not aux or block identity. Recipes match
    // aux independently and vanilla accepts block ingredients with runtime ids.
    (stack.extra_data.is_empty() || stack.extra_data.as_ref() == [0; 10])
        && stack.nbt_digest == digest
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{ItemRegistryEntry, ItemRegistryVersion};

    use super::entry_capacity;

    fn entry(identifier: &str, component_based: bool, declared: Option<u8>) -> ItemRegistryEntry {
        ItemRegistryEntry {
            identifier: identifier.into(),
            network_id: 900,
            component_based,
            version: ItemRegistryVersion::DataDriven,
            component_digest: [0; 32],
            negotiated_max_stack_size: declared,
            canonical_empty_component_data: declared.is_none(),
            item_tags: Arc::from([]),
        }
    }

    // A custom item binds merges only through its own declared stack size.
    #[test]
    fn component_items_use_their_declared_stack_size() {
        assert_eq!(entry_capacity(&entry("zeqa:gem", true, Some(16))), Some(16));
        assert_eq!(entry_capacity(&entry("zeqa:gem", true, None)), None);
        assert_eq!(entry_capacity(&entry("zeqa:gem", false, Some(16))), None);
        assert_eq!(
            entry_capacity(&entry("minecraft:stick", false, None)),
            Some(64)
        );
    }

    // A server block's own item is the BlockItem its block registers: not component based, no
    // item version, and it keeps Item's default stack size whatever data rides along.
    #[test]
    fn server_block_items_stack_to_the_item_default() {
        let block_item = |declared| ItemRegistryEntry {
            version: ItemRegistryVersion::None,
            ..entry("benergistics:controller", false, declared)
        };
        let default = Some(protocol::ITEM_DEFAULT_MAX_STACK_SIZE);
        assert_eq!(entry_capacity(&block_item(Some(16))), default);
        assert_eq!(entry_capacity(&block_item(None)), default);
    }
}
