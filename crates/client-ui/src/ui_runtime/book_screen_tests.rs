//! Book commits retain edits while the transport is full.
use super::*;

#[path = "book_screen_tests/backing.rs"]
mod backing;

/// Allocates this fixture's written-book ID after the protocol test registry.
fn written_book_network_id() -> i32 {
    protocol::vanilla_item_registry()
        .iter()
        .map(|entry| entry.network_id)
        .max()
        .unwrap()
        + 1
}

/// Publishes a fresh writable book without requiring a resource carrier.
fn held_book() -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = UiRuntime::new(1);
    let mut entries = protocol::vanilla_item_registry().to_vec();
    let mut written = entries
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:writable_book")
        .unwrap()
        .clone();
    written.identifier = "minecraft:written_book".into();
    written.network_id = written_book_network_id();
    entries.push(written);
    let network_id = entries
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:writable_book")
        .unwrap()
        .network_id;
    let ledger = player.inventory.ledger_mut();
    ledger.apply(&protocol::InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    ledger.apply_registry(&protocol::ItemRegistryEvent {
        entries: entries.into(),
    });
    ledger.apply(&protocol::InventoryEvent::Content(
        protocol::InventoryContentEvent {
            container: protocol::ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
            slots: vec![protocol::NetworkItemStack {
                network_id,
                count: 1,
                stack_network_id: 9,
                ..protocol::NetworkItemStack::empty()
            }]
            .into(),
            storage_item: protocol::NetworkItemStack::empty(),
        },
    ));
    (player, runtime)
}

#[test]
fn book_opening_waits_for_admission_and_initializes_only_once() {
    let (mut player, mut runtime) = held_book();
    let original = player
        .inventory
        .ledger()
        .displayed_stack(0)
        .unwrap()
        .clone();
    assert!(runtime.open_held_book(&mut player, Some([0.0, 70.0, 0.0]), |_| false));
    assert!(runtime.screen.book.is_none());
    assert_eq!(
        player.inventory.ledger().displayed_stack(0),
        Some(&original)
    );
    let mut packets = Vec::new();
    assert!(
        runtime.open_held_book(&mut player, Some([0.0, 70.0, 0.0]), |packet| {
            packets.push(packet);
            true
        })
    );
    assert!(runtime.screen.book.as_ref().unwrap().editable);
    let initialized = player
        .inventory
        .ledger()
        .displayed_stack(0)
        .unwrap()
        .clone();
    assert!(!protocol::book_needs_init(&initialized.extra_data));
    runtime.finish_book(&mut player, false);
    assert!(
        runtime.open_held_book(&mut player, Some([0.0, 70.0, 0.0]), |packet| {
            packets.push(packet);
            true
        })
    );
    assert!(
        protocol::test_support::transactions::observe_transaction(&packets[1])
            .unwrap()
            .actions
            .is_empty(),
        "an initialized book uses the item without another initialization"
    );
    runtime.screen.book.as_mut().unwrap().type_text("saved");
    runtime.finish_book(&mut player, false);
    packets.push(runtime.take_client_packet().unwrap());
    assert!(runtime.take_client_packet().is_none());
    assert!(protocol::test_support::transactions::observe_transaction(&packets[0]).is_some());
    assert_eq!(
        packets[2].kind(),
        protocol::book_edit_packet(0, &BookEdit::DeletePage { page: 1 })
            .unwrap()
            .kind()
    );
    assert_eq!(
        player.inventory.ledger().displayed_stack(0),
        Some(&initialized)
    );
}

#[test]
fn fresh_book_does_not_open_without_a_pose_or_while_its_slot_has_a_pending_drop() {
    let (mut player, mut runtime) = held_book();
    assert!(runtime.open_held_book(&mut player, None, |_| panic!("missing pose must not send")));
    assert!(runtime.screen.book.is_none());
    player
        .inventory
        .ledger_mut()
        .begin_world_drop(0, None)
        .unwrap();
    assert!(
        !runtime.open_held_book(&mut player, Some([0.0, 70.0, 0.0]), |_| panic!(
            "pending drop must not send"
        ))
    );
    assert!(runtime.screen.book.is_none());
}

#[test]
fn an_unencodable_page_preserves_the_editor_and_publishes_no_partial_commit() {
    for sign in [false, true] {
        let mut runtime = UiRuntime::new(1);
        let mut book = BookState::new(
            BookSource::Held(0),
            vec!["old".into(), "old".into()],
            true,
            "Title".into(),
            "Author".into(),
        );
        book.pages[0] = "valid change".into();
        book.pages[1] = "🦀".repeat(MAX_BOOK_PAGE_BYTES / '🦀'.len_utf8() + 1);
        let pages = book.pages.clone();
        runtime.open_book(book);
        assert!(!runtime.commit_book(sign));
        assert!(runtime.inventory_open);
        assert_eq!(runtime.screen.book.as_ref().unwrap().pages, pages);
        assert!(runtime.take_client_packet().is_none());

        runtime.screen.book.as_mut().unwrap().pages[1] = "repaired".into();
        assert!(runtime.commit_book(sign));
        for (page, text) in [(0, "valid change"), (1, "repaired")] {
            assert_eq!(
                runtime.take_client_packet(),
                protocol::book_edit_packet(
                    0,
                    &BookEdit::ReplacePage {
                        page,
                        text: text.into()
                    }
                )
            );
        }
        if sign {
            assert_eq!(
                runtime.take_client_packet(),
                protocol::book_edit_packet(
                    0,
                    &BookEdit::Finalize {
                        title: "Title".into(),
                        author: "Author".into(),
                        xuid: String::new()
                    }
                )
            );
        }
        assert!(runtime.screen.book.is_none());
        assert!(runtime.take_client_packet().is_none());
    }
}

#[test]
fn an_unencodable_signature_does_not_publish_valid_page_edits() {
    let mut runtime = UiRuntime::new(1);
    let mut book = BookState::new(
        BookSource::Held(0),
        vec!["old".into()],
        true,
        "Title".into(),
        "a".repeat(MAX_BOOK_PAGE_BYTES + 1),
    );
    book.type_text(" change");
    runtime.open_book(book);
    assert!(!runtime.commit_book(true));
    assert_eq!(runtime.screen.book.as_ref().unwrap().pages[0], "old change");
    assert!(runtime.take_client_packet().is_none());
    assert!(runtime.commit_book(false));
    assert_eq!(
        runtime.take_client_packet(),
        protocol::book_edit_packet(
            0,
            &BookEdit::ReplacePage {
                page: 0,
                text: "old change".into()
            }
        )
    );
    assert!(runtime.take_client_packet().is_none());
}

#[test]
fn book_commit_retains_edits_until_the_whole_batch_can_be_queued() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    for _ in 0..MAX_QUEUED_CLIENT_PACKETS {
        runtime.queue_client_packet(
            protocol::book_edit_packet(0, &BookEdit::DeletePage { page: 1 }).unwrap(),
        );
    }
    let mut book = BookState::new(
        BookSource::Held(0),
        vec!["old".into()],
        true,
        "Title".into(),
        "Author".into(),
    );
    book.type_text(" edit");
    runtime.open_book(book);
    runtime.finish_book(&mut player_runtime, true);
    assert!(runtime.screen.book.is_none());
    assert!(!runtime.inventory_open);
    let mut count = 0;
    while runtime.take_client_packet().is_some() {
        count += 1;
    }
    assert_eq!(
        count,
        MAX_QUEUED_CLIENT_PACKETS + 2,
        "edit and finalize must both survive backlog"
    );
}

#[test]
fn large_book_commit_drains_in_order_without_losing_the_final_packet() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut book = BookState::new(
        BookSource::Held(0),
        vec![String::new(); MAX_BOOK_PAGES],
        true,
        "Title".into(),
        "Author".into(),
    );
    book.pages.fill("edit".into());
    runtime.open_book(book);
    runtime.finish_book(&mut player_runtime, true);
    let mut count = 0;
    while runtime.take_client_packet().is_some() {
        count += 1;
    }
    assert_eq!(count, MAX_BOOK_PAGES + 1);
    assert!(runtime.screen.book.is_none());
}

#[test]
fn a_second_book_commit_keeps_its_editor_until_the_first_commit_drains() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    for _ in 0..2 {
        let mut book = BookState::new(
            BookSource::Held(0),
            vec!["old".into()],
            true,
            String::new(),
            String::new(),
        );
        book.type_text(" edit");
        runtime.open_book(book);
        runtime.finish_book(&mut player_runtime, false);
    }
    assert!(runtime.inventory_open);
    assert_eq!(runtime.screen.book.as_ref().unwrap().pages[0], "old edit");
    assert!(runtime.take_client_packet().is_some());
    runtime.finish_book(&mut player_runtime, false);
    assert!(!runtime.inventory_open);
    assert!(runtime.take_client_packet().is_some());
    assert!(runtime.take_client_packet().is_none());
}

#[test]
fn a_retired_session_discards_its_pending_book_and_screen_packets() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.queue_client_packet(
        protocol::book_edit_packet(0, &BookEdit::DeletePage { page: 1 }).unwrap(),
    );
    let mut book = BookState::new(
        BookSource::Held(0),
        vec!["old".into()],
        true,
        String::new(),
        String::new(),
    );
    book.type_text(" edit");
    runtime.open_book(book);
    runtime.finish_book(&mut player_runtime, false);
    player_runtime.begin_session(2);
    runtime.begin_session(2);
    assert!(runtime.take_client_packet().is_none());
}

/// A server restatement of an initialized book keeps its client-owned editor visible.
#[test]
fn review_regression_inventory_updates_preserve_the_book_screen() {
    let (mut player, mut runtime) = held_book();
    assert!(runtime.open_held_book(&mut player, Some([0.0, 70.0, 0.0]), |_| true));
    let initialized = player
        .inventory
        .ledger()
        .displayed_stack(0)
        .unwrap()
        .clone();
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            1,
            protocol::InventoryEvent::Slot(protocol::InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: protocol::ContainerIdentity::window(0),
                    slot: 0,
                },
                stack: initialized,
                storage_item: None,
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player);
    assert!(runtime.inventory_open());
    assert!(runtime.screen.book.as_ref().unwrap().editable);
    assert!(!player.inventory.ledger().personal_inventory_desired_open());
}
