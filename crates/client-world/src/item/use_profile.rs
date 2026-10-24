//! Numerical item-use admission shared by gameplay, presentation and consume audio.

use super::*;
use assets::item_use::{
    DEFAULT_USE_MOVEMENT_MODIFIER, ItemConsumption, ItemProjectileRole, ItemUseDurationRule,
    ItemUseKind, ItemUseProfile, ItemUseRequirement,
};

/// A category identity belongs to one effective registry admission, never a leaked string.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UseCooldownCategory {
    pub generation: u64,
    pub index: u16,
}

/// A resolved category timer is small enough to copy into the accepted gameplay frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedUseCooldown {
    pub category: UseCooldownCategory,
    pub ticks: u32,
}

/// Effective facts cached by numerical network ID; unresolved fields grant no prediction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectiveItemUse {
    pub animation: assets::item_use::ItemUseAnimation,
    pub kind: Option<ItemUseKind>,
    pub ticks: Option<u32>,
    pub requirement: ItemUseRequirement,
    pub consumption: ItemConsumption,
    pub movement: f64,
    pub cooldown: Option<ResolvedUseCooldown>,
    pub projectile: ItemProjectileRole,
}

impl Default for EffectiveItemUse {
    /// Keeps unsupported items distinct from declared generic hold or consumption behavior.
    fn default() -> Self {
        Self {
            animation: assets::item_use::ItemUseAnimation::None,
            kind: None,
            ticks: None,
            requirement: ItemUseRequirement::Nothing,
            consumption: ItemConsumption::Unresolved,
            movement: 1.0,
            cooldown: None,
            projectile: ItemProjectileRole::None,
        }
    }
}

/// Resolves immutable rules outside gameplay; stack-dependent crossbow timing remains separate.
pub fn resolve_item_use(
    profile: &ItemUseProfile,
    declared_ticks: Option<u32>,
    category: Option<UseCooldownCategory>,
) -> EffectiveItemUse {
    let ItemUseProfile::Provisional {
        animation,
        kind,
        duration,
        requirement,
        consumption,
        movement_bits,
        cooldown,
        projectile,
        ..
    } = profile
    else {
        return EffectiveItemUse {
            ticks: declared_ticks,
            ..Default::default()
        };
    };
    EffectiveItemUse {
        animation: *animation,
        kind: Some(*kind),
        ticks: match duration {
            ItemUseDurationRule::Declared => declared_ticks,
            ItemUseDurationRule::LongWeapon => Some(assets::item_use::LONG_WEAPON_USE_TICKS),
            ItemUseDurationRule::Fixed(ticks) => Some(*ticks),
            ItemUseDurationRule::Crossbow => Some(assets::item_use::CROSSBOW_CHARGE_TICKS),
        },
        requirement: *requirement,
        consumption: *consumption,
        movement: f64::from_bits(*movement_bits),
        cooldown: cooldown
            .as_ref()
            .zip(category)
            .map(|(cooldown, category)| ResolvedUseCooldown {
                category,
                ticks: cooldown.ticks,
            }),
        projectile: *projectile,
    }
}

impl ItemStateStore {
    /// Returns a compiled native throwable identity admitted without a component payload override.
    pub(crate) fn native_throwable_identifier(&self, network_id: i32) -> Option<&str> {
        let record = self.registry.get(&network_id)?;
        (record.unmodified_throwable
            && self.item_use_profile(network_id).kind == Some(assets::item_use::ItemUseKind::Throw))
        .then_some(record.identifier.as_ref())
    }

    /// Rebuilds numerical profiles and owned category admissions only when source facts change.
    pub(super) fn rebuild_use_profiles(&mut self) {
        self.presentation_generation = Arc::new(());
        // Category generations prevent a previous registry timer from blocking new definitions.
        static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let generation = NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut categories = BTreeMap::<Box<str>, u16>::new();
        let mut profiles = BTreeMap::new();
        for entry in self.registry.values() {
            let mut profile = self.assets.as_ref().map_or_else(
                || assets::item_use::diagnostic_profile(&entry.identifier),
                |assets| assets.item_use_profile(&entry.identifier),
            );
            let mut ticks = self
                .use_durations
                .as_ref()
                .and_then(|durations| durations.get(entry.identifier.as_ref()).copied());
            if let ItemUseProfile::Provisional {
                duration_alias: Some(alias),
                ..
            } = &profile
            {
                ticks = ticks.or_else(|| self.use_durations.as_ref()?.get(alias.as_ref()).copied());
            }
            if let Some(components) = entry.components.as_deref() {
                admit_shooter(&mut profile, components);
            }
            if let Some(overlay) = self
                .use_overlays
                .as_ref()
                .and_then(|overlays| overlays.get(entry.identifier.as_ref()))
            {
                ticks = overlay.duration_ticks.or(ticks);
                overlay_authored(&mut profile, overlay);
            }
            if let Some(components) = entry.components.as_deref() {
                ticks = components.use_duration_ticks.or(ticks);
                overlay_components(&mut profile, components);
            }
            let category = match &profile {
                ItemUseProfile::Provisional {
                    cooldown: Some(cooldown),
                    ..
                } => {
                    let next =
                        u16::try_from(categories.len()).expect("registry bounds category count");
                    let index = *categories.entry(cooldown.category.clone()).or_insert(next);
                    Some(UseCooldownCategory { generation, index })
                }
                _ => None,
            };
            profiles.insert(
                entry.network_id,
                resolve_item_use(&profile, ticks, category),
            );
        }
        self.projectile_roles = self
            .registry
            .values()
            .filter_map(|entry| {
                let role = profiles.get(&entry.network_id)?.projectile;
                (role != ItemProjectileRole::None).then(|| (Arc::clone(&entry.identifier), role))
            })
            .collect();
        self.use_profiles = profiles;
    }

    /// Reads the selected hand's numerical admission; an explicit local zero means empty.
    pub(crate) fn held_use_profile(
        &self,
        actor: ActorLifetimeId,
        hand: ActorHandedness,
        local_network_id: Option<i32>,
    ) -> EffectiveItemUse {
        let network_id = local_network_id
            .or_else(|| {
                self.get_in_hand(actor, hand)
                    .map(|held| held.item.identity.network_id)
            })
            .unwrap_or(0);
        self.item_use_profile(network_id)
    }

    /// Retrieves admitted facts without resolving names or parsing component data.
    pub(crate) fn item_use_profile(&self, network_id: i32) -> EffectiveItemUse {
        self.use_profiles
            .get(&network_id)
            .copied()
            .unwrap_or_default()
    }
}

/// Admits a checked native use profile only when the negotiated payload is canonically empty.
pub(super) fn unmodified_throwable(
    entry: &protocol::ItemRegistryEntry,
    assets: Option<&assets::RuntimeEntityAssets>,
) -> bool {
    !matches!(entry.version, protocol::ItemRegistryVersion::Unknown(_))
        && entry.canonical_empty_component_data
        && assets.is_some_and(|assets| {
            matches!(
                assets.item_use_profile(&entry.identifier),
                ItemUseProfile::Provisional {
                    kind: ItemUseKind::Throw,
                    ..
                }
            )
        })
}

/// Admits a drawing shooter as a held use that its release ends, as a bow does. It runs before
/// any overlay so authored and wire facts refine the admitted use instead of being dropped.
fn admit_shooter(profile: &mut ItemUseProfile, components: &protocol::ItemComponents) {
    if matches!(profile, ItemUseProfile::Unresolved)
        && components
            .shooter
            .is_some_and(protocol::ItemShooter::starts_held_draw)
    {
        *profile = ItemUseProfile::Provisional {
            animation: assets::item_use::ItemUseAnimation::None,
            kind: ItemUseKind::Hold,
            duration: ItemUseDurationRule::Declared,
            requirement: ItemUseRequirement::Nothing,
            consumption: ItemConsumption::None,
            movement_bits: DEFAULT_USE_MOVEMENT_MODIFIER.to_bits(),
            cooldown: None,
            projectile: ItemProjectileRole::None,
            duration_alias: None,
        };
    }
}

/// Converts parsed wire evidence once before applying the common authored overlay.
fn overlay_components(profile: &mut ItemUseProfile, components: &protocol::ItemComponents) {
    let overlay = assets::equipment::CompiledItemUseOverlay {
        identifier: Box::from(""),
        duration_ticks: components.use_duration_ticks,
        food_can_always_eat: components.food_can_always_eat,
        consumption: components
            .use_animation
            .as_deref()
            .map(|animation| match animation {
                "eat" => ItemConsumption::Eat,
                "drink" => ItemConsumption::Drink,
                _ => ItemConsumption::None,
            })
            .or_else(|| components.food.then_some(ItemConsumption::Eat)),
        use_modifiers_present: components.use_modifiers_present,
        movement_modifier_bits: components.use_movement_modifier_bits,
        cooldown: components.use_cooldown.as_ref().map(|cooldown| {
            assets::item_use::ItemUseCategory {
                category: cooldown.category.as_ref().into(),
                ticks: cooldown.ticks,
            }
        }),
    };
    overlay_authored(profile, &overlay);
}

/// Applies explicit fields while duration alone leaves consumption unresolved.
fn overlay_authored(
    profile: &mut ItemUseProfile,
    overlay: &assets::equipment::CompiledItemUseOverlay,
) {
    if overlay.food_can_always_eat.is_some() && matches!(profile, ItemUseProfile::Unresolved) {
        *profile = ItemUseProfile::Provisional {
            animation: assets::item_use::ItemUseAnimation::None,
            kind: ItemUseKind::Hold,
            duration: ItemUseDurationRule::Declared,
            requirement: ItemUseRequirement::Nothing,
            consumption: ItemConsumption::Unresolved,
            movement_bits: DEFAULT_USE_MOVEMENT_MODIFIER.to_bits(),
            cooldown: None,
            projectile: ItemProjectileRole::None,
            duration_alias: None,
        };
    }
    let ItemUseProfile::Provisional {
        duration,
        requirement,
        consumption,
        movement_bits,
        cooldown,
        ..
    } = profile
    else {
        return;
    };
    if overlay.duration_ticks.is_some() {
        *duration = ItemUseDurationRule::Declared;
    }
    if let Some(always) = overlay.food_can_always_eat {
        *requirement = if always {
            ItemUseRequirement::Nothing
        } else {
            ItemUseRequirement::Appetite
        };
    }
    if let Some(authored) = overlay.consumption {
        *consumption = authored;
    }
    if let Some(bits) = overlay.movement_modifier_bits {
        *movement_bits = bits;
    } else if overlay.use_modifiers_present {
        *movement_bits = 1.0_f64.to_bits();
    }
    if let Some(authored) = &overlay.cooldown {
        *cooldown = Some(authored.clone());
    }
}

impl ItemStateStore {
    /// Installs immutable compiled authored facts and rebuilds only after a source replacement.
    pub(crate) fn set_use_overlays(
        &mut self,
        overlays: Arc<BTreeMap<Box<str>, assets::equipment::CompiledItemUseOverlay>>,
    ) {
        if !self
            .use_overlays
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &overlays))
        {
            self.use_overlays = Some(overlays);
            self.rebuild_use_profiles();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds original network admissions with owned component evidence.
    fn entry(
        id: i32,
        name: &str,
        components: protocol::ItemComponents,
    ) -> protocol::ItemRegistryEntry {
        protocol::ItemRegistryEntry {
            identifier: name.into(),
            network_id: id,
            component_based: true,
            version: protocol::ItemRegistryVersion::DataDriven,
            component_digest: [0; 32],
            components: Some(Arc::new(components)),
            negotiated_max_stack_size: None,
            canonical_empty_component_data: false,
            item_tags: Arc::from([]),
        }
    }

    #[test]
    fn animation_facts_follow_numeric_registry_replacements_and_retail_fallback() {
        use assets::facts::ItemTraits;
        let entries = protocol::vanilla_item_registry();
        let sword = entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == "minecraft:iron_sword")
            .unwrap();
        let mut spear = entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == "minecraft:wooden_spear")
            .unwrap()
            .clone();
        spear.network_id = sword.network_id;
        let mut store = ItemStateStore::diagnostic();
        assert!(
            store
                .animation_fact(sword.network_id)
                .traits
                .contains(ItemTraits::SWORD)
        );
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([spear]),
        }));
        let replacement = store.animation_fact(sword.network_id);
        assert!(replacement.traits.contains(ItemTraits::SPEAR));
        assert!(!replacement.traits.contains(ItemTraits::SWORD));
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([]),
        }));
        assert!(
            store
                .animation_fact(sword.network_id)
                .traits
                .contains(ItemTraits::SWORD)
        );
    }

    #[test]
    fn numeric_animation_admission_clears_when_the_registry_identity_changes() {
        let mut store = ItemStateStore::diagnostic();
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([entry(15000, "minecraft:bow", Default::default())]),
        }));
        assert_eq!(
            store
                .resolve_identity(assets::ItemStackIdentity {
                    network_id: 15000,
                    count: 1,
                    ..assets::ItemStackIdentity::empty()
                })
                .use_animation,
            assets::item_use::ItemUseAnimation::Bow
        );
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([entry(15000, "fixture:display", Default::default())]),
        }));
        assert_eq!(
            store
                .resolve_identity(assets::ItemStackIdentity {
                    network_id: 15000,
                    count: 1,
                    ..assets::ItemStackIdentity::empty()
                })
                .use_animation,
            assets::item_use::ItemUseAnimation::None
        );
        assert_eq!(
            store.projectile_role(Some("fixture:unadmitted")),
            ItemProjectileRole::None
        );
    }

    #[test]
    fn animation_duration_uses_numeric_admission_for_both_hands_and_replacements() {
        let mut store = ItemStateStore::diagnostic();
        let actor = ActorLifetimeId {
            session_id: 1,
            dimension: 0,
            runtime_id: 7,
            spawn_revision: 1,
        };
        let component = |ticks| protocol::ItemComponents {
            use_duration_ticks: Some(ticks),
            use_animation: Some("drink".into()),
            food: true,
            food_can_always_eat: Some(true),
            ..Default::default()
        };
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([
                entry(15000, "fixture:main", component(17)),
                entry(15001, "fixture:off", component(23))
            ])
        }));
        for (hand, network_id, ticks) in [
            (ActorHandedness::Right, 15000, 17),
            (ActorHandedness::Left, 15001, 23),
        ] {
            let extra = Arc::<[u8]>::from([]);
            let stack = NetworkItemStack {
                network_id,
                metadata: 0,
                stack_network_id: -1,
                count: 1,
                nbt_digest: Sha256::digest(&extra).into(),
                block_runtime_id: 0,
                extra_data: extra,
            };
            assert!(store.apply_equipment(
                actor,
                1,
                EquipmentEvent {
                    actor_runtime_id: actor.runtime_id,
                    stack,
                    inventory_slot: 0,
                    selected_slot: 0,
                    window_id: 0,
                    handedness: Some(hand),
                }
            ));
            let remote = store.held_use_profile(actor, hand, None);
            assert_eq!(remote.ticks, Some(ticks));
            assert_eq!(remote.consumption, ItemConsumption::Drink);
            assert_eq!(
                store.held_use_profile(actor, hand, Some(network_id)),
                remote
            );
            assert_eq!(
                store.held_use_profile(actor, hand, Some(0)),
                EffectiveItemUse::default()
            );
        }
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([entry(15000, "fixture:renamed", component(31))])
        }));
        assert_eq!(
            store
                .held_use_profile(actor, ActorHandedness::Right, None)
                .ticks,
            Some(31)
        );
        assert_eq!(
            store
                .held_use_profile(actor, ActorHandedness::Right, Some(15000))
                .ticks,
            Some(31)
        );
        assert_eq!(
            store.held_use_profile(actor, ActorHandedness::Left, None),
            EffectiveItemUse::default()
        );
    }

    #[test]
    fn declared_duration_does_not_admit_use_kind_food_or_movement() {
        let mut store = ItemStateStore::diagnostic();
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([entry(
                15000,
                "fixture:hold",
                protocol::ItemComponents {
                    use_duration_ticks: Some(32),
                    ..Default::default()
                }
            )])
        }));
        let profile = store.item_use_profile(15000);
        assert_eq!(profile.ticks, Some(32));
        assert_eq!(profile.kind, None);
        assert_eq!(profile.movement, 1.0);
        assert_eq!(profile.requirement, ItemUseRequirement::Nothing);
        assert_eq!(profile.consumption, ItemConsumption::Unresolved);
        assert_eq!(store.item_use_profile(15001).kind, None);
    }

    #[test]
    fn drawing_shooter_with_a_duration_is_a_held_use() {
        let shooter = |shooter: protocol::ItemShooter| protocol::ItemComponents {
            use_duration_ticks: Some(2_000_000_000),
            use_modifiers_present: true,
            use_movement_modifier_bits: Some(0.65_f64.to_bits()),
            shooter: Some(shooter),
            ..Default::default()
        };
        let mut store = ItemStateStore::diagnostic();
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([
                entry(
                    15000,
                    "fixture:blaster",
                    shooter(protocol::ItemShooter {
                        draws: true,
                        ..Default::default()
                    }),
                ),
                entry(15001, "fixture:instant", shooter(Default::default())),
            ]),
        }));
        let profile = store.item_use_profile(15000);
        assert_eq!(profile.kind, Some(ItemUseKind::Hold));
        assert_eq!(profile.ticks, Some(2_000_000_000));
        assert_eq!(profile.requirement, ItemUseRequirement::Nothing);
        assert_eq!(profile.movement, 0.65);
        assert_eq!(store.item_use_profile(15001).kind, None);
    }

    #[test]
    fn authored_shooter_slowdown_and_cooldown_survive_admission_under_wire_facts() {
        let blaster = |movement: Option<f64>| protocol::ItemComponents {
            use_duration_ticks: Some(2_000_000_000),
            use_modifiers_present: movement.is_some(),
            use_movement_modifier_bits: movement.map(f64::to_bits),
            shooter: Some(protocol::ItemShooter {
                draws: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut store = ItemStateStore::diagnostic();
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([
                entry(15000, "fixture:authored_blaster", blaster(None)),
                entry(15001, "fixture:wire_blaster", blaster(Some(0.65))),
            ]),
        }));
        let authored = |identifier: &str| {
            (
                Box::from(identifier),
                assets::equipment::CompiledItemUseOverlay {
                    identifier: Box::from(identifier),
                    duration_ticks: None,
                    food_can_always_eat: None,
                    consumption: None,
                    use_modifiers_present: true,
                    movement_modifier_bits: Some(0.4_f64.to_bits()),
                    cooldown: Some(assets::item_use::ItemUseCategory {
                        category: "fixture:blasters".into(),
                        ticks: 6,
                    }),
                },
            )
        };
        store.set_use_overlays(Arc::new(BTreeMap::from([
            authored("fixture:authored_blaster"),
            authored("fixture:wire_blaster"),
        ])));
        let authored = store.item_use_profile(15000);
        assert_eq!(authored.kind, Some(ItemUseKind::Hold));
        assert_eq!(authored.movement, 0.4);
        assert_eq!(authored.cooldown.map(|cooldown| cooldown.ticks), Some(6));
        let wire = store.item_use_profile(15001);
        assert_eq!(wire.kind, Some(ItemUseKind::Hold));
        assert_eq!(wire.movement, 0.65);
        assert_eq!(wire.cooldown.map(|cooldown| cooldown.ticks), Some(6));
    }

    #[test]
    fn custom_food_without_use_modifiers_keeps_the_default_slowdown() {
        let mut store = ItemStateStore::diagnostic();
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([entry(
                15000,
                "fixture:custom_food",
                protocol::ItemComponents {
                    food: true,
                    food_can_always_eat: Some(true),
                    use_duration_ticks: Some(32),
                    ..Default::default()
                },
            )]),
        }));
        let profile = store.item_use_profile(15000);
        assert_eq!(profile.kind, Some(ItemUseKind::Hold));
        assert_eq!(profile.consumption, ItemConsumption::Eat);
        assert_eq!(profile.ticks, Some(32));
        assert_eq!(profile.movement, DEFAULT_USE_MOVEMENT_MODIFIER);
    }

    #[test]
    fn a_present_use_modifiers_component_supplies_its_neutral_default() {
        let mut store = ItemStateStore::diagnostic();
        for (network_id, identifier) in [
            (15000, "fixture:custom_food"),
            (15001, "minecraft:golden_apple"),
        ] {
            assert!(store.apply_registry(protocol::ItemRegistryEvent {
                entries: Arc::from([entry(
                    network_id,
                    identifier,
                    protocol::ItemComponents {
                        food: true,
                        food_can_always_eat: Some(true),
                        use_duration_ticks: Some(32),
                        use_modifiers_present: true,
                        ..Default::default()
                    },
                )]),
            }));
            let profile = store.item_use_profile(network_id);
            assert_eq!(profile.kind, Some(ItemUseKind::Hold));
            assert_eq!(profile.movement, 1.0);
        }
    }

    #[test]
    fn authored_food_uses_the_same_component_presence_defaults_as_wire_food() {
        for (present, modifier, expected) in [
            (false, None, DEFAULT_USE_MOVEMENT_MODIFIER),
            (true, None, 1.0),
            (true, Some(0.0_f64.to_bits()), 0.0),
            (true, Some(0.75_f64.to_bits()), 0.75),
        ] {
            let mut store = ItemStateStore::diagnostic();
            assert!(store.apply_registry(protocol::ItemRegistryEvent {
                entries: Arc::from([entry(15000, "fixture:authored_food", Default::default())]),
            }));
            store.set_use_overlays(Arc::new(BTreeMap::from([(
                Box::from("fixture:authored_food"),
                assets::equipment::CompiledItemUseOverlay {
                    identifier: Box::from("fixture:authored_food"),
                    duration_ticks: Some(32),
                    food_can_always_eat: Some(true),
                    consumption: Some(ItemConsumption::Eat),
                    use_modifiers_present: present,
                    movement_modifier_bits: modifier,
                    cooldown: None,
                },
            )])));
            let profile = store.item_use_profile(15000);
            assert_eq!(profile.kind, Some(ItemUseKind::Hold));
            assert_eq!(profile.movement, expected);
        }
    }

    #[test]
    fn food_movement_and_owned_shared_categories_are_resolved_at_admission() {
        let mut store = ItemStateStore::diagnostic();
        let components = protocol::ItemComponents {
            food: true,
            food_can_always_eat: Some(true),
            use_animation: Some("drink".into()),
            use_duration_ticks: Some(12),
            use_movement_modifier_bits: Some(0.75_f64.to_bits()),
            use_cooldown: Some(protocol::ItemUseCooldown {
                category: "fixture:shared".into(),
                ticks: 9,
            }),
            ..Default::default()
        };
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([
                entry(15000, "fixture:one", components.clone()),
                entry(15001, "fixture:two", components.clone()),
            ])
        }));
        let first = store.item_use_profile(15000);
        assert_eq!(first.consumption, ItemConsumption::Drink);
        assert_eq!(first.requirement, ItemUseRequirement::Nothing);
        assert_eq!(first.movement, 0.75);
        assert_eq!(first.cooldown, store.item_use_profile(15001).cooldown);
        let old_category = first.cooldown.unwrap().category;
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([entry(
                15000,
                "fixture:replacement",
                protocol::ItemComponents {
                    food: true,
                    food_can_always_eat: Some(false),
                    use_duration_ticks: Some(4),
                    ..components
                }
            )])
        }));
        let replacement = store.item_use_profile(15000);
        assert_eq!(replacement.requirement, ItemUseRequirement::Appetite);
        assert_eq!(replacement.ticks, Some(4));
        assert_ne!(replacement.cooldown.unwrap().category, old_category);
        assert_eq!(store.item_use_profile(15001).kind, None);
    }

    #[test]
    fn rejected_registry_replacement_preserves_all_admitted_profiles() {
        let mut store = ItemStateStore::diagnostic();
        let entry = entry(
            15000,
            "fixture:original",
            protocol::ItemComponents {
                use_duration_ticks: Some(7),
                ..Default::default()
            },
        );
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([entry.clone()])
        }));
        let profile = store.item_use_profile(15000);
        assert!(!store.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([entry.clone(), entry])
        }));
        assert_eq!(store.item_use_profile(15000), profile);
    }
}
