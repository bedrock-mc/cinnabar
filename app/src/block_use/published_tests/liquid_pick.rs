//! Bucket targets through the production interaction producer and negotiated item registry.

use super::*;
use assets::facts::EMPTY_BUCKET_IDENTIFIER;

/// Replaces the chest with a liquid and puts an ordinary block behind it.
fn liquid_fixture(
    identifier: &str,
    metadata: u32,
    liquid: &str,
    depth: u8,
    obstructed: bool,
) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = fixture();
    item_use_tests::hold_air_item(&mut world, identifier, 1, std::sync::Arc::from([]));
    if metadata != 0 {
        world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
            let mut player = world.resource_mut::<client_state::PlayerRuntime>();
            let mut stack = player.selected_stack().unwrap().clone();
            stack.metadata = metadata;
            ui.inventory_ledger_mut(&mut player)
                .apply(&InventoryEvent::Slot(InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity {
                            window_id: Some(0),
                            slot_type: None,
                            dynamic_id: None,
                        },
                        slot: 0,
                    },
                    stack,
                    storage_item: None,
                }));
        });
    }
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let liquid = records
        .iter()
        .find(|record| {
            record.name.as_ref() == liquid
                && serde_json::from_str::<serde_json::Value>(&record.canonical_state).unwrap()
                    [assets::facts::BlockProperty::LiquidDepth.name()]["value"]
                    .as_u64()
                    == Some(u64::from(depth))
        })
        .unwrap()
        .sequential_id;
    let stone = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .unwrap()
        .sequential_id;
    let mut updates = vec![
        protocol::BlockUpdateEvent {
            dimension: 0,
            position: [4, 2, 6],
            layer: 0,
            network_id: liquid,
        },
        protocol::BlockUpdateEvent {
            dimension: 0,
            position: [4, 2, 5],
            layer: 0,
            network_id: stone,
        },
    ];
    if obstructed {
        updates.push(protocol::BlockUpdateEvent {
            dimension: 0,
            position: [4, 2, 7],
            layer: 0,
            network_id: stone,
        });
    }
    let mut client_world = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client_world.stream.as_mut().unwrap();
    stream
        .submit(3, protocol::WorldEvent::BlockUpdates(updates))
        .unwrap();
    test_time::eventually_within(Duration::from_secs(5), "liquid fixture commit", || {
        stream.poll([4.5, 2.620_01, 8.5], 0);
        stream.committed_sequence() >= 3
    });
    (world, captured)
}

#[test]
fn empty_bucket_clicks_the_liquid_source_instead_of_the_block_behind_it() {
    assert!(
        protocol::vanilla_item_registry()
            .iter()
            .any(|entry| entry.identifier.as_ref() == EMPTY_BUCKET_IDENTIFIER)
    );
    for liquid in ["minecraft:water", "minecraft:lava"] {
        let (mut world, mut captured) =
            liquid_fixture(EMPTY_BUCKET_IDENTIFIER, 0, liquid, 0, false);
        world.run_system_cached(produce_block_use).unwrap();
        assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]], "{liquid}");
        let player = world.resource::<client_state::PlayerRuntime>();
        assert_eq!(player.selected_stack().unwrap().metadata, 0);
        assert_eq!(player.selected_stack().unwrap().count, 1);
    }
}

#[test]
fn liquid_pick_keeps_flowing_liquids_other_items_and_solid_occluders_distinct() {
    for (item, metadata, depth, obstructed, expected) in [
        (EMPTY_BUCKET_IDENTIFIER, 0, 1, false, [4, 2, 5]),
        (EMPTY_BUCKET_IDENTIFIER, 8, 0, false, [4, 2, 5]),
        ("minecraft:water_bucket", 0, 0, false, [4, 2, 5]),
        ("minecraft:diamond_sword", 0, 0, false, [4, 2, 5]),
        ("fixture:bucket", 0, 0, false, [4, 2, 5]),
        (EMPTY_BUCKET_IDENTIFIER, 0, 0, true, [4, 2, 7]),
    ] {
        let (mut world, mut captured) =
            liquid_fixture(item, metadata, "minecraft:water", depth, obstructed);
        world.run_system_cached(produce_block_use).unwrap();
        assert_eq!(
            transaction_targets(&mut captured),
            [expected],
            "{item} {depth} {obstructed}"
        );
    }
}
