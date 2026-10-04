//! The physics phase reads the same player owner that earlier commands mutate.
use crate::movement::GameplayWorldView;
use crate::player_runtime::PlayerRuntime;
use bevy::prelude::*;
use gameplay::movement::control_modes::SPRINT_HUNGER_FLOOR;
use gameplay::movement::local_facts::{LocalMovementFacts, read};
use gameplay::test_support::DEPTH_STRIDER_ENCHANTMENT_ID;
use inventory::inventory_ledger::{
    CellGesture, InventoryTarget, PERSONAL_INVENTORY_WINDOW_TYPE, PLAYER_INVENTORY_SLOT_COUNT,
};
use protocol::{ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack};
use std::sync::Arc;

#[derive(Resource)]
struct Stream(chunk_pipeline::WorldStream);

#[derive(Resource, Default)]
struct Observed(Option<LocalMovementFacts>);

/// Builds fixed little-endian item NBT with a known movement enchantment.
fn enchanted_boots() -> NetworkItemStack {
    let mut bytes = vec![255, 255, 1, 10, 0, 0, 9, 4, 0];
    bytes.extend(b"ench");
    bytes.push(10);
    bytes.extend(1_i32.to_le_bytes());
    for (name, value) in [
        (b"id".as_slice(), DEPTH_STRIDER_ENCHANTMENT_ID),
        (b"lvl".as_slice(), 3),
    ] {
        bytes.push(2);
        bytes.extend((name.len() as u16).to_le_bytes());
        bytes.extend(name);
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0, 0]);
    NetworkItemStack {
        network_id: 1,
        count: 1,
        stack_network_id: 10,
        extra_data: bytes.into(),
        ..NetworkItemStack::default()
    }
}

/// Seeds known cells and an acknowledged personal window without a transport connection.
fn player() -> PlayerRuntime {
    let mut player = PlayerRuntime::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let ledger = player.inventory.ledger_mut();
    ledger.apply(&InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    for (container, slots) in [
        (
            ContainerIdentity::window(0),
            vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT],
        ),
        (
            ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
            vec![
                NetworkItemStack::default(),
                NetworkItemStack::default(),
                NetworkItemStack::default(),
                enchanted_boots(),
            ],
        ),
        (
            ContainerIdentity {
                window_id: None,
                slot_type: Some(protocol::CONTAINER_NAME_CURSOR),
                dynamic_id: None,
            },
            vec![NetworkItemStack::default()],
        ),
    ] {
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container,
            slots: slots.into(),
            storage_item: NetworkItemStack::default(),
        }));
    }
    assert!(ledger.request_personal_open(1));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 0, 0],
        runtime_entity_id: -1,
    }));
    player
}

/// Issues the same synchronous armor gesture as the UI, without a server response.
fn command(mut player: ResMut<PlayerRuntime>) {
    player
        .inventory
        .ledger_mut()
        .begin_target_gesture(InventoryTarget::Armor(3), CellGesture::Click)
        .unwrap();
    player
        .facts
        .apply_hunger_attribute(&protocol::ActorAttribute {
            name: Arc::from("minecraft:player.hunger"),
            min: 0.0,
            max: 20.0,
            current: 0.0,
            default: Some(20.0),
            modifiers: Arc::from([]),
        });
}

/// Samples the production movement view in the physics phase.
fn sample(player: Res<PlayerRuntime>, stream: Res<Stream>, mut observed: ResMut<Observed>) {
    observed.0 = Some(read(Some(&player), &GameplayWorldView(&stream.0), false));
}

#[test]
fn physics_reads_predicted_equipment_and_hunger_in_the_same_frame() {
    let player = player();
    let stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0, 70.0, 0.0],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let before = read(Some(&player), &GameplayWorldView(&stream), false);
    assert_eq!(before.depth_strider, 3);
    assert!(!before.sprint_blocked);
    assert!(before.swim_hunger_blocked);
    let mut app = App::new();
    app.insert_resource(player)
        .insert_resource(Stream(stream))
        .init_resource::<Observed>()
        .configure_sets(
            Update,
            (
                crate::app::ClientFrameSet::UiAuthority,
                crate::app::ClientFrameSet::Physics,
            )
                .chain(),
        )
        .add_systems(
            Update,
            command.in_set(crate::app::ClientFrameSet::UiAuthority),
        )
        .add_systems(Update, sample.in_set(crate::app::ClientFrameSet::Physics));
    app.update();
    let after = app.world().resource::<Observed>().0.unwrap();
    assert_eq!(after.depth_strider, 0);
    assert!(after.sprint_blocked);
    assert!(after.swim_hunger_blocked);
    assert!(
        app.world()
            .resource::<PlayerRuntime>()
            .inventory
            .ledger()
            .pending_request_id()
            .is_some()
    );
}

#[test]
fn swimming_food_gate_uses_native_floor_and_flight_permission() {
    let mut player = player();
    let stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0, 70.0, 0.0],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    assert!(read(Some(&player), &GameplayWorldView(&stream), false).swim_hunger_blocked);
    for (food, blocked) in [
        (SPRINT_HUNGER_FLOOR, true),
        (SPRINT_HUNGER_FLOOR + 1, false),
    ] {
        player
            .facts
            .apply_hunger_attribute(&protocol::ActorAttribute {
                name: Arc::from("minecraft:player.hunger"),
                min: 0.0,
                max: 20.0,
                current: f32::from(food),
                default: Some(20.0),
                modifiers: Arc::from([]),
            });
        assert_eq!(
            read(Some(&player), &GameplayWorldView(&stream), true).swim_hunger_blocked,
            blocked
        );
    }
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    player
        .facts
        .apply_hunger_attribute(&protocol::ActorAttribute {
            name: Arc::from("minecraft:player.hunger"),
            min: 0.0,
            max: 20.0,
            current: 0.0,
            default: Some(20.0),
            modifiers: Arc::from([]),
        });
    assert!(!read(Some(&player), &GameplayWorldView(&stream), false).swim_hunger_blocked);
}
