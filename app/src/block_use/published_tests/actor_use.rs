use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;
use protocol::ActorUseAction;
use protocol::test_support::transactions::{TransactionBody, observe_transaction};
use {super::*, gameplay::block_use::RepeatClock};

/// Adds a selectable actor after the fixture's latest terrain update.
pub(super) fn spawn(world: &mut World, z: f32) {
    spawn_kind(
        world,
        z,
        protocol::ActorKind::Entity {
            identifier: "minecraft:villager_v2".into(),
        },
    );
}

/// Commits a selectable actor of the requested kind into the fixture's stream.
fn spawn_kind(world: &mut World, z: f32, kind: protocol::ActorKind) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream
        .submit(
            sequence,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 99,
                runtime_id: 99,
                kind,
                position: [4.5, 1.5, z],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: protocol::NetworkItemStack::empty(),
                metadata: [].into(),
                attributes: [].into(),
                properties: [].into(),
                links: [].into(),
            })),
        )
        .unwrap();
    test_time::eventually_within(Duration::from_secs(5), "fixture actor spawn", || {
        stream.poll([4.5, 2.620_01, 8.5], 0);
        stream.committed_sequence() >= sequence
    });
}

#[test]
fn actor_use_is_admitted_on_the_press_frame_before_the_next_physics_tick() {
    let (mut world, mut captured) = fixture();
    spawn(&mut world, 7.5);
    let tick = world.resource::<MovementTicker>().completed_tick();
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(world.resource::<MovementTicker>().completed_tick(), tick);
    let packets = captured.drain();
    assert_eq!(packets.len(), 1);
    let packet =
        observe_transaction(&packets[0]).expect("the press must emit an actor transaction");
    let tx = packet
        .actor_use()
        .expect("the nearer actor must consume the block and air use");
    assert_eq!(tx.action, Some(ActorUseAction::Interact));
    assert_eq!(tx.runtime_id, 99);
    assert_eq!(tx.slot, 0);
    assert_eq!(tx.from_position, [4.5, 2.620_01, 8.5]);
    assert!(world.resource::<BlockUseRuntime>().press_interacted());
    hold_use(&mut world);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        captured.drain().is_empty(),
        "holding must not duplicate an actor press"
    );
}

#[test]
fn a_block_occludes_actor_use() {
    let (mut world, mut captured) = fixture();
    spawn(&mut world, 5.5);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]]);
}

#[test]
fn a_refused_actor_press_remains_pending_without_consuming_air_use() {
    let (mut world, _) = fixture();
    spawn(&mut world, 7.5);
    let (network, _guard) = NetworkHandle::<PackApplication>::with_command_capacity(1);
    network
        .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Build))
        .unwrap();
    world.insert_resource(network);
    world.run_system_cached(produce_block_use).unwrap();
    let runtime = world.resource::<BlockUseRuntime>();
    assert!(!runtime.press_interacted());
    assert!(
        runtime
            .due(
                true,
                102,
                RepeatClock {
                    now: Duration::from_millis(1_000),
                    sneaking: false,
                    speed: 0.0,
                    survival: true
                }
            )
            .is_some()
    );
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    world.insert_resource(network);
    hold_use(&mut world);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(captured.drain().len(), 1);
    assert!(world.resource::<BlockUseRuntime>().press_interacted());
}

/// Uses a snowball on a player after the server set `interact_text`, returning the packet kinds.
fn throw_at_player(interact_text: &str) -> Vec<&'static str> {
    let (mut world, mut captured) = fixture();
    super::item_use_tests::hold_snowball(&mut world);
    spawn_kind(
        &mut world,
        7.5,
        protocol::ActorKind::Player {
            uuid: [7; 16],
            username: "target".into(),
        },
    );
    world
        .resource_mut::<UiRuntimeResource>()
        .apply_local_metadata(
            7,
            u64::MAX,
            &[protocol::ActorMetadata {
                key: 100,
                value: protocol::ActorMetadataValue::String(interact_text.into()),
            }],
        )
        .unwrap();
    world.run_system_cached(produce_block_use).unwrap();
    produce_air_use(&mut world);
    captured
        .drain()
        .into_iter()
        .filter_map(|packet| match observe_transaction(&packet)?.body {
            TransactionBody::ItemUseOnActor(tx) => {
                assert_eq!(tx.action, Some(ActorUseAction::Interact));
                assert_eq!(tx.runtime_id, 99);
                Some("interact")
            }
            TransactionBody::ItemUse(_) => Some("use"),
            _ => None,
        })
        .collect()
}

/// A throwable used on a player with no interaction must still throw, after the interact.
#[test]
fn a_throwable_used_on_a_player_without_an_interaction_still_throws() {
    assert_eq!(throw_at_player(""), ["interact", "use"]);
}

#[test]
fn a_server_offered_player_interaction_consumes_the_use() {
    assert_eq!(throw_at_player("action.interact.ride.horse"), ["interact"]);
}

/// Uses original component facts with the production actor and held-item FIFO producers.
fn throw_at_entity(
    identifier: &str,
    offered_use: assets::entity::offered_use::EntityUseAdmission,
    interact_text: &str,
) -> Vec<&'static str> {
    let mut compiled =
        assets::carriers::startup::test_carriers::synthetic_interaction_assets(identifier);
    let mut sources = compiled.sources.into_vec();
    sources.insert(
        0,
        assets::EntityAssetSource {
            path: "behavior/entities/fixture.json".into(),
            source_bytes: 1,
            source_sha256: [1; 32],
        },
    );
    let (source_bytes, source_sha256) = assets::item_use::source_identity();
    sources.push(assets::EntityAssetSource {
        path: assets::item_use::SOURCE_PATH.into(),
        source_bytes,
        source_sha256,
    });
    compiled.symbols[0].source_index += 1;
    compiled.sources = sources.into();
    compiled.item_use_profiles = assets::item_use::compile(&compiled.item_visuals);
    compiled
        .interaction_traits
        .iter_mut()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap()
        .offered_use = offered_use;
    let entities = std::sync::Arc::new(
        assets::RuntimeEntityAssets::decode(&assets::encode_entity_blob(&compiled).unwrap())
            .unwrap(),
    );
    let (mut world, mut captured) = super::fixture_with_entity_assets(
        NetworkHandle::<PackApplication>::stub_capturing_packets(),
        Some(entities),
    );
    super::item_use_tests::hold_snowball(&mut world);
    spawn_kind(
        &mut world,
        7.5,
        protocol::ActorKind::Entity {
            identifier: identifier.into(),
        },
    );
    world
        .resource_mut::<UiRuntimeResource>()
        .apply_local_metadata(
            7,
            u64::MAX,
            &[protocol::ActorMetadata {
                key: protocol_ids::ActorDataKey::InteractText.wire_value(),
                value: protocol::ActorMetadataValue::String(interact_text.into()),
            }],
        )
        .unwrap();
    world.run_system_cached(produce_block_use).unwrap();
    produce_air_use(&mut world);
    captured
        .drain()
        .into_iter()
        .filter_map(|packet| match observe_transaction(&packet)?.body {
            TransactionBody::ItemUseOnActor(tx) => {
                assert_eq!(tx.runtime_id, 99);
                assert_eq!(tx.action, Some(ActorUseAction::Interact));
                Some("interact")
            }
            TransactionBody::ItemUse(_) => Some("use"),
            _ => None,
        })
        .collect()
}

/// A no-offering descriptor is evidence only alongside the actor's verified resolver disposition.
fn no_component_offering() -> assets::entity::offered_use::EntityUseAdmission {
    assets::entity::offered_use::EntityUseAdmission::Components {
        source: 0,
        offerings: Box::new([]),
    }
}

#[test]
fn a_throwable_used_on_a_zombie_without_an_offering_still_throws_after_interact() {
    assert_eq!(
        throw_at_entity("minecraft:zombie", no_component_offering(), ""),
        ["interact", "use"]
    );
}

#[test]
fn server_text_native_prevent_default_and_unresolved_actors_preserve_interaction() {
    assert_eq!(
        throw_at_entity(
            "minecraft:zombie",
            no_component_offering(),
            "action.interact.fixture"
        ),
        ["interact"]
    );
    assert_eq!(
        throw_at_entity("minecraft:armor_stand", no_component_offering(), ""),
        ["interact"]
    );
    assert_eq!(
        throw_at_entity("minecraft:villager_v2", no_component_offering(), ""),
        ["interact"]
    );
    assert_eq!(
        throw_at_entity("minecraft:zombie", Default::default(), ""),
        ["interact"]
    );
}

#[test]
fn possible_ride_feed_equipment_and_offhand_offerings_consume_the_actor_press() {
    use assets::entity::offered_use::{
        ComponentUse, EntityUseAdmission, UseCondition, UseSelector,
    };
    for (identifier, component, condition) in [
        (
            "minecraft:horse",
            "minecraft:rideable",
            UseCondition::Unresolved,
        ),
        (
            "minecraft:happy_ghast",
            "minecraft:ageable",
            UseCondition::HeldItem {
                identifier: "minecraft:snowball".into(),
                metadata: None,
            },
        ),
        (
            "minecraft:zombie",
            "minecraft:equippable",
            UseCondition::HeldItem {
                identifier: "minecraft:snowball".into(),
                metadata: None,
            },
        ),
        (
            "minecraft:zombie",
            "minecraft:interact",
            UseCondition::Unresolved,
        ),
    ] {
        let definition = EntityUseAdmission::Components {
            source: 0,
            offerings: Box::from([ComponentUse {
                selector: UseSelector::UnresolvedComponentGroup { group: 0 },
                component: component.into(),
                condition,
            }]),
        };
        assert_eq!(
            throw_at_entity(identifier, definition, ""),
            ["interact"],
            "{component}"
        );
    }
}
