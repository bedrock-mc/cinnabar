use super::*;
use protocol::{ActorEvent, ActorIdentifier, ActorIdentifierRegistry, ActorMetadata};
use std::sync::Arc;

/// Derives a custom namespace from a real pinned constructor for the inheritance regression.
fn custom(base: &str) -> Arc<str> {
    format!("fixture:{}", base.strip_prefix("minecraft:").unwrap()).into()
}

/// Supplies streamed bounds using the existing actor ingress fixture.
fn spawn(identifier: &str, id: u64) -> ActorEvent {
    super::super::tests::entity_spawn(
        id,
        id as i64,
        identifier,
        Arc::from([
            ActorMetadata {
                key: BOUNDING_BOX_WIDTH_METADATA_KEY,
                value: ActorMetadataValue::Float(2.0),
            },
            ActorMetadata {
                key: BOUNDING_BOX_HEIGHT_METADATA_KEY,
                value: ActorMetadataValue::Float(1.0),
            },
        ]),
    )
}

#[test]
fn custom_entities_inherit_native_class_shadow_offsets_and_effect_membership() {
    let facts = EntityFacts::pinned();
    for (i, base) in [
        "minecraft:spider",
        "minecraft:arrow",
        "minecraft:horse",
        "minecraft:boat",
        "minecraft:ender_dragon",
        "minecraft:ender_crystal",
        "minecraft:painting",
        "minecraft:falling_block",
        "minecraft:splash_potion",
        "minecraft:wolf",
    ]
    .iter()
    .enumerate()
    {
        assert_ne!(facts.get(base).class_key, 0);
        let identifier = custom(base);
        for late in [false, true] {
            let mut store = ActorStore::new(1, 0);
            let id = i as u64 + 1;
            let registry = || ActorIdentifierRegistry {
                entries: Arc::from([ActorIdentifier {
                    identifier: identifier.clone(),
                    base_identifier: Arc::from(*base),
                }]),
                skipped: 0,
            };
            if !late {
                store.apply_actor_interactions(registry());
            }
            store.apply(1, 1, spawn(&identifier, id));
            if late {
                store.actors.get_mut(&id).unwrap().status.terrain_interlock =
                    super::super::terrain_interlock::TerrainInterlock::Hidden;
                store.apply_actor_interactions(registry());
                assert_eq!(
                    store.actors[&id].status.terrain_interlock,
                    super::super::terrain_interlock::TerrainInterlock::Hidden
                );
            }
            let actor = &store.actors[&id];
            assert_eq!(
                actor.entity_fact,
                facts.get(base),
                "base {base}, late {late}"
            );
            assert_eq!(actor.is_horse(), facts.get(base).horse);
            assert_eq!(
                actor.shadow_radius(false),
                facts.get(base).shadow.radius(2.0, false, 0, false)
            );
            assert_eq!(actor.shadow_hidden(), facts.get(base).projectile);
            assert_eq!(
                store.effect_members.dragons.contains(&id),
                actor.entity_fact.native_class == NativeEntityClass::EnderDragon
            );
            assert_eq!(
                store.effect_members.crystals.contains(&id),
                actor.entity_fact.native_class == NativeEntityClass::EnderCrystal
            );
            assert_eq!(
                store.effect_members.paintings.contains(&id),
                actor.entity_fact.native_class == NativeEntityClass::Painting
            );
            if actor.entity_fact.native_class == NativeEntityClass::Boat {
                assert_eq!(
                    actor.network_position_offset(),
                    facts.get(base).network_y_offset
                );
            }
            let previous = actor.entity_fact;
            assert_eq!(store.mount_equipment(id), facts.get(base).equipment);
            store.apply_actor_interactions(ActorIdentifierRegistry {
                entries: Arc::from([ActorIdentifier {
                    identifier: identifier.clone(),
                    base_identifier: "minecraft:bee".into(),
                }]),
                skipped: 0,
            });
            assert_eq!(store.actors[&id].entity_fact, previous);
            store.begin_session(2, 0);
            assert!(store.custom_actor_facts.is_empty());
        }
    }
}

#[test]
fn item_network_offset_matches_the_shared_protocol_contract() {
    assert_eq!(
        EntityFacts::pinned().get("minecraft:item").network_y_offset,
        protocol::ITEM_ACTOR_NETWORK_OFFSET
    );
}

#[test]
fn unknown_bases_are_retained_unresolved_and_counted_once() {
    let mut store = ActorStore::new(1, 0);
    let registry = ActorIdentifierRegistry {
        entries: Arc::from([ActorIdentifier {
            identifier: custom("minecraft:spider"),
            base_identifier: custom("minecraft:horse"),
        }]),
        skipped: 0,
    };
    store.apply_actor_interactions(registry.clone());
    assert_eq!(store.actor_identifier_skips, 1);
    let fact = store.custom_actor_facts.values().next().unwrap();
    assert_eq!(fact.native_class, NativeEntityClass::Unresolved);
    store.apply_actor_interactions(registry);
    assert_eq!(store.actor_identifier_skips, 1);
}
