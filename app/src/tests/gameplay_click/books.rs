//! Book opening admits initialization before the editor takes input ownership.

use super::*;
use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;
use protocol::{ContainerIdentity, InventoryContentEvent, InventoryEvent, ItemRegistryEvent};

/// Installs a fresh writable book and a pose for its opening transaction.
fn book_app() -> App {
    let mut app = gameplay_app(false);
    let registry = protocol::vanilla_item_registry();
    let network_id = registry
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:writable_book")
        .unwrap()
        .network_id;
    let mut player = app.world_mut().resource_mut::<PlayerRuntime>();
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let ledger = player.inventory.ledger_mut();
    ledger.apply_registry(&ItemRegistryEvent { entries: registry });
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        slots: vec![protocol::NetworkItemStack {
            network_id,
            count: 1,
            stack_network_id: 9,
            ..protocol::NetworkItemStack::empty()
        }]
        .into(),
        storage_item: protocol::NetworkItemStack::empty(),
    }));
    let mut movement = client_interaction::movement::MovementTicker::default();
    movement.reset(7, 100, [0.0, 70.0, 0.0]);
    movement.set_source(gameplay::movement::MovementSource::Physics);
    app.insert_resource(movement);
    app
}

#[test]
fn opening_a_fresh_book_admits_initialization_before_editing() {
    for touch in [false, true] {
        let mut app = book_app();
        let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
        app.insert_resource(network);
        app.update();
        if touch {
            app.add_systems(
                Update,
                press_touch_book.in_set(ClientFrameSet::SemanticSample),
            );
        } else {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Right);
        }
        app.update();
        assert!(
            app.world()
                .resource::<UiRuntimeResource>()
                .screen_state()
                .book
                .is_some()
        );
        let packets = captured.drain();
        assert_eq!(
            packets.len(),
            1,
            "opening must admit the initialization transaction"
        );
        let transaction =
            protocol::test_support::transactions::observe_transaction(&packets[0]).unwrap();
        assert_eq!(transaction.actions.len(), 1);
        assert!(!protocol::book_needs_init(
            &transaction.actions[0].to_item.user_data
        ));
    }
}

/// Supplies the touch use edge after raw touch routing has sampled this frame.
fn press_touch_book(
    mut targets: bevy::prelude::ResMut<crate::semantic_controls::SemanticTouchTargets>,
) {
    targets.press(semantic_input::touch::USE);
}

#[test]
fn opening_a_fresh_book_waits_when_transport_is_full() {
    let mut app = book_app();
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    let packet =
        protocol::book_edit_packet(0, &protocol::BookEdit::DeletePage { page: 1 }).unwrap();
    while network.send_inventory_packet(packet.clone()).is_ok() {}
    app.insert_resource(network);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .screen_state()
            .book
            .is_none()
    );
    let player = app.world().resource::<PlayerRuntime>();
    assert!(protocol::book_needs_init(
        &player
            .inventory
            .ledger()
            .displayed_stack(0)
            .unwrap()
            .extra_data
    ));
    captured.drain();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all();
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .screen_state()
            .book
            .is_some()
    );
    assert_eq!(captured.drain().len(), 1);
}
