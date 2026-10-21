use super::*;

/// Opens a confirmed personal screen without refreshing the selected backing book.
fn open_inventory(player: &mut player_state::PlayerState) {
    let ledger = player.inventory.ledger_mut();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&protocol::InventoryEvent::Open(
        protocol::ContainerOpenEvent {
            container: protocol::ContainerIdentity::window(2),
            window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        },
    ));
}

/// Installs an explicit two-cell fixture, keeping item IDs tied to the registry.
fn publish_pair(player: &mut player_state::PlayerState, book_first: bool) {
    let book = player.inventory.ledger().backing_stack(0).unwrap().clone();
    let stone_id = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:stone")
        .unwrap()
        .network_id;
    let stone = protocol::NetworkItemStack {
        network_id: stone_id,
        count: 1,
        stack_network_id: 11,
        ..protocol::NetworkItemStack::empty()
    };
    let slots = if book_first {
        vec![book, stone]
    } else {
        vec![stone, book]
    };
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Content(
            protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(
                    protocol::PLAYER_INVENTORY_WINDOW_ID,
                ),
                slots: slots.into(),
                storage_item: protocol::NetworkItemStack::empty(),
            },
        ));
}

/// A sparse removal or replacement cannot hide the backing book from item use.
#[test]
fn held_book_uses_backing_behind_screen_prediction() {
    for replace in [false, true] {
        let (mut player, mut runtime) = held_book();
        publish_pair(&mut player, true);
        open_inventory(&mut player);
        let ledger = player.inventory.ledger_mut();
        let mut requests = vec![ledger.begin_click(u8::from(replace)).unwrap()];
        if replace {
            requests.push(ledger.begin_click(0).unwrap());
        }
        while ledger.pending_batch().unwrap().is_some() {
            assert!(ledger.mark_transport_enqueued(1));
        }
        let mut packets = Vec::new();
        assert!(
            runtime.open_held_book(&mut player, Some([0.0, 70.0, 0.0]), |packet| {
                packets.push(packet);
                true
            })
        );
        assert!(runtime.screen.book.as_ref().unwrap().editable);
        assert_eq!(packets.len(), 1);
        let transaction =
            protocol::test_support::transactions::observe_transaction(&packets[0]).unwrap();
        assert_eq!(transaction.actions[0].from_item.stack_network_id, Some(9));
        assert!(!protocol::book_needs_init(
            &player
                .inventory
                .ledger()
                .backing_stack(0)
                .unwrap()
                .extra_data
        ));
        let ledger = player.inventory.ledger_mut();
        for request_id in requests.into_iter().rev() {
            ledger.apply(&protocol::InventoryEvent::Response(
                protocol::ItemStackResponseEvent {
                    responses: vec![protocol::StackResponse {
                        request_id,
                        status: protocol::StackResponseStatus::Rejected,
                        containers: [].into(),
                    }]
                    .into(),
                },
            ));
        }
        assert!(!protocol::book_needs_init(
            &ledger.backing_stack(0).unwrap().extra_data
        ));
    }
}

/// A book visible only in sparse prediction must not open over a backing non-book.
#[test]
fn screen_only_book_does_not_open() {
    let (mut player, mut runtime) = held_book();
    publish_pair(&mut player, false);
    open_inventory(&mut player);
    let ledger = player.inventory.ledger_mut();
    ledger.begin_click(1).unwrap();
    ledger.begin_click(0).unwrap();
    assert!(
        !runtime.open_held_book(&mut player, Some([0.0, 70.0, 0.0]), |_| panic!(
            "backing stone cannot use a book"
        ))
    );
    assert!(runtime.screen.book.is_none());
}

/// An unrelated retained input does not prevent an admitted backing-book initialization.
#[test]
fn held_book_opens_during_crafting_recovery() {
    let (mut player, mut runtime) = held_book();
    open_inventory(&mut player);
    let ledger = player.inventory.ledger_mut();
    let book = ledger.backing_stack(0).unwrap().clone();
    ledger.apply(&protocol::InventoryEvent::Slot(
        protocol::InventorySlotEvent {
            identity: protocol::SlotIdentity {
                container: protocol::ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slot: 28,
            },
            stack: book,
            storage_item: None,
        },
    ));
    ledger.apply(&protocol::InventoryEvent::Close(
        protocol::ContainerCloseEvent {
            container: protocol::ContainerIdentity::window(2),
            window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
            server_initiated: true,
        },
    ));
    assert!(ledger.resync_required());
    let mut sent = 0;
    assert!(
        runtime.open_held_book(&mut player, Some([0.0, 70.0, 0.0]), |_| {
            sent += 1;
            true
        })
    );
    assert_eq!(sent, 1);
    assert!(runtime.screen.book.is_some());
}

/// Reading an initialized or written book still belongs to ordinary item use.
#[test]
fn initialized_books_send_item_use_before_opening() {
    for editable in [false, true] {
        let (mut player, mut runtime) = held_book();
        assert!(runtime.open_held_book(&mut player, Some([0.0, 70.0, 0.0]), |_| true));
        runtime.finish_book(&mut player, false);
        if !editable {
            let network_id = written_book_network_id();
            let mut written = player.inventory.ledger().backing_stack(0).unwrap().clone();
            written.network_id = network_id;
            player
                .inventory
                .ledger_mut()
                .apply(&protocol::InventoryEvent::Slot(
                    protocol::InventorySlotEvent {
                        identity: protocol::SlotIdentity {
                            container: protocol::ContainerIdentity::window(
                                protocol::PLAYER_INVENTORY_WINDOW_ID,
                            ),
                            slot: 0,
                        },
                        stack: written,
                        storage_item: None,
                    },
                ));
        }
        let mut packets = Vec::new();
        assert!(
            runtime.open_held_book(&mut player, Some([0.0, 70.0, 0.0]), |packet| {
                packets.push(packet);
                true
            })
        );
        assert_eq!(packets.len(), 1);
        let transaction =
            protocol::test_support::transactions::observe_transaction(&packets[0]).unwrap();
        assert!(transaction.actions.is_empty());
        assert!(matches!(
            transaction.body,
            protocol::test_support::transactions::TransactionBody::ItemUse(_)
        ));
        assert_eq!(runtime.screen.book.as_ref().unwrap().editable, editable);
    }
}
