use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use assets::{
    BlockVisualId, ItemStackIdentity, ItemVisualDefinitionRoute, ItemVisualId, ItemVisualKey,
    ItemVisualRoute, RuntimeEntityAssets,
};
use protocol::{
    ActorHandedness, ArmorEquipmentEvent, EquipmentEvent, ItemRegistryEntry, ItemRegistryEvent,
    ItemRegistryVersion, NetworkItemStack,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ActorItemKind {
    #[default]
    Empty,
    Shield,
    FilledMap,
    Crossbow,
    Bow,
    Brush,
    HeavyCore,
    Spear,
    Spyglass,
    GoatHorn,
    Other,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ActorItemInput {
    pub(crate) main_hand: ActorItemKind,
    pub(crate) off_hand: ActorItemKind,
    pub(crate) armor_layers: [bool; 5],
    pub(crate) main_hand_remaining_use_duration: f32,
    pub(crate) off_hand_remaining_use_duration: f32,
    pub(crate) main_hand_charged: bool,
    pub(crate) off_hand_charged: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorArmorSnapshot {
    pub actor: ActorLifetimeId,
    pub event: ActorEventIdentity,
    pub helmet: CanonicalItemStack,
    pub chestplate: CanonicalItemStack,
    pub leggings: CanonicalItemStack,
    pub boots: CanonicalItemStack,
    pub body: CanonicalItemStack,
}

#[derive(Debug)]
pub(crate) struct ItemStateStore {
    assets: Option<Arc<RuntimeEntityAssets>>,
    registry: BTreeMap<i32, CanonicalItemRegistryRecord>,
    equipment: BTreeMap<ActorLifetimeId, ActorEquipmentSnapshot>,
    offhand: BTreeMap<ActorLifetimeId, ActorEquipmentSnapshot>,
    armor: BTreeMap<ActorLifetimeId, ActorArmorSnapshot>,
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
            registry: built_in_registry(),
            equipment: BTreeMap::new(),
            offhand: BTreeMap::new(),
            armor: BTreeMap::new(),
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
        self.offhand.clear();
        self.armor.clear();
        self.pending.clear();
    }

    pub(crate) fn remove(&mut self, lifetime: ActorLifetimeId) {
        self.equipment.remove(&lifetime);
        self.offhand.remove(&lifetime);
        self.armor.remove(&lifetime);
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
        let target = match hand {
            ActorHandedness::Left => &mut self.offhand,
            ActorHandedness::Right => &mut self.equipment,
        };
        target.insert(
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

    pub(crate) fn apply_armor_equipment(
        &mut self,
        lifetime: ActorLifetimeId,
        sequence: u64,
        armor: ArmorEquipmentEvent,
    ) -> bool {
        let Some(helmet) = self.canonicalize(&armor.helmet) else {
            return false;
        };
        let Some(chestplate) = self.canonicalize(&armor.chestplate) else {
            return false;
        };
        let Some(leggings) = self.canonicalize(&armor.leggings) else {
            return false;
        };
        let Some(boots) = self.canonicalize(&armor.boots) else {
            return false;
        };
        let Some(body) = self.canonicalize(&armor.body) else {
            return false;
        };
        let unresolved = [&helmet, &chestplate, &leggings, &boots, &body]
            .into_iter()
            .any(|item| !item.identity.is_empty() && item.identifier.is_none());
        self.armor.insert(
            lifetime,
            ActorArmorSnapshot {
                actor: lifetime,
                event: event_identity(
                    lifetime,
                    sequence,
                    ActorSourceTick::IngressSequence(sequence),
                ),
                helmet,
                chestplate,
                leggings,
                boots,
                body,
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
        let mut next = built_in_registry();
        let mut identifiers = HashMap::with_capacity(registry.entries.len());
        let mut network_ids = HashMap::with_capacity(registry.entries.len());
        for entry in registry.entries.iter() {
            if network_ids.insert(entry.network_id, ()).is_some()
                || identifiers
                    .insert(Arc::clone(&entry.identifier), ())
                    .is_some()
            {
                return false;
            }
            next.insert(entry.network_id, registry_record(entry));
        }
        self.registry = next;

        let mut lifetimes = self.equipment.keys().copied().collect::<Vec<_>>();
        lifetimes.extend(self.offhand.keys().copied());
        lifetimes.extend(self.armor.keys().copied());
        lifetimes.sort_unstable();
        lifetimes.dedup();
        self.pending.clear();
        for lifetime in lifetimes {
            for offhand in [false, true] {
                let identity = if offhand {
                    self.offhand
                        .get(&lifetime)
                        .map(|equipment| equipment.item.identity)
                } else {
                    self.equipment
                        .get(&lifetime)
                        .map(|equipment| equipment.item.identity)
                };
                let Some(identity) = identity else { continue };
                let item = self.resolve_identity(identity);
                let unresolved = !item.identity.is_empty() && item.identifier.is_none();
                let target = if offhand {
                    &mut self.offhand
                } else {
                    &mut self.equipment
                };
                if let Some(equipment) = target.get_mut(&lifetime) {
                    equipment.item = item;
                }
                if unresolved {
                    self.retain_pending(lifetime);
                }
            }
            let Some(identities) = self.armor.get(&lifetime).map(|armor| {
                [
                    armor.helmet.identity,
                    armor.chestplate.identity,
                    armor.leggings.identity,
                    armor.boots.identity,
                    armor.body.identity,
                ]
            }) else {
                continue;
            };
            let resolved = identities.map(|identity| self.resolve_identity(identity));
            let unresolved = resolved
                .iter()
                .any(|item| !item.identity.is_empty() && item.identifier.is_none());
            if let Some(armor) = self.armor.get_mut(&lifetime) {
                [
                    &mut armor.helmet,
                    &mut armor.chestplate,
                    &mut armor.leggings,
                    &mut armor.boots,
                    &mut armor.body,
                ]
                .into_iter()
                .zip(resolved)
                .for_each(|(target, item)| *target = item);
            }
            if unresolved {
                self.retain_pending(lifetime);
            }
        }
        true
    }

    pub(crate) fn get(&self, lifetime: ActorLifetimeId) -> Option<&ActorEquipmentSnapshot> {
        let right = self.equipment.get(&lifetime);
        if right.is_some_and(|equipment| !equipment.item.identity.is_empty()) {
            return right;
        }
        self.offhand.get(&lifetime).or(right)
    }

    pub(crate) fn get_hand(
        &self,
        lifetime: ActorLifetimeId,
        hand: ActorHandedness,
    ) -> Option<&ActorEquipmentSnapshot> {
        match hand {
            ActorHandedness::Left => self.offhand.get(&lifetime),
            ActorHandedness::Right => self.equipment.get(&lifetime),
        }
    }

    pub(crate) fn armor(&self, lifetime: ActorLifetimeId) -> Option<&ActorArmorSnapshot> {
        self.armor.get(&lifetime)
    }

    pub(crate) fn animation_input(&self, lifetime: ActorLifetimeId) -> ActorItemInput {
        let armor_layers = self
            .armor(lifetime)
            .map(|armor| {
                [
                    !armor.helmet.identity.is_empty(),
                    !armor.chestplate.identity.is_empty(),
                    !armor.leggings.identity.is_empty(),
                    !armor.boots.identity.is_empty(),
                    !armor.body.identity.is_empty(),
                ]
            })
            .unwrap_or_default();
        ActorItemInput {
            main_hand: item_kind(self.get_hand(lifetime, ActorHandedness::Right)),
            off_hand: item_kind(self.get_hand(lifetime, ActorHandedness::Left)),
            armor_layers,
            ..ActorItemInput::default()
        }
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn canonicalize(&self, stack: &NetworkItemStack) -> Option<CanonicalItemStack> {
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
        };
        let identity = if identity.count == 0 {
            ItemStackIdentity::empty()
        } else if identity.network_id == 0 {
            return None;
        } else {
            identity
        };
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
            .chain(self.offhand.keys().copied())
            .chain(self.armor.keys().copied())
            .filter(|lifetime| lifetime.runtime_id == runtime_id)
            .collect::<Vec<_>>();
        for lifetime in lifetimes {
            self.remove(lifetime);
        }
    }
}

fn item_kind(equipment: Option<&ActorEquipmentSnapshot>) -> ActorItemKind {
    let Some(identifier) = equipment.and_then(|equipment| equipment.item.identifier.as_deref())
    else {
        return ActorItemKind::Empty;
    };
    match identifier.rsplit(':').next().unwrap_or(identifier) {
        "shield" => ActorItemKind::Shield,
        "filled_map" => ActorItemKind::FilledMap,
        "crossbow" => ActorItemKind::Crossbow,
        "bow" => ActorItemKind::Bow,
        "brush" => ActorItemKind::Brush,
        "heavy_core" => ActorItemKind::HeavyCore,
        name if name.ends_with("_spear") => ActorItemKind::Spear,
        "spyglass" => ActorItemKind::Spyglass,
        "goat_horn" => ActorItemKind::GoatHorn,
        _ => ActorItemKind::Other,
    }
}

fn built_in_registry() -> BTreeMap<i32, CanonicalItemRegistryRecord> {
    protocol::vanilla_item_registry()
        .iter()
        .map(|entry| (entry.network_id, registry_record(entry)))
        .collect()
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
