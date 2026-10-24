//! Regression tests for optimistic counts after bulk reads.

use super::*;
use crate::menu::InboxItem;

/// Creates a bulk-read baseline followed by a larger, partially loaded service page.
fn refreshed_bulk_read() -> MenuFeeds {
    let mut menu = MenuFeeds {
        home: MenuHome {
            inbox: vec![InboxItem {
                instance_id: "old".into(),
                category: "News".into(),
                unread: true,
                ..Default::default()
            }],
            inbox_counts: [(0, 30)].into(),
            inbox_unread: 30,
            ..Default::default()
        },
        ..Default::default()
    };
    menu.inbox_state.reconcile(&mut menu.home);
    let mut service = menu.home.clone();
    menu.activate_inbox(Action::MarkAllRead);
    service.inbox_counts.insert(0, 40);
    service
        .inbox
        .extend(["new-one", "new-two"].map(|identity| InboxItem {
            instance_id: identity.into(),
            category: "News".into(),
            unread: true,
            ..Default::default()
        }));
    menu.home = service;
    menu.inbox_state.reconcile(&mut menu.home);
    assert_eq!(menu.home.inbox_unread, 10);
    menu
}

#[test]
fn opening_new_messages_after_bulk_read_preserves_unloaded_unread_counts() {
    let mut menu = refreshed_bulk_read();
    for (index, expected) in [(1, 9), (1, 9), (2, 8)] {
        menu.activate_inbox(Action::Open(index));
        assert_eq!(menu.home.inbox_unread, expected);
        assert_eq!(menu.home.inbox_counts[&0], expected);
    }
}

#[test]
fn deleting_new_messages_after_bulk_read_preserves_unloaded_unread_counts() {
    let mut menu = refreshed_bulk_read();
    for expected in [9, 8] {
        menu.activate_inbox(Action::Delete(1));
        menu.activate_inbox(Action::ConfirmDelete);
        assert_eq!(menu.home.inbox_unread, expected);
        assert_eq!(menu.home.inbox_counts[&0], expected);
    }
    menu.activate_inbox(Action::Open(0));
    assert_eq!(menu.home.inbox_unread, 8);
}

/// Only an actual service-session replacement retires optimistic interactions for reused IDs.
#[test]
fn native_messaging_session_replacement_retires_old_optimistic_inbox_actions() {
    let mut menu = refreshed_bulk_read();
    let mut original = menu.home.clone();
    original.message_session = Some("original-session".into());
    original.inbox[0].unread = true;
    original.inbox_counts.insert(0, 1);
    original.inbox.truncate(1);
    menu.home = original.clone();
    menu.inbox_state.reconcile(&mut menu.home);
    menu.activate_inbox(Action::Open(0));
    assert!(!menu.home.inbox[0].unread);
    assert_eq!(menu.inbox_state.pending.len(), 1);
    menu.home = original.clone();
    menu.inbox_state.reconcile(&mut menu.home);
    assert!(
        !menu.home.inbox[0].unread,
        "same-session polling must retain the read"
    );
    assert_eq!(menu.inbox_state.pending.len(), 1);
    original.message_session = Some("replacement-session".into());
    menu.home = original;
    menu.inbox_state.reconcile(&mut menu.home);
    assert!(
        menu.home.inbox[0].unread,
        "old read must not modify a new session's row"
    );
    assert!(menu.inbox_state.pending.is_empty());
    assert!(menu.inbox_state.opened.is_none());
}
