use super::*;

#[test]
fn facts_migration_unique_native_classes_resolve_their_asset_identity() {
    for (class, expected) in [
        (NativeEntityClass::Player, "minecraft:player"),
        (NativeEntityClass::Tnt, "minecraft:tnt"),
        (
            NativeEntityClass::FireworksRocket,
            "minecraft:fireworks_rocket",
        ),
    ] {
        assert_eq!(class.identifier(), Some(expected));
    }
    assert_eq!(NativeEntityClass::Ordinary.identifier(), None);
    assert_eq!(NativeEntityClass::Unresolved.identifier(), None);
    for record in &EntityFacts::pinned().records {
        assert_eq!(
            record.fact.native_class == NativeEntityClass::AreaEffectCloud,
            record.identifier.as_ref() == "minecraft:area_effect_cloud",
            "{}",
            record.identifier,
        );
    }
}

#[test]
fn pinned_facts_preserve_classes_components_and_equipment() {
    let facts = EntityFacts::pinned();
    for actor in include_str!("../../../../protocol/data/retail_actors_1_26_50.txt").lines() {
        assert_ne!(facts.get(actor).class_key, 0, "missing actor {actor}");
    }
    assert_eq!(
        facts
            .get("minecraft:spider")
            .shadow
            .radius(2.0, false, 0, false),
        1.4
    );
    assert!(facts.get("minecraft:arrow").projectile);
    for name in [
        "minecraft:boat",
        "minecraft:chest_boat",
        "minecraft:shulker",
    ] {
        assert!(facts.get(name).collidable, "{name}");
    }
    assert!(!facts.get("minecraft:bee").collidable);
    assert!(!facts.get("minecraft:happy_ghast").collidable);
    assert_eq!(
        facts.get("minecraft:boat").native_class,
        NativeEntityClass::Boat
    );
    assert!(
        facts
            .get("minecraft:horse")
            .equipment
            .slots()
            .unwrap()
            .saddle
    );
    assert_eq!(
        facts.get("minecraft:splash_potion").native_class,
        NativeEntityClass::SplashPotion
    );
    assert_eq!(
        facts.get("minecraft:wolf").interactions.aim_assist(),
        Some(true)
    );
    assert_eq!(
        facts.get("minecraft:painting").native_class,
        NativeEntityClass::Painting
    );
    assert_eq!(facts.get("fixture:unknown"), EntityFact::default());
    assert_eq!(facts.get("minecraft:shulker").pick_radius, 0.0);
    assert_eq!(facts.get("minecraft:fireball").pick_radius, 1.0);
    assert_eq!(facts.player().pick_radius, DEFAULT_ACTOR_PICK_RADIUS);
}

#[test]
fn corrupt_stale_or_trailing_carriers_are_rejected() {
    let protocol = crate::active_content_registry_protocol();
    assert!(EntityFacts::decode(SOURCE_BYTES, protocol + 1).is_err());
    for index in [0, MAGIC.len(), SOURCE_BYTES.len() - 1] {
        let mut bytes = SOURCE_BYTES.to_vec();
        bytes[index] ^= 1;
        assert!(EntityFacts::decode(&bytes, protocol).is_err());
    }
    let mut trailing = SOURCE_BYTES.to_vec();
    trailing.push(0);
    assert!(EntityFacts::decode(&trailing, protocol).is_err());
    let mut forged = EntityFacts::decode(SOURCE_BYTES, protocol).unwrap();
    forged.records.swap(0, 1);
    assert!(forged.encode().is_err());
    forged.records.swap(0, 1);
    forged.records[0].fact.shadow = ShadowRadius::Width(-1.0);
    assert!(forged.encode().is_err());
}

#[test]
fn entity_pick_radius_must_be_finite_and_nonnegative() {
    for radius in [-0.1, f32::INFINITY, f32::NAN] {
        let mut facts =
            EntityFacts::decode(SOURCE_BYTES, crate::active_content_registry_protocol()).unwrap();
        facts.records[0].fact.pick_radius = radius;
        assert!(facts.encode().is_err(), "radius {radius}");
    }
}

#[test]
fn a_valid_carrier_with_different_facts_cannot_replace_the_startup_pin() {
    assert!(EntityFacts::validate_pinned(SOURCE_BYTES).is_ok());
    let mut changed =
        EntityFacts::decode(SOURCE_BYTES, crate::active_content_registry_protocol()).unwrap();
    changed.records[0].fact.provisional = !changed.records[0].fact.provisional;
    let bytes = changed.encode().unwrap();
    assert!(EntityFacts::validate_pinned(&bytes).is_err());
}

#[test]
fn compiled_fish_animation_preserves_the_constructor_membership() {
    for record in &super::EntityFacts::pinned().records {
        assert_eq!(
            record.fact.fish_animation,
            matches!(
                record.identifier.as_ref(),
                "minecraft:cod"
                    | "minecraft:salmon"
                    | "minecraft:pufferfish"
                    | "minecraft:tropicalfish"
            ),
            "{}",
            record.identifier
        );
    }
}

/// Bamboo is the one boat variant drawn as a raft.
#[test]
fn the_raft_variant_is_bamboo() {
    assert_eq!(
        super::BOAT_WOOD_TEXTURES[super::BOAT_RAFT_VARIANT],
        "bamboo"
    );
}
