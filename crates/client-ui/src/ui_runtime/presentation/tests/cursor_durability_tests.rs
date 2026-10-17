//! A picked-up stack keeps its corrected wear in publication and both inventory painters.

use super::*;
use crate::ui_runtime::presentation::publish::capture_hud_frame;
use protocol::{
    ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack, StackResponse,
    StackResponseContainer, StackResponseSlot, StackResponseStatus,
};

mod armor;

/// Corrects mining damage or accepts a pickup from player slot zero onto the cursor.
fn corrected(request_id: i32, slot_type: u8, damage: i32, stack_id: i32) -> InventoryEvent {
    let mut containers = vec![StackResponseContainer {
        container: ContainerIdentity {
            window_id: None,
            slot_type: Some(slot_type),
            dynamic_id: None,
        },
        slots: Arc::from([StackResponseSlot {
            slot: 0,
            hotbar_slot: 0,
            count: 1,
            item_stack_id: stack_id,
            custom_name: Arc::from(""),
            filtered_custom_name: Arc::from(""),
            durability_correction: damage,
        }]),
    }];
    if slot_type == protocol::CONTAINER_NAME_CURSOR {
        containers.push(StackResponseContainer {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                dynamic_id: None,
            },
            slots: Arc::from([StackResponseSlot {
                slot: 0,
                hotbar_slot: 0,
                count: 0,
                item_stack_id: 0,
                custom_name: Arc::from(""),
                filtered_custom_name: Arc::from(""),
                durability_correction: -1,
            }]),
        });
    }
    InventoryEvent::Response(protocol::ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            request_id,
            status: StackResponseStatus::Accepted,
            containers: containers.into(),
        }]),
    })
}

#[test]
fn cursor_durability_follows_predicted_pickup_and_server_corrections() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    let stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    let registry = protocol::vanilla_item_registry();
    let tool = registry
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:wooden_pickaxe")
        .unwrap();
    let maximum = stream
        .authority()
        .item_durability(tool.network_id)
        .maximum()
        .unwrap();
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from([NetworkItemStack {
            network_id: tool.network_id,
            stack_network_id: 9,
            count: 1,
            ..NetworkItemStack::empty()
        }]),
        storage_item: NetworkItemStack::empty(),
    }));
    let damage = (maximum / 2) as i32;
    let mine = ledger.begin_mining_request(0, damage).unwrap();
    ledger.apply(&corrected(
        mine,
        protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
        damage,
        9,
    ));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0; 3],
        runtime_entity_id: -1,
    }));
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let capture = |player: &player_state::PlayerState,
                   runtime: &mut UiRuntime,
                   presentation: &mut UiPresentationRuntime| {
        capture_hud_frame(
            player,
            runtime,
            presentation,
            Some(&stream),
            semantic_input::PerspectiveMode::FirstPerson,
            0,
            Default::default(),
        );
    };
    capture(&player, &mut runtime, &mut presentation);
    let original = presentation.hud_frame().hotbar_durability[0].unwrap();
    assert!(original > 0.0 && original < 1.0);
    let take = runtime
        .inventory_ledger_mut(&mut player)
        .begin_take_count(0, 1)
        .unwrap();
    capture(&player, &mut runtime, &mut presentation);
    assert_eq!(presentation.hud_frame().hotbar_durability[0], None);
    assert_eq!(presentation.hud_frame().cursor_durability, Some(original));
    runtime.inventory_ledger_mut(&mut player).apply(&corrected(
        take,
        protocol::CONTAINER_NAME_CURSOR,
        damage + 1,
        10,
    ));
    capture(&player, &mut runtime, &mut presentation);
    let corrected = presentation.hud_frame().cursor_durability.unwrap();
    assert!(corrected < original);
    runtime
        .inventory_ledger_mut(&mut player)
        .begin_place_count(0, 1)
        .unwrap();
    capture(&player, &mut runtime, &mut presentation);
    assert_eq!(presentation.hud_frame().cursor_durability, None);
    assert_eq!(
        presentation.hud_frame().hotbar_durability[0],
        Some(corrected)
    );
}
