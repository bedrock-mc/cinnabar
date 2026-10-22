mod use_profile;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, OnceLock},
};
pub use use_profile::{
    EffectiveItemUse, ResolvedUseCooldown, UseCooldownCategory, resolve_item_use,
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

use crate::{ActorEventIdentity, ActorSourceTick};
use actor_animation::ActorLifetimeId;

mod armour;
mod durability;
pub use durability::resolve_item_durability;
#[cfg(test)]
mod animation_tests;
mod glint;
mod grip;
#[cfg(test)]
mod map_tests;
#[cfg(test)]
mod presentation_tests;

pub const MAX_ITEM_REGISTRY_RECORDS: usize = protocol::MAX_ITEM_REGISTRY_ENTRIES;
pub const MAX_PENDING_ITEM_RESOLUTIONS: usize = 1_024;
/// Equipment notices kept between drains; later ones are dropped.
pub const MAX_EQUIPMENT_NOTICES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalItemStack {
    /// Drawing role resolved for the current numeric item registry.
    pub use_animation: assets::item_use::ItemUseAnimation,
    /// Grip resolved for the current numeric item registry generation.
    pub grip: assets::item::grip::ItemGrip,
    /// Intrinsic shine resolved for the current numeric registry generation.
    pub glint: assets::item::glint::ItemGlint,
    pub identity: ItemStackIdentity,
    pub identifier: Option<Arc<str>>,
    pub visual: ItemVisualRoute,
    /// Projectile a loaded crossbow holds; `None` for any uncharged stack.
    pub charged_projectile: Option<Arc<str>>,
    /// Signed image identity retained independently of visual metadata.
    pub map_id: Option<i64>,
    /// Projectile drawing facts resolved when this stack enters canonical admission.
    pub charged_projectile_role: assets::item_use::ItemProjectileRole,
    /// Durability damage retained separately from the stack's visual metadata.
    pub damage: Option<u32>,
    /// Maximum resolved at registry admission; gameplay and presentation share this fact.
    pub durability: assets::item::durability::ItemDurability,
    /// The stack carries the enchantment list that enables worn item glint.
    pub enchanted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalItemRegistryRecord {
    /// Descriptor after the current component payload has been admitted.
    pub grip: assets::item::grip::ItemGrip,
    /// Intrinsic shine resolved for the current numeric registry generation.
    pub glint: assets::item::glint::ItemGlint,
    pub identifier: Arc<str>,
    pub network_id: i32,
    pub component_based: bool,
    pub version: ItemRegistryVersion,
    pub component_digest: [u8; 32],
    /// Immutable session evidence used only when the effective cache is rebuilt.
    pub components: Option<Arc<protocol::ItemComponents>>,
    /// Compiled facts after applying the current registry's component payload.
    pub armour: assets::item::armour::ItemArmour,
    pub durability: assets::item::durability::ItemDurability,
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

/// One worn armor stack with the dye colour its NBT carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorArmorPiece {
    pub item: CanonicalItemStack,
    /// Leather dye RGB (24-bit) from the stack's `customColor` tag.
    pub dye_rgb: Option<u32>,
}

/// An actor's five worn armor stacks from its latest MobArmorEquipment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorArmorSnapshot {
    /// Lifetime the stacks were applied to; `spawn_revision` 0 when the actor did not exist yet.
    pub actor: ActorLifetimeId,
    pub event: ActorEventIdentity,
    pub helmet: ActorArmorPiece,
    pub chestplate: ActorArmorPiece,
    pub leggings: ActorArmorPiece,
    pub boots: ActorArmorPiece,
    pub body: ActorArmorPiece,
}

/// Where one MobEquipment or MobArmorEquipment landed, for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipmentNotice {
    pub runtime_id: u64,
    pub armor: bool,
    pub outcome: EquipmentOutcome,
    /// Identifiers of the event's non-empty stacks; `None` where the registry has no entry.
    pub items: Box<[Option<Arc<str>>]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipmentOutcome {
    Applied,
    /// No live actor has the runtime id.
    UnknownActor,
    /// The local player's held stacks come from the client-owned inventory instead.
    LocalPlayer,
    /// A stack's NBT digest did not match its payload, or it named network id 0.
    RejectedStack,
    /// Older than the latest applied actor event, or from another session.
    Stale,
}

type EquipmentKey = (ActorLifetimeId, ActorHandedness);

/// Every hand that keys actor equipment. The match fails to compile when a hand is added, so
/// keyed removal and lookup cannot silently miss one.
const HANDS: [ActorHandedness; 2] = {
    match ActorHandedness::Left {
        ActorHandedness::Left | ActorHandedness::Right => {}
    }
    [ActorHandedness::Left, ActorHandedness::Right]
};

#[derive(Debug)]
pub(crate) struct ItemStateStore {
    presentation_generation: Arc<()>,
    assets: Option<Arc<RuntimeEntityAssets>>,
    registry: BTreeMap<i32, CanonicalItemRegistryRecord>,
    facts: inventory::SessionItemFacts,
    skipped_registries: u64,
    equipment: BTreeMap<EquipmentKey, ActorEquipmentSnapshot>,
    pending: VecDeque<EquipmentKey>,
    /// Latest armor by runtime id; the client-owned runtime survives actor churn.
    armor: BTreeMap<u64, ActorArmorSnapshot>,
    persistent_armor_runtime: Option<u64>,
    use_durations: Option<Arc<BTreeMap<Box<str>, u32>>>,
    use_profiles: BTreeMap<i32, EffectiveItemUse>,
    projectile_roles: BTreeMap<Arc<str>, assets::item_use::ItemProjectileRole>,
    use_overlays: Option<Arc<BTreeMap<Box<str>, assets::equipment::CompiledItemUseOverlay>>>,
    attack_timings: Option<Arc<BTreeMap<Box<str>, protocol::ItemAttackTiming>>>,
    notices: Vec<EquipmentNotice>,
}

impl ItemStateStore {
    /// Retains this store's registry identity independently of its address.
    pub(crate) fn presentation_generation(&self) -> &Arc<()> {
        &self.presentation_generation
    }

    pub(crate) fn diagnostic() -> Self {
        Self::new(None)
    }

    pub(crate) fn with_assets(assets: Arc<RuntimeEntityAssets>) -> Self {
        Self::new(Some(assets))
    }

    fn new(assets: Option<Arc<RuntimeEntityAssets>>) -> Self {
        let registry = built_in_registry(assets.as_deref());
        let mut store = Self {
            presentation_generation: Arc::new(()),
            assets,
            registry,
            facts: inventory::SessionItemFacts::default(),
            skipped_registries: 0,
            equipment: BTreeMap::new(),
            pending: VecDeque::new(),
            armor: BTreeMap::new(),
            persistent_armor_runtime: None,
            use_durations: None,
            use_profiles: BTreeMap::new(),
            projectile_roles: BTreeMap::new(),
            use_overlays: None,
            attack_timings: None,
            notices: Vec::new(),
        };
        store.rebuild_use_profiles();
        store
    }

    /// Records where an equipment event landed; the stacks are named through the registry.
    pub(crate) fn note(
        &mut self,
        runtime_id: u64,
        armor: bool,
        outcome: EquipmentOutcome,
        stacks: &[&NetworkItemStack],
    ) {
        if self.notices.len() >= MAX_EQUIPMENT_NOTICES {
            return;
        }
        let items = stacks
            .iter()
            .filter(|stack| !stack.is_empty())
            .map(|stack| self.identifier_for_network_id(stack.network_id))
            .collect();
        self.notices.push(EquipmentNotice {
            runtime_id,
            armor,
            outcome,
            items,
        });
    }

    pub(crate) fn take_notices(&mut self) -> Vec<EquipmentNotice> {
        std::mem::take(&mut self.notices)
    }

    pub(crate) fn set_use_durations(&mut self, durations: Arc<BTreeMap<Box<str>, u32>>) {
        if !self
            .use_durations
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &durations))
        {
            self.use_durations = Some(durations);
            self.rebuild_use_profiles();
        }
    }

    /// Replaces the session's effective compiled and server-defined attack facts.
    pub(crate) fn set_attack_timings(
        &mut self,
        timings: Arc<BTreeMap<Box<str>, protocol::ItemAttackTiming>>,
    ) {
        self.attack_timings = Some(timings);
    }

    /// Shares the same selected-item facts with packet admission and animation.
    pub(crate) fn attack_timing(&self, identifier: &str) -> Option<&protocol::ItemAttackTiming> {
        self.attack_timings.as_ref()?.get(identifier)
    }

    /// Keeps this runtime's armor across actor removal and dimension resets (the local player).
    pub(crate) fn set_persistent_armor_runtime(&mut self, runtime_id: u64) {
        self.persistent_armor_runtime = Some(runtime_id);
    }

    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.presentation_generation = Arc::new(());
        self.registry.clear();
        self.armor.clear();
        self.clear_actor_state();
    }

    pub(crate) fn clear_actor_state(&mut self) {
        self.equipment.clear();
        self.pending.clear();
        let persistent = self.persistent_armor_runtime;
        self.armor
            .retain(|runtime_id, _| Some(*runtime_id) == persistent);
    }

    pub(crate) fn remove(&mut self, lifetime: ActorLifetimeId) {
        for hand in HANDS {
            self.equipment.remove(&(lifetime, hand));
        }
        self.pending.retain(|(actor, _)| *actor != lifetime);
        if self.persistent_armor_runtime != Some(lifetime.runtime_id) {
            self.armor.remove(&lifetime.runtime_id);
        }
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
        let key = (lifetime, ActorHandedness::Right);
        self.equipment.insert(
            key,
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
            self.retain_pending(key);
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
        let key = (lifetime, hand);
        self.equipment.insert(
            key,
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
        self.pending.retain(|pending| *pending != key);
        if unresolved {
            self.retain_pending(key);
        }
        true
    }

    /// Stores all five worn stacks, or rejects the event if any stack's NBT digest is wrong.
    pub(crate) fn apply_armor(
        &mut self,
        lifetime: ActorLifetimeId,
        sequence: u64,
        event: &ArmorEquipmentEvent,
    ) -> bool {
        let (Some(helmet), Some(chestplate), Some(leggings), Some(boots), Some(body)) = (
            self.armor_piece(&event.helmet),
            self.armor_piece(&event.chestplate),
            self.armor_piece(&event.leggings),
            self.armor_piece(&event.boots),
            self.armor_piece(&event.body),
        ) else {
            return false;
        };
        self.armor.insert(
            lifetime.runtime_id,
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
        true
    }

    pub(crate) fn armor(&self, runtime_id: u64) -> Option<&ActorArmorSnapshot> {
        self.armor.get(&runtime_id)
    }

    fn armor_piece(&self, stack: &NetworkItemStack) -> Option<ActorArmorPiece> {
        Some(ActorArmorPiece {
            item: self.canonicalize(stack)?,
            dye_rgb: protocol::item_custom_color(&stack.extra_data),
        })
    }

    /// Copies negotiated animation facts by numeric item identity.
    pub(crate) fn animation_fact(&self, network_id: i32) -> assets::facts::ItemFact {
        let facts = if self.facts.kind(network_id).is_some() {
            &self.facts
        } else {
            retail_animation_facts()
        };
        *facts.get(network_id)
    }

    pub(crate) fn apply_registry(&mut self, registry: ItemRegistryEvent) -> bool {
        let Some(facts) = inventory::SessionItemFacts::from_registry(
            assets::facts::ItemFacts::builtin().expect("valid pinned item facts"),
            &registry,
        ) else {
            self.skipped_registries = self.skipped_registries.saturating_add(1);
            if self.skipped_registries.is_power_of_two() {
                tracing::warn!(
                    skipped = self.skipped_registries,
                    entries = registry.entries.len(),
                    "Skipping item registry outside numeric admission bounds"
                );
            }
            return false;
        };
        let mut next = built_in_registry(self.assets.as_deref());
        for entry in registry.entries.iter() {
            next.insert(
                entry.network_id,
                registry_record(entry, self.assets.as_deref()),
            );
        }
        self.facts = facts;
        self.registry = next;
        self.rebuild_use_profiles();

        let runtimes = self.armor.keys().copied().collect::<Vec<_>>();
        for runtime_id in runtimes {
            let Some(mut snapshot) = self.armor.remove(&runtime_id) else {
                continue;
            };
            for piece in [
                &mut snapshot.helmet,
                &mut snapshot.chestplate,
                &mut snapshot.leggings,
                &mut snapshot.boots,
                &mut snapshot.body,
            ] {
                let charged = piece.item.charged_projectile.take();
                let damage = piece.item.damage;
                let enchanted = piece.item.enchanted;
                let map_id = piece.item.map_id;
                piece.item = self.resolve_identity(piece.item.identity);
                piece.item.charged_projectile = charged;
                piece.item.charged_projectile_role =
                    self.projectile_role(piece.item.charged_projectile.as_deref());
                piece.item.damage = damage;
                piece.item.enchanted = enchanted;
                piece.item.map_id = map_id;
            }
            self.armor.insert(runtime_id, snapshot);
        }

        let keys = self.equipment.keys().copied().collect::<Vec<_>>();
        self.pending.clear();
        for key in keys {
            let Some(identity) = self
                .equipment
                .get(&key)
                .map(|equipment| equipment.item.identity)
            else {
                continue;
            };
            let mut item = self.resolve_identity(identity);
            item.charged_projectile = self
                .equipment
                .get(&key)
                .and_then(|equipment| equipment.item.charged_projectile.clone());
            item.charged_projectile_role = self.projectile_role(item.charged_projectile.as_deref());
            item.damage = self
                .equipment
                .get(&key)
                .and_then(|equipment| equipment.item.damage);
            item.enchanted = self
                .equipment
                .get(&key)
                .is_some_and(|equipment| equipment.item.enchanted);
            item.map_id = self
                .equipment
                .get(&key)
                .and_then(|equipment| equipment.item.map_id);
            let unresolved = !item.identity.is_empty() && item.identifier.is_none();
            if let Some(equipment) = self.equipment.get_mut(&key) {
                equipment.item = item;
            }
            if unresolved {
                self.retain_pending(key);
            }
        }
        true
    }

    pub(crate) fn get(&self, lifetime: ActorLifetimeId) -> Option<&ActorEquipmentSnapshot> {
        HANDS
            .into_iter()
            .filter_map(|hand| self.get_in_hand(lifetime, hand))
            .max_by_key(|equipment| equipment.event.ingress_sequence)
    }

    pub(crate) fn get_in_hand(
        &self,
        lifetime: ActorLifetimeId,
        hand: ActorHandedness,
    ) -> Option<&ActorEquipmentSnapshot> {
        self.equipment.get(&(lifetime, hand))
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
            block_runtime_id: stack.block_runtime_id,
        };
        let identity = if identity.count == 0 {
            ItemStackIdentity::empty()
        } else if identity.network_id == 0 {
            return None;
        } else {
            identity
        };
        let mut item = self.resolve_identity(identity);
        item.charged_projectile = protocol::item_charged_projectile(&stack.extra_data);
        item.charged_projectile_role = self.projectile_role(item.charged_projectile.as_deref());
        item.damage = protocol::item_stack_damage(stack);
        item.enchanted = protocol::item_has_enchantment_list(&stack.extra_data);
        item.map_id = protocol::item_map_id(&stack.extra_data);
        Some(item)
    }

    /// Reads a retained intrinsic disposition; missing numeric aliases stay unresolved.
    pub(crate) fn item_glint(&self, network_id: i32) -> assets::item::glint::ItemGlint {
        self.registry
            .get(&network_id)
            .map_or(Default::default(), |record| record.glint)
    }

    /// Resolves stack projectile metadata against this registry's admitted identities.
    fn projectile_role(&self, identifier: Option<&str>) -> assets::item_use::ItemProjectileRole {
        identifier
            .and_then(|key| self.projectile_roles.get(key))
            .copied()
            .unwrap_or_default()
    }

    /// The registry identifier for an item network id.
    pub(crate) fn identifier_for_network_id(&self, network_id: i32) -> Option<Arc<str>> {
        self.registry
            .get(&network_id)
            .map(|record| Arc::clone(&record.identifier))
    }

    fn resolve_identity(&self, identity: ItemStackIdentity) -> CanonicalItemStack {
        if identity.is_empty() {
            return CanonicalItemStack {
                use_animation: assets::item_use::ItemUseAnimation::None,
                charged_projectile_role: assets::item_use::ItemProjectileRole::None,
                grip: assets::item::grip::ItemGrip::Unresolved,
                glint: assets::item::glint::ItemGlint::Unresolved,
                identity,
                identifier: None,
                visual: ItemVisualRoute::EmptyHand,
                map_id: None,
                charged_projectile: None,
                damage: None,
                durability: assets::item::durability::ItemDurability::NotDamageable,
                enchanted: false,
            };
        }
        let identifier = self
            .registry
            .get(&identity.network_id)
            .map(|record| Arc::clone(&record.identifier));
        let visual = classify_retained_block(
            identifier
                .as_deref()
                .map_or(ItemVisualRoute::Missing, |identifier| {
                    self.resolve_visual(identifier, identity.metadata)
                }),
            identity.block_runtime_id,
        );
        CanonicalItemStack {
            use_animation: self.item_use_profile(identity.network_id).animation,
            charged_projectile_role: assets::item_use::ItemProjectileRole::None,
            grip: self
                .registry
                .get(&identity.network_id)
                .map_or(Default::default(), |record| record.grip),
            glint: self
                .registry
                .get(&identity.network_id)
                .map_or(Default::default(), |record| record.glint),
            identity,
            identifier,
            visual,
            map_id: None,
            charged_projectile: None,
            damage: None,
            durability: self.item_durability(identity.network_id),
            enchanted: false,
        }
    }

    /// Visual route for an item identifier with no stack context (e.g. the TNT block).
    pub(crate) fn visual_for_identifier(&self, identifier: &str) -> ItemVisualRoute {
        self.resolve_visual(identifier, 0)
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

    fn retain_pending(&mut self, key: EquipmentKey) {
        if self.pending.len() < MAX_PENDING_ITEM_RESOLUTIONS && !self.pending.contains(&key) {
            self.pending.push_back(key);
        }
    }

    fn remove_runtime(&mut self, runtime_id: u64) {
        self.equipment
            .retain(|(lifetime, _), _| lifetime.runtime_id != runtime_id);
        self.pending
            .retain(|(lifetime, _)| lifetime.runtime_id != runtime_id);
        if self.persistent_armor_runtime != Some(runtime_id) {
            self.armor.remove(&runtime_id);
        }
    }
}

/// Admits retail fallback once, independently of the server registry's size and ID span.
fn retail_animation_facts() -> &'static inventory::SessionItemFacts {
    static FACTS: OnceLock<inventory::SessionItemFacts> = OnceLock::new();
    FACTS.get_or_init(|| {
        inventory::SessionItemFacts::from_registry(
            assets::facts::ItemFacts::builtin().expect("valid pinned item facts"),
            &ItemRegistryEvent {
                entries: protocol::vanilla_item_registry(),
            },
        )
        .expect("valid pinned retail item registry")
    })
}

/// Admits the retail registry through the same compiled facts as server entries.
fn built_in_registry(
    assets: Option<&RuntimeEntityAssets>,
) -> BTreeMap<i32, CanonicalItemRegistryRecord> {
    protocol::vanilla_item_registry()
        .iter()
        .map(|entry| (entry.network_id, registry_record(entry, assets)))
        .collect()
}

/// Keeps compiled item visuals authoritative, including sprites for items that place blocks.
/// A retained block identity supplies geometry only when no compiled visual resolves.
fn classify_retained_block(route: ItemVisualRoute, block_runtime_id: i32) -> ItemVisualRoute {
    if block_runtime_id == 0
        || matches!(
            route,
            ItemVisualRoute::Compiled(_) | ItemVisualRoute::BlockItem(_)
        )
    {
        return route;
    }
    ItemVisualRoute::RetainedBlock { block_runtime_id }
}

#[cfg(test)]
mod visual_route_tests {
    use super::*;

    #[test]
    fn sprite_items_keep_their_visual_when_the_stack_carries_a_placed_block() {
        let sprite = ItemVisualRoute::Compiled(ItemVisualId(7));
        for block_runtime_id in [1, i32::MIN] {
            assert_eq!(classify_retained_block(sprite, block_runtime_id), sprite);
        }
        let block = ItemVisualRoute::BlockItem(BlockVisualId(3));
        assert_eq!(classify_retained_block(block, 1), block);
        assert_eq!(
            classify_retained_block(ItemVisualRoute::Missing, i32::MIN),
            ItemVisualRoute::RetainedBlock {
                block_runtime_id: i32::MIN,
            }
        );
    }
}

/// Resolves item-owned facts once for this registry generation.
fn registry_record(
    entry: &ItemRegistryEntry,
    assets: Option<&RuntimeEntityAssets>,
) -> CanonicalItemRegistryRecord {
    CanonicalItemRegistryRecord {
        grip: grip::resolve(entry, assets),
        glint: glint::resolve(entry, assets),
        identifier: Arc::clone(&entry.identifier),
        network_id: entry.network_id,
        component_based: entry.component_based,
        version: entry.version,
        component_digest: entry.component_digest,
        components: entry.components.clone(),
        armour: armour::resolve(entry, assets),
        durability: resolve_item_durability(entry, assets),
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

#[cfg(test)]
mod armor_tests {
    use super::*;

    fn lifetime(runtime_id: u64, spawn_revision: u64) -> ActorLifetimeId {
        ActorLifetimeId {
            session_id: 1,
            dimension: 0,
            runtime_id,
            spawn_revision,
        }
    }

    fn stack(network_id: i32, extra: &[u8]) -> NetworkItemStack {
        NetworkItemStack {
            network_id,
            metadata: 0,
            stack_network_id: -1,
            count: 1,
            nbt_digest: Sha256::digest(extra).into(),
            block_runtime_id: 0,
            extra_data: Arc::from(extra),
        }
    }

    fn dyed_extra() -> Vec<u8> {
        let mut encoded = vec![0xff, 0xff, 0x01, 0x0a, 0x00, 0x00, 0x03];
        encoded.extend_from_slice(&11u16.to_le_bytes());
        encoded.extend_from_slice(b"customColor");
        encoded.extend_from_slice(&0x0033_66ccu32.to_le_bytes());
        encoded.push(0x00);
        encoded
    }

    fn event(runtime_id: u64, helmet: NetworkItemStack) -> ArmorEquipmentEvent {
        ArmorEquipmentEvent {
            actor_runtime_id: runtime_id,
            helmet,
            chestplate: NetworkItemStack::empty(),
            leggings: NetworkItemStack::empty(),
            boots: NetworkItemStack::empty(),
            body: NetworkItemStack::empty(),
        }
    }

    #[test]
    fn armor_keeps_dye_and_drops_with_the_actor_unless_persistent() {
        let mut store = ItemStateStore::diagnostic();
        let extra = dyed_extra();
        assert!(store.apply_armor(lifetime(7, 1), 1, &event(7, stack(1, &extra))));
        assert!(store.apply_armor(lifetime(8, 1), 2, &event(8, stack(1, &extra))));
        assert_eq!(store.armor(7).unwrap().helmet.dye_rgb, Some(0x0033_66cc));
        assert!(store.armor(7).unwrap().chestplate.item.identity.is_empty());

        store.set_persistent_armor_runtime(8);
        store.remove(lifetime(7, 1));
        store.remove(lifetime(8, 1));
        assert!(store.armor(7).is_none());
        assert!(store.armor(8).is_some());
        store.clear_actor_state();
        assert!(store.armor(8).is_some());
    }

    #[test]
    fn canonical_stack_keeps_the_charged_crossbow_projectile() {
        let mut extra = vec![0xff, 0xff, 0x01, 0x0a, 0x00, 0x00, 0x0a];
        extra.extend_from_slice(&11u16.to_le_bytes());
        extra.extend_from_slice(b"chargedItem");
        extra.push(0x08);
        extra.extend_from_slice(&4u16.to_le_bytes());
        extra.extend_from_slice(b"Name");
        extra.extend_from_slice(&15u16.to_le_bytes());
        extra.extend_from_slice(b"minecraft:arrow");
        extra.extend_from_slice(&[0x00, 0x00]);
        let store = ItemStateStore::diagnostic();
        let charged = store.canonicalize(&stack(1, &extra)).unwrap();
        assert_eq!(
            charged.charged_projectile.as_deref(),
            Some("minecraft:arrow")
        );
        let plain = store.canonicalize(&stack(1, &dyed_extra())).unwrap();
        assert_eq!(plain.charged_projectile, None);
    }

    #[test]
    fn canonical_armor_retains_enchantment_glint() {
        let mut extra = vec![0xff, 0xff, 0x01, 0x0a, 0x00, 0x00, 0x09];
        extra.extend_from_slice(&4u16.to_le_bytes());
        extra.extend_from_slice(b"ench");
        extra.extend_from_slice(&[0x0a, 0, 0, 0, 0, 0]);
        let store = ItemStateStore::diagnostic();
        assert!(store.canonicalize(&stack(1, &extra)).unwrap().enchanted);
        assert!(
            !store
                .canonicalize(&stack(1, &dyed_extra()))
                .unwrap()
                .enchanted
        );
    }

    #[test]
    fn registry_refresh_preserves_held_and_worn_damage_and_glint() {
        let mut extra = vec![0xff, 0xff, 0x01, 0x0a, 0x00, 0x00, 0x09];
        extra.extend_from_slice(&4u16.to_le_bytes());
        extra.extend_from_slice(b"ench");
        extra.extend_from_slice(&[0x0a, 0, 0, 0, 0, 0x03]);
        extra.extend_from_slice(&6u16.to_le_bytes());
        extra.extend_from_slice(b"Damage");
        extra.extend_from_slice(&7u32.to_le_bytes());
        extra.push(0);
        let actor = lifetime(7, 1);
        let mut store = ItemStateStore::diagnostic();
        store.insert_spawn(actor, 1, stack(1, &extra));
        assert!(store.apply_armor(actor, 2, &event(7, stack(1, &extra))));
        assert!(store.apply_registry(ItemRegistryEvent {
            entries: Arc::from([])
        }));
        for item in [
            &store.get(actor).unwrap().item,
            &store.armor(7).unwrap().helmet.item,
        ] {
            assert_eq!(item.damage, Some(7));
            assert!(
                item.enchanted,
                "registry resolution must retain the stack's enchantments"
            );
        }
    }

    #[test]
    fn registry_retains_every_numeric_alias_for_a_shared_item_identifier() {
        let entries = [101, 102, 103].map(|network_id| protocol::ItemRegistryEntry {
            identifier: Arc::from("example:menu_icon"),
            network_id,
            component_based: true,
            version: protocol::ItemRegistryVersion::DataDriven,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        });
        let mut store = ItemStateStore::diagnostic();
        assert!(store.apply_registry(ItemRegistryEvent {
            entries: Arc::from(entries)
        }));
        for network_id in [101, 102, 103] {
            let item = store.canonicalize(&stack(network_id, &[])).unwrap();
            assert_eq!(item.identifier.as_deref(), Some("example:menu_icon"));
        }
    }

    #[test]
    fn armor_with_a_wrong_nbt_digest_is_rejected_whole() {
        let mut store = ItemStateStore::diagnostic();
        let mut bad = stack(1, &dyed_extra());
        bad.nbt_digest = [9; 32];
        assert!(!store.apply_armor(lifetime(7, 1), 1, &event(7, bad)));
        assert!(store.armor(7).is_none());
    }
}

#[cfg(test)]
mod equip_identity_tests {
    use super::*;

    fn vanilla_network_id(identifier: &str) -> i32 {
        protocol::vanilla_item_registry()
            .iter()
            .find(|item| item.identifier.as_ref() == identifier)
            .unwrap()
            .network_id
    }

    /// The held identity animation compares ignores count and reads durability from stack tags.
    #[test]
    fn canonical_stacks_drop_count_and_read_tag_damage() {
        let store = ItemStateStore::diagnostic();
        let mut stack = NetworkItemStack {
            network_id: vanilla_network_id("minecraft:potion"),
            count: 2,
            metadata: 21,
            ..Default::default()
        };
        let before = store.canonicalize(&stack).unwrap();
        stack.count = 1;
        let after = store.canonicalize(&stack).unwrap();
        assert_eq!(before.identifier, after.identifier);
        assert_eq!(before.identity.metadata, after.identity.metadata);

        let network_id = vanilla_network_id("minecraft:iron_sword");
        for damage in [0_u32, 10] {
            let mut extra = vec![0xff, 0xff, 1, 10, 0, 0, 3, 6, 0];
            extra.extend_from_slice(b"Damage");
            extra.extend_from_slice(&damage.to_le_bytes());
            extra.push(0);
            let item = store
                .canonicalize(&NetworkItemStack {
                    network_id,
                    count: 1,
                    nbt_digest: Sha256::digest(&extra).into(),
                    extra_data: extra.into(),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(item.damage, Some(damage));
        }
    }
}
