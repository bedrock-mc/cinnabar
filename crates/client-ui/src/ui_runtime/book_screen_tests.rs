//! Book commits retain edits while the transport is full.
use super::*;

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
