//! Server chest titles survive the HUD capture boundary after UI extraction.

use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue,
    ActorSpawnEvent, ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent,
    NetworkItemStack, WorldBootstrap, WorldEvent,
};

use super::{UiPresentationRuntime, UiRuntime, fixture_font};
use crate::ui_runtime::presentation::publish::capture_hud_frame;

#[test]
fn server_menu_title_keeps_actor_formatting_and_clears_empty_names() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut stream = chunk_pipeline::WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    // Distinct unique and runtime IDs catch use of the wrong actor identity.
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 70,
                runtime_id: 7,
                kind: ActorKind::Entity {
                    identifier: "minecraft:chest_minecart".into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    for (sequence, event) in [
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(4),
            window_type: protocol::WINDOW_TYPE_CONTAINER,
            position: [0; 3],
            runtime_entity_id: 70,
        }),
        InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(4),
            slots: vec![NetworkItemStack::empty(); 45].into(),
            storage_item: NetworkItemStack::empty(),
        }),
    ]
    .into_iter()
    .enumerate()
    {
        runtime
            .enqueue_inventory_event(&mut player, 1, sequence as u64 + 1, event)
            .unwrap();
    }
    runtime.drain_pending_inventory(&mut player);
    assert!(runtime.inventory_open());
    assert_eq!(
        runtime.inventory_ledger(&player).storage_slot_count(),
        Some(45)
    );
    for (sequence, title) in [(2, "§r§aServer menu\n§lPick a game"), (3, "")] {
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 7,
                    metadata: Arc::from([ActorMetadata {
                        key: 4,
                        value: ActorMetadataValue::String(title.into()),
                    }]),
                    properties: Arc::from([]),
                    tick: sequence,
                })),
            )
            .unwrap();
        capture_hud_frame(
            &player,
            &mut runtime,
            &mut presentation,
            Some(&stream),
            semantic_input::PerspectiveMode::FirstPerson,
            sequence,
            Default::default(),
        );
        let expected = (!title.is_empty()).then_some(title);
        let text = &presentation.hud_frame().window_text;
        assert_eq!(text.custom_title.as_deref(), expected);
        assert_eq!(text.title.as_deref(), expected);
    }
}

#[test]
fn server_menu_default_zero_descriptor_draws_chest_cells() {
    use crate::ui_runtime::presentation::{forms, inventory_pointer::InventoryScreen};
    let Some(mut presentation) =
        crate::test_support::engine_presentation_with(forms::pack_harness::font())
    else {
        eprintln!(
            "skipping server_menu_default_zero_descriptor_draws_chest_cells: missing local UI carrier; make assets"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    for (sequence, event) in [
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(4),
            window_type: protocol::WINDOW_TYPE_CONTAINER,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }),
        InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: Some(4),
                slot_type: Some(0),
                dynamic_id: Some(0),
            },
            slots: vec![
                NetworkItemStack::empty();
                inventory::inventory_ledger::LARGE_STORAGE_SLOT_COUNT
            ]
            .into(),
            storage_item: NetworkItemStack::empty(),
        }),
    ]
    .into_iter()
    .enumerate()
    {
        runtime
            .enqueue_inventory_event(&mut player, 1, sequence as u64 + 1, event)
            .unwrap();
    }
    runtime.drain_pending_inventory(&mut player);
    for now in [0, 500] {
        presentation
            .build(
                &player,
                &runtime,
                now,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    let input = presentation
        .build(
            &player,
            &runtime,
            5_000,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    forms::snapshot::write(&input, "server-chest-default-descriptor");
    let screen = InventoryScreen::of_runtime(&player, &runtime);
    assert_eq!(
        screen,
        InventoryScreen::Storage(inventory::inventory_ledger::LARGE_STORAGE_SLOT_COUNT)
    );
    let frame = presentation.engine_container_frame().unwrap();
    let last = frame.hits.iter().find(|hit| {
        hit.collection.as_deref() == Some("container_items") && hit.collection_index == Some(53)
    });
    assert!(last.is_some(), "the last chest cell remains interactive");
}
