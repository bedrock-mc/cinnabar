use std::collections::BTreeMap;

use protocol::{ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, NetworkItemStack};
use sha2::{Digest, Sha256};

use super::cells::Held;
use super::equipment::equipment_admission;
use super::{Cell, PlayerInventoryLedger};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OccupiedStackRelation {
    Compatible { capacity: u16 },
    Incompatible,
    Unsupported,
}

impl PlayerInventoryLedger {
    /// Item identity excludes count and server stack IDs, retaining all item user data.
    pub(crate) fn same_item(a: &NetworkItemStack, b: &NetworkItemStack) -> bool {
        a.network_id == b.network_id
            && a.metadata == b.metadata
            && a.block_runtime_id == b.block_runtime_id
            && protocol::item_user_data_equal(&a.extra_data, &b.extra_data) == Some(true)
    }

    /// Retained registry identity for consumers caching identifier-dependent data.
    pub fn item_registry_snapshot(
        &self,
    ) -> Option<&std::sync::Arc<BTreeMap<i32, ItemRegistryEntry>>> {
        self.item_registry.as_ref()
    }

    pub fn negotiated_item_entry(&self, network_id: i32) -> Option<&ItemRegistryEntry> {
        self.item_registry.as_ref()?.get(&network_id)
    }

    /// The negotiated item registry by network id; a resend that changes it replaces the `Arc`.
    pub fn negotiated_item_registry(
        &self,
    ) -> Option<&std::sync::Arc<BTreeMap<i32, ItemRegistryEntry>>> {
        self.item_registry.as_ref()
    }

    /// Effective facts from the current negotiated item registry.
    pub fn item_facts(&self) -> &crate::SessionItemFacts {
        &self.item_facts
    }

    pub fn apply_registry(&mut self, event: &ItemRegistryEvent) {
        let Some(next) = registry_map(event) else {
            return;
        };
        if self
            .item_registry
            .as_ref()
            .is_some_and(|previous| previous.as_ref() == &next)
        {
            return;
        }
        let Ok(pinned) = assets::facts::ItemFacts::builtin() else {
            return;
        };
        let Some(facts) = crate::SessionItemFacts::from_registry(pinned, event) else {
            return;
        };
        let facts = std::sync::Arc::new(facts);
        let Some(previous) = self.item_registry.as_ref() else {
            self.item_registry = Some(std::sync::Arc::new(next));
            self.item_facts = facts;
            self.refresh_fuel_facts();
            self.refresh_trade_items();
            return;
        };
        let affected_cells = self.registry_affected_cells(previous, &next);
        let affected_requests: Vec<i32> = self
            .queue
            .iter()
            .filter(|pending| {
                // Item facts feed derived results, so any unsent one is recomputed by a new take.
                (pending.state == super::InventoryPendingState::AwaitingTransport
                    && pending.predicts_derived_result())
                    || self.placement_admission_changes(pending, &facts)
                    || pending
                        .predicted
                        .iter()
                        .filter_map(|prediction| prediction.held.as_ref())
                        .map(|held| held.stack.network_id)
                        .any(|network_id| {
                            registry_identity_changed(previous, &next, network_id)
                                || (pending.predicted.iter().any(|prediction| {
                                    matches!(prediction.cell, Cell::Armor(_) | Cell::Offhand)
                                }) && (equipment_admission(&self.item_facts, network_id)
                                    != equipment_admission(&facts, network_id)
                                    || previous.get(&network_id).and_then(entry_capacity)
                                        != next.get(&network_id).and_then(entry_capacity)))
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
        self.item_facts = facts;
        self.refresh_fuel_facts();
        self.refresh_trade_items();
        self.retire_rerouted_quick_moves();
        self.refold();
    }

    /// Checks item compatibility and the settled identities needed for a new merge.
    pub(super) fn occupied_stack_relation(
        &self,
        source: &Held,
        destination: &Held,
    ) -> OccupiedStackRelation {
        let (source_stack, destination_stack) = (&source.stack, &destination.stack);
        if self.authority == Some(protocol::InventoryAuthority::Server)
            && (self.stack_awaiting_identity(source_stack)
                || self.stack_awaiting_identity(destination_stack)
                || ((source_stack.stack_network_id > 0
                    || protocol::is_legacy_request_id(source_stack.stack_network_id))
                    && source_stack.stack_network_id == destination_stack.stack_network_id))
        {
            return OccupiedStackRelation::Unsupported;
        }
        self.merge_item_relation(source, destination)
    }

    /// Compares retained item identity and capacity without requiring transport-ready stack IDs.
    pub(super) fn merge_item_relation(
        &self,
        source: &Held,
        destination: &Held,
    ) -> OccupiedStackRelation {
        let (source_held, destination_held) = (source, destination);
        let (source, destination) = (&source.stack, &destination.stack);
        let distinct_network_ids = source.network_id != destination.network_id;
        // Vanilla item matching compares block/aux identity; a
        // nonzero block identity is an ordinary block stack, not an unknown
        // stack shape. Count and sparse/server stack ids are not item identity.
        if source.metadata != destination.metadata
            || source.block_runtime_id != destination.block_runtime_id
        {
            return OccupiedStackRelation::Incompatible;
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
        match held_item_data_equal(source_held, destination_held) {
            Some(true) => {}
            Some(false) => return OccupiedStackRelation::Incompatible,
            None => return OccupiedStackRelation::Unsupported,
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

/// Compares retained user data after checking each descriptor's integrity.
pub(super) fn held_item_data_equal(left: &Held, right: &Held) -> Option<bool> {
    for held in [left, right] {
        let digest: [u8; 32] = Sha256::digest(&held.stack.extra_data).into();
        if digest != held.stack.nbt_digest {
            return None;
        }
    }
    protocol::item_user_data_equal(&left.stack.extra_data, &right.stack.extra_data)
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

/// Returns the entry's stack capacity, or `None` when its declarations are unresolved.
///
/// DataDriven entries use component capacities even for known names. Other entries keep
/// the known item's capacity, or use the component flag for an unknown name.
pub(super) fn entry_capacity(entry: &ItemRegistryEntry) -> Option<u8> {
    entry_capacity_with_base(entry, protocol::vanilla_item_capacity(&entry.identifier, 0))
}

/// Resolves negotiated capacity using a caller's admitted base capacity.
pub(crate) fn entry_capacity_with_base(
    entry: &ItemRegistryEntry,
    vanilla: Option<u8>,
) -> Option<u8> {
    let components = entry.components.as_deref();
    if components.is_some_and(|components| components.stack_capacity_ambiguous)
        || (components.is_none() && !entry.canonical_empty_component_data)
    {
        // Ambiguous or unavailable facts do not prove that capacity declarations were absent.
        return None;
    }
    let item_properties = components.is_some_and(|components| components.item_properties_declared);
    let stack_component =
        components.is_some_and(|components| components.max_stack_size_component_declared);
    let component_item = matches!(entry.version, ItemRegistryVersion::DataDriven)
        || (vanilla.is_none()
            && entry.component_based
            && !matches!(entry.version, ItemRegistryVersion::Legacy));
    if component_item {
        return if item_properties {
            entry.negotiated_max_stack_size
        } else if stack_component {
            components.and_then(|components| components.max_stack_size)
        } else {
            Some(protocol::ITEM_DEFAULT_MAX_STACK_SIZE)
        };
    }
    if stack_component && vanilla.is_none() && entry.version == ItemRegistryVersion::Legacy {
        return components.and_then(|components| components.max_stack_size_component);
    }
    if stack_component {
        // The top-level stack component's effect on plain and vanilla items is unresolved.
        return None;
    }
    Some(vanilla.unwrap_or(protocol::ITEM_DEFAULT_MAX_STACK_SIZE))
}

/// Checks retained byte integrity without interpreting pending request IDs as wire stack IDs.
pub(super) fn recipe_input_valid(stack: &NetworkItemStack) -> bool {
    <[u8; 32]>::from(Sha256::digest(&stack.extra_data)) == stack.nbt_digest
}

#[cfg(test)]
pub(super) fn plain_stack(stack: &NetworkItemStack) -> bool {
    let digest: [u8; 32] = Sha256::digest(&stack.extra_data).into();
    // Plainness describes user data, independently of aux and block identity.
    (stack.extra_data.is_empty() || stack.extra_data.as_ref() == [0; 10])
        && stack.nbt_digest == digest
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{ItemComponents, ItemRegistryEntry, ItemRegistryVersion};

    use super::entry_capacity;

    /// A DataDriven entry whose components declare `declared` through `item_properties`, or
    /// carry no components.
    fn entry(identifier: &str, component_based: bool, declared: Option<u8>) -> ItemRegistryEntry {
        ItemRegistryEntry {
            identifier: identifier.into(),
            network_id: 900,
            component_based,
            version: ItemRegistryVersion::DataDriven,
            components: declared.map(|size| {
                Arc::new(ItemComponents {
                    max_stack_size: Some(size),
                    item_properties_declared: true,
                    ..ItemComponents::default()
                })
            }),
            component_digest: [0; 32],
            negotiated_max_stack_size: declared,
            canonical_empty_component_data: declared.is_none(),
            item_tags: Arc::from([]),
        }
    }

    /// `entry` registered under `version`.
    fn versioned(entry: ItemRegistryEntry, version: ItemRegistryVersion) -> ItemRegistryEntry {
        ItemRegistryEntry { version, ..entry }
    }

    #[test]
    fn unparsed_component_data_keeps_capacity_unresolved() {
        for version in [
            ItemRegistryVersion::DataDriven,
            ItemRegistryVersion::Legacy,
            ItemRegistryVersion::None,
            ItemRegistryVersion::Unknown(99),
        ] {
            for identifier in ["minecraft:egg", "test:gem"] {
                for component_based in [false, true] {
                    let unresolved = ItemRegistryEntry {
                        components: None,
                        canonical_empty_component_data: false,
                        ..versioned(entry(identifier, component_based, Some(1)), version)
                    };
                    assert_eq!(entry_capacity(&unresolved), None);
                }
            }
        }
    }

    // DataDriven entries register as component items ahead of the vanilla classes, so their
    // stack size comes from item_properties or the item default, never the vanilla class.
    #[test]
    fn data_driven_entries_take_item_properties_or_the_item_default() {
        assert_eq!(entry_capacity(&entry("zeqa:gem", true, Some(16))), Some(16));
        assert_eq!(
            entry_capacity(&entry("zeqa:gem", false, Some(16))),
            Some(16)
        );
        assert_eq!(entry_capacity(&entry("zeqa:gem", true, None)), Some(64));
        let parsed_without_declarations = ItemRegistryEntry {
            components: Some(Arc::new(ItemComponents::default())),
            canonical_empty_component_data: false,
            ..entry("zeqa:gem", true, None)
        };
        assert_eq!(
            entry_capacity(&parsed_without_declarations),
            Some(protocol::ITEM_DEFAULT_MAX_STACK_SIZE)
        );
        assert_eq!(
            entry_capacity(&entry("minecraft:egg", true, None)),
            Some(64),
            "an empty component item replaces the vanilla egg"
        );
        let undeclared = ItemRegistryEntry {
            components: Some(Arc::new(ItemComponents {
                item_properties_declared: true,
                ..ItemComponents::default()
            })),
            ..entry("zeqa:gem", true, None)
        };
        assert_eq!(
            entry_capacity(&undeclared),
            None,
            "properties without a size"
        );
    }

    // Other entries keep the vanilla class of their name whatever the component flag says.
    #[test]
    fn legacy_and_unversioned_vanilla_entries_keep_the_vanilla_class() {
        for version in [ItemRegistryVersion::Legacy, ItemRegistryVersion::None] {
            assert_eq!(
                entry_capacity(&versioned(entry("minecraft:egg", true, None), version)),
                Some(16)
            );
            assert_eq!(
                entry_capacity(&versioned(entry("minecraft:stick", false, None), version)),
                Some(64)
            );
        }
        assert_eq!(
            entry_capacity(&versioned(
                entry("zeqa:gem", true, Some(16)),
                ItemRegistryVersion::Legacy
            )),
            Some(64),
            "a Legacy entry is a plain item, which ignores item_properties"
        );
        assert_eq!(
            entry_capacity(&versioned(
                entry("zeqa:gem", true, Some(16)),
                ItemRegistryVersion::None
            )),
            Some(16),
            "an unversioned component-based entry is a component item"
        );
    }

    // A component item without item_properties takes the stack size component; plain items
    // read it differently, so they leave the stack size unsettled.
    #[test]
    fn the_stack_size_component_binds_only_component_items() {
        let with_component = |entry: ItemRegistryEntry| ItemRegistryEntry {
            components: Some(Arc::new(ItemComponents {
                max_stack_size: Some(1),
                max_stack_size_component_declared: true,
                ..ItemComponents::default()
            })),
            canonical_empty_component_data: false,
            ..entry
        };
        assert_eq!(
            entry_capacity(&with_component(entry("zeqa:gem", true, None))),
            Some(1)
        );
        assert_eq!(
            entry_capacity(&with_component(versioned(
                entry("minecraft:stick", false, None),
                ItemRegistryVersion::Legacy
            ))),
            None
        );
    }

    #[test]
    fn custom_content_legacy_items_use_declared_stack_capacity() {
        for size in [1, 16, 64] {
            let legacy = ItemRegistryEntry {
                version: ItemRegistryVersion::Legacy,
                components: Some(Arc::new(ItemComponents {
                    max_stack_size: Some(size),
                    max_stack_size_component: Some(size),
                    max_stack_size_component_declared: true,
                    ..Default::default()
                })),
                ..entry("fixture:legacy", false, None)
            };
            assert_eq!(entry_capacity(&legacy), Some(size));
        }
    }

    #[test]
    fn custom_content_legacy_scalar_capacity_is_independent_from_item_properties() {
        let legacy = ItemRegistryEntry {
            version: ItemRegistryVersion::Legacy,
            negotiated_max_stack_size: Some(64),
            components: Some(Arc::new(ItemComponents {
                max_stack_size: Some(64),
                max_stack_size_component: Some(1),
                max_stack_size_component_declared: true,
                item_properties_declared: true,
                ..Default::default()
            })),
            ..entry("fixture:legacy", false, None)
        };
        assert_eq!(entry_capacity(&legacy), Some(1));
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
