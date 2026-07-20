use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use assets::{
    BlockVisualId, ItemStackIdentity, ItemVisualDefinitionRoute, ItemVisualId, ItemVisualKey,
    ItemVisualRoute, RuntimeEntityAssets,
};
use protocol::{
    ActorHandedness, EquipmentEvent, ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion,
    NetworkItemStack,
};
use sha2::{Digest, Sha256};

use crate::{ActorEventIdentity, ActorLifetimeId, ActorSourceTick};

pub const MAX_ITEM_REGISTRY_RECORDS: usize = 16_384;
pub const MAX_PENDING_ITEM_RESOLUTIONS: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalItemStack {
    pub identity: ItemStackIdentity,
    pub identifier: Option<Arc<str>>,
    pub visual: ItemVisualRoute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalItemRegistryRecord {
    pub identifier: Arc<str>,
    pub network_id: i32,
    pub component_based: bool,
    pub version: ItemRegistryVersion,
    pub component_digest: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorEquipmentSnapshot {
    pub actor: ActorLifetimeId,
    pub event: ActorEventIdentity,
    pub item: CanonicalItemStack,
    pub inventory_slot: i32,
    pub selected_slot: u8,
    pub window_id: u8,
    pub hand: ActorHandedness,
    pub hand_defaulted: bool,
}

#[derive(Debug)]
pub(crate) struct ItemStateStore {
    assets: Option<Arc<RuntimeEntityAssets>>,
    registry: BTreeMap<i32, CanonicalItemRegistryRecord>,
    equipment: BTreeMap<ActorLifetimeId, ActorEquipmentSnapshot>,
    pending: VecDeque<ActorLifetimeId>,
}

impl ItemStateStore {
    pub(crate) fn diagnostic() -> Self {
        Self::new(None)
    }

    pub(crate) fn with_assets(assets: Arc<RuntimeEntityAssets>) -> Self {
        Self::new(Some(assets))
    }

    fn new(assets: Option<Arc<RuntimeEntityAssets>>) -> Self {
        Self {
            assets,
            registry: BTreeMap::new(),
            equipment: BTreeMap::new(),
            pending: VecDeque::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.registry.clear();
        self.clear_actor_state();
    }

    pub(crate) fn clear_actor_state(&mut self) {
        self.equipment.clear();
        self.pending.clear();
    }

    pub(crate) fn remove(&mut self, lifetime: ActorLifetimeId) {
        self.equipment.remove(&lifetime);
        self.pending.retain(|pending| *pending != lifetime);
    }

    pub(crate) fn insert_spawn(
        &mut self,
        lifetime: ActorLifetimeId,
        sequence: u64,
        stack: NetworkItemStack,
    ) {
        self.remove_runtime(lifetime.runtime_id);
        let Some(item) = self.canonicalize(&stack) else {
            return;
        };
        let unresolved = !item.identity.is_empty() && item.identifier.is_none();
        self.equipment.insert(
            lifetime,
            ActorEquipmentSnapshot {
                actor: lifetime,
                event: event_identity(
                    lifetime,
                    sequence,
                    ActorSourceTick::IngressSequence(sequence),
                ),
                item,
                inventory_slot: -1,
                selected_slot: 0,
                window_id: u8::MAX,
                hand: ActorHandedness::Right,
                hand_defaulted: true,
            },
        );
        if unresolved {
            self.retain_pending(lifetime);
        }
    }

    pub(crate) fn apply_equipment(
        &mut self,
        lifetime: ActorLifetimeId,
        sequence: u64,
        equipment: EquipmentEvent,
    ) -> bool {
        let Some(item) = self.canonicalize(&equipment.stack) else {
            return false;
        };
        let unresolved = !item.identity.is_empty() && item.identifier.is_none();
        let (hand, hand_defaulted) = equipment
            .handedness
            .map_or((ActorHandedness::Right, true), |hand| (hand, false));
        self.equipment.insert(
            lifetime,
            ActorEquipmentSnapshot {
                actor: lifetime,
                event: event_identity(
                    lifetime,
                    sequence,
                    ActorSourceTick::IngressSequence(sequence),
                ),
                item,
                inventory_slot: equipment.inventory_slot,
                selected_slot: equipment.selected_slot,
                window_id: equipment.window_id,
                hand,
                hand_defaulted,
            },
        );
        self.pending.retain(|pending| *pending != lifetime);
        if unresolved {
            self.retain_pending(lifetime);
        }
        true
    }

    pub(crate) fn apply_registry(&mut self, registry: ItemRegistryEvent) -> bool {
        if registry.entries.len() > MAX_ITEM_REGISTRY_RECORDS {
            return false;
        }
        let mut next = BTreeMap::new();
        let mut identifiers = HashMap::with_capacity(registry.entries.len());
        for entry in registry.entries.iter() {
            if next.contains_key(&entry.network_id)
                || identifiers
                    .insert(Arc::clone(&entry.identifier), ())
                    .is_some()
            {
                return false;
            }
            next.insert(entry.network_id, registry_record(entry));
        }
        self.registry = next;

        let lifetimes = self.equipment.keys().copied().collect::<Vec<_>>();
        self.pending.clear();
        for lifetime in lifetimes {
            let Some(identity) = self
                .equipment
                .get(&lifetime)
                .map(|equipment| equipment.item.identity)
            else {
                continue;
            };
            let item = self.resolve_identity(identity);
            let unresolved = !item.identity.is_empty() && item.identifier.is_none();
            if let Some(equipment) = self.equipment.get_mut(&lifetime) {
                equipment.item = item;
            }
            if unresolved {
                self.retain_pending(lifetime);
            }
        }
        true
    }

    pub(crate) fn get(&self, lifetime: ActorLifetimeId) -> Option<&ActorEquipmentSnapshot> {
        self.equipment.get(&lifetime)
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    fn canonicalize(&self, stack: &NetworkItemStack) -> Option<CanonicalItemStack> {
        let digest: [u8; 32] = Sha256::digest(stack.extra_data.as_ref()).into();
        if digest != stack.nbt_digest {
            return None;
        }
        let identity = ItemStackIdentity {
            network_id: stack.network_id,
            metadata: stack.metadata,
            stack_network_id: stack.stack_network_id,
            count: stack.count,
            nbt_digest: stack.nbt_digest,
        }
        .validate()
        .ok()?;
        Some(self.resolve_identity(identity))
    }

    fn resolve_identity(&self, identity: ItemStackIdentity) -> CanonicalItemStack {
        if identity.is_empty() {
            return CanonicalItemStack {
                identity,
                identifier: None,
                visual: ItemVisualRoute::EmptyHand,
            };
        }
        let identifier = self
            .registry
            .get(&identity.network_id)
            .map(|record| Arc::clone(&record.identifier));
        let visual = identifier
            .as_deref()
            .map_or(ItemVisualRoute::Missing, |identifier| {
                self.resolve_visual(identifier, identity.metadata)
            });
        CanonicalItemStack {
            identity,
            identifier,
            visual,
        }
    }

    fn resolve_visual(&self, identifier: &str, metadata: u32) -> ItemVisualRoute {
        let Some(assets) = self.assets.as_ref() else {
            return ItemVisualRoute::Missing;
        };
        let key = ItemVisualKey {
            identifier: identifier.into(),
            metadata,
        };
        if let Ok(index) = assets
            .item_visuals()
            .binary_search_by(|visual| visual.key.cmp(&key))
        {
            return match assets.item_visuals()[index].route {
                ItemVisualDefinitionRoute::Sprite { .. } => {
                    ItemVisualRoute::Compiled(ItemVisualId(index as u32))
                }
                ItemVisualDefinitionRoute::BlockItem { block_visual } => {
                    ItemVisualRoute::BlockItem(BlockVisualId(block_visual.0))
                }
                ItemVisualDefinitionRoute::EmptyHand => ItemVisualRoute::EmptyHand,
                ItemVisualDefinitionRoute::Missing => ItemVisualRoute::Missing,
            };
        }
        assets
            .item_visual_aliases()
            .binary_search_by(|alias| alias.key.cmp(&key))
            .ok()
            .map_or(ItemVisualRoute::Missing, |index| {
                ItemVisualRoute::Compiled(assets.item_visual_aliases()[index].visual)
            })
    }

    fn retain_pending(&mut self, lifetime: ActorLifetimeId) {
        if self.pending.len() < MAX_PENDING_ITEM_RESOLUTIONS && !self.pending.contains(&lifetime) {
            self.pending.push_back(lifetime);
        }
    }

    fn remove_runtime(&mut self, runtime_id: u64) {
        let lifetimes = self
            .equipment
            .keys()
            .copied()
            .filter(|lifetime| lifetime.runtime_id == runtime_id)
            .collect::<Vec<_>>();
        for lifetime in lifetimes {
            self.remove(lifetime);
        }
    }
}

fn registry_record(entry: &ItemRegistryEntry) -> CanonicalItemRegistryRecord {
    CanonicalItemRegistryRecord {
        identifier: Arc::clone(&entry.identifier),
        network_id: entry.network_id,
        component_based: entry.component_based,
        version: entry.version,
        component_digest: entry.component_digest,
    }
}

fn event_identity(
    actor: ActorLifetimeId,
    ingress_sequence: u64,
    source_tick: ActorSourceTick,
) -> ActorEventIdentity {
    ActorEventIdentity {
        session_id: actor.session_id,
        dimension: actor.dimension,
        actor_lifetime: actor.spawn_revision,
        ingress_sequence,
        source_tick,
    }
}
