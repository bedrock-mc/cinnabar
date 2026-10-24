use super::*;
use protocol::{ActorEvent, ActorIdentifier, ActorIdentifierRegistry, ActorMetadata};
use protocol_ids::ActorDataKey;
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
                key: ActorDataKey::Width.wire_value(),
                value: ActorMetadataValue::Float(2.0),
            },
            ActorMetadata {
                key: ActorDataKey::Height.wire_value(),
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
        "minecraft:area_effect_cloud",
    ]
    .iter()
    .enumerate()
    {
        assert_ne!(facts.get(base).class_key, 0);
        let expected = EntityFact {
            // A custom base supplies class facts, but cannot verify its interaction resolver.
            actor_use_resolver: assets::entity::offered_use::ActorUseResolver::Unresolved,
            ..facts.get(base)
        };
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
            assert_eq!(actor.entity_fact, expected, "base {base}, late {late}");
            assert_eq!(actor.is_horse(), facts.get(base).horse);
            assert_eq!(actor.is_collidable(), facts.get(base).collidable);
            assert_eq!(
                store.effect_members.collidable.contains(&id),
                actor.is_collidable()
            );
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

/// Admits a constructor from the real pinned inventory under a derived custom namespace.
fn admit_base(store: &mut ActorStore, identifier: &str, base: &str) {
    store.apply_actor_interactions(ActorIdentifierRegistry {
        entries: Arc::from([ActorIdentifier {
            identifier: identifier.into(),
            base_identifier: base.into(),
        }]),
        skipped: 0,
    });
}

/// Compares rendered endpoints, decoded targets and the native position after admission.
fn same_positions(first: &ActorSnapshot, second: &ActorSnapshot) {
    for (a, b) in [
        (first.position, second.position),
        (first.previous_pose.position, second.previous_pose.position),
        (first.received_pose.position, second.received_pose.position),
        (
            first.last_received_pose().position,
            second.last_received_pose().position,
        ),
        (first.native_position(), second.native_position()),
    ] {
        for (a, b) in a.into_iter().zip(b) {
            assert!((a - b).abs() < 1.0e-5, "{a} differs from {b}");
        }
    }
}

#[test]
fn late_bases_reconcile_spawn_teleport_and_queued_network_poses() {
    use protocol::{ActorInterpolation, ActorPositionOrigin};
    for record in EntityFacts::pinned()
        .records
        .iter()
        .filter(|r| r.fact.network_y_offset > 0.0)
    {
        let base = record.identifier.as_ref();
        let identifier = custom(base);
        for origin in [
            ActorPositionOrigin::NetworkOffset,
            ActorPositionOrigin::Feet,
        ] {
            for history in ["spawn", "teleport", "queued"] {
                let mut early = ActorStore::new(1, 0);
                let mut late = ActorStore::new(1, 0);
                admit_base(&mut early, &identifier, base);
                for store in [&mut early, &mut late] {
                    store.apply(1, 1, spawn(&identifier, 1));
                }
                let offset = early.actors[&1].network_position_offset();
                if history != "spawn" {
                    let wire_y = if origin == ActorPositionOrigin::NetworkOffset {
                        64.0 + offset
                    } else {
                        64.0
                    };
                    let ActorEvent::Move(mut movement) =
                        super::super::tests::network_move(1, wire_y, history == "teleport")
                    else {
                        unreachable!()
                    };
                    movement.position_origin = origin;
                    movement.on_ground = None;
                    movement.interpolation = ActorInterpolation {
                        ticks: 3,
                        force_completion: true,
                    };
                    for store in [&mut early, &mut late] {
                        store.apply(1, 2, ActorEvent::Move(movement));
                        if history == "queued" {
                            store.advance_interpolation_ticks(1);
                        }
                    }
                    if history == "queued" {
                        movement.position[1] = Some(wire_y + 4.0);
                        movement.interpolation.force_completion = false;
                        for store in [&mut early, &mut late] {
                            store.apply(1, 3, ActorEvent::Move(movement));
                        }
                        // A rotation-only feet packet must retain the buffered Y's original frame.
                        movement.position = [None; 3];
                        movement.position_origin = ActorPositionOrigin::Feet;
                        movement.yaw = Some(45.0);
                        for store in [&mut early, &mut late] {
                            store.apply(1, 4, ActorEvent::Move(movement));
                        }
                    }
                }
                admit_base(&mut late, &identifier, base);
                same_positions(&early.actors[&1], &late.actors[&1]);
                for _ in 0..6 {
                    early.advance_interpolation_ticks(1);
                    late.advance_interpolation_ticks(1);
                    same_positions(&early.actors[&1], &late.actors[&1]);
                }
            }
        }
    }
}

#[test]
fn late_falling_block_admission_preserves_sync_and_waits_for_initial_terrain() {
    use super::super::terrain_interlock::{FALLING_VISIBILITY_FALLBACK, TerrainInterlock};
    let base = "minecraft:falling_block";
    let identifier = custom(base);
    for message in [None, Some(1), Some(2), Some(99)] {
        let mut early = ActorStore::new(1, 0);
        let mut late = ActorStore::new(1, 0);
        let mut frame_at = None;
        admit_base(&mut early, &identifier, base);
        for store in [&mut early, &mut late] {
            let ActorEvent::Spawn(mut event) = spawn(&identifier, 1) else {
                unreachable!()
            };
            let mut metadata = event.metadata.to_vec();
            metadata.push(ActorMetadata {
                key: ActorDataKey::Variant.wire_value(),
                value: ActorMetadataValue::Int(0),
            });
            event.metadata = metadata.into();
            store.apply(1, 1, ActorEvent::Spawn(event));
            if let TerrainInterlock::Pending { attached_at } =
                store.actors[&1].status.terrain_interlock
            {
                frame_at.get_or_insert(attached_at);
            }
            if let Some(message) = message {
                assert_eq!(
                    store.apply_terrain_sync(protocol::ActorBlockSyncMessage {
                        actor_unique_id: 1,
                        message,
                    }),
                    matches!(message, 1 | 2)
                );
            }
        }
        let initial = late.actors[&1].status.terrain_interlock;
        admit_base(&mut late, &identifier, base);
        let state = late.actors[&1].status.terrain_interlock;
        let at = match message {
            Some(1) => {
                assert_eq!(state, TerrainInterlock::Visible);
                frame_at.unwrap()
            }
            Some(2) => {
                assert_eq!(state, TerrainInterlock::Hidden);
                frame_at.unwrap()
            }
            _ => {
                let TerrainInterlock::Pending { attached_at } = state else {
                    panic!("late falling-block admission must wait for terrain")
                };
                assert_eq!(initial, TerrainInterlock::InitialVisible { attached_at });
                let deadline = attached_at + FALLING_VISIBILITY_FALLBACK;
                assert!(late.block_entities_at(1.0, deadline).is_empty());
                assert_eq!(
                    late.block_entities_at(1.0, deadline + std::time::Duration::from_nanos(1))
                        .len(),
                    1
                );
                attached_at
            }
        };
        let visible = message == Some(1);
        assert_eq!(late.block_entity_candidates_at(1.0, at).len(), 1);
        assert_eq!(late.block_entity_candidates_at(1.0, at)[0].visible, visible);
        assert_eq!(late.block_entities_at(1.0, at).len(), usize::from(visible));
        admit_base(&mut late, &identifier, base);
        assert_eq!(late.actors[&1].status.terrain_interlock, state);
    }
}
