use std::collections::VecDeque;

mod reused_ids;

use protocol::{ContainerCloseEvent, ContainerIdentity, ContainerOpenEvent, InventoryEvent};

use super::*;
use crate::inventory_ledger::{
    GENERIC_STORAGE_WINDOW_TYPE, PlayerInventoryLedger,
    lifecycle_tests::{ledger_with_slot_zero, personal_open},
};

/// Produces reproducible scheduling and intent changes without a clock or a new dependency.
fn random_word(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Creates a server storage menu, including a replacement that reuses a just-closed ID.
fn storage_open(window_id: i32) -> InventoryEvent {
    InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(window_id),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    })
}

/// A server that suppresses duplicate personal opens instead of replying to each request.
struct Server {
    personal_open: bool,
    personal_window_id: i32,
    replace_next_storage_close: bool,
    closes: usize,
    storage: Option<i32>,
    replies: VecDeque<InventoryEvent>,
    duplicate_opens: usize,
}

impl Default for Server {
    /// Starts with closed windows and the protocol's personal inventory ID.
    fn default() -> Self {
        Self {
            personal_open: false,
            personal_window_id: protocol::PLAYER_INVENTORY_WINDOW_ID,
            replace_next_storage_close: false,
            closes: 0,
            storage: None,
            replies: VecDeque::new(),
            duplicate_opens: 0,
        }
    }
}

impl Server {
    /// Processes one admitted control immediately while allowing its reply to be delayed or lost.
    fn send(&mut self, ledger: &mut PlayerInventoryLedger, now: u64, lose_reply: bool) {
        if ledger.pending_batch().unwrap()
            == Some((protocol::repeated_container_open_warning_packet(), 1))
        {
            assert!(ledger.mark_transport_enqueued(now));
            return;
        }
        let Some(request) = ledger.pending_control() else {
            // Non-control traffic, including a world drop, may unblock a close.
            ledger.mark_transport_enqueued(now);
            return;
        };
        if let ControlRequest::Close(window) = request {
            assert!(!ledger.controls.close_outstanding(window.window_id));
        }
        let kind = request.kind();
        assert!(
            ledger.controls.state(kind).sent.is_none(),
            "never two outstanding requests of one kind"
        );
        let other = match kind {
            ControlKind::Personal => ControlKind::Storage,
            ControlKind::Storage => ControlKind::Personal,
        };
        let other_sent = ledger.controls.state(other).sent.map(|sent| sent.request);
        assert!(ledger.mark_transport_enqueued(now));
        assert_eq!(ledger.controls.state(kind).sent.unwrap().request, request);
        assert_eq!(
            ledger.controls.state(other).sent.map(|sent| sent.request),
            other_sent
        );
        assert!(
            ledger
                .pending_control()
                .is_none_or(|next| next.kind() != kind)
        );
        let mut replacement = None;
        let reply = match request {
            ControlRequest::Open { .. } => {
                if self.personal_open {
                    self.duplicate_opens += 1;
                    return;
                }
                self.personal_open = true;
                InventoryEvent::Open(personal_open(self.personal_window_id))
            }
            ControlRequest::Close(window) => {
                self.closes += 1;
                // The wire close addresses an ID, so a redundant close can hit its replacement.
                if window.window_id == self.personal_window_id {
                    self.personal_open = false;
                }
                if self.storage == Some(window.window_id) {
                    self.storage = None;
                    if self.replace_next_storage_close {
                        self.replace_next_storage_close = false;
                        self.storage = Some(window.window_id);
                        replacement = Some(storage_open(window.window_id));
                    }
                }
                InventoryEvent::Close(ContainerCloseEvent {
                    container: ContainerIdentity::window(window.window_id),
                    window_type: window.window_type,
                    server_initiated: false,
                })
            }
        };
        if !lose_reply {
            self.replies.push_back(reply);
        }
        if let Some(replacement) = replacement {
            self.replies.push_back(replacement);
        }
    }

    /// Delivers replies in wire order, including replies delayed beyond a local timeout.
    fn deliver(&mut self, ledger: &mut PlayerInventoryLedger) {
        if let Some(reply) = self.replies.pop_front() {
            ledger.apply(&reply);
        }
    }

    /// Opens storage independently of an outstanding personal request.
    fn open_storage(&mut self, ledger: &mut PlayerInventoryLedger, window_id: i32) {
        // Deliver earlier replacement menus before announcing the next server window.
        while self.replies.iter().any(|reply| {
            matches!(reply, InventoryEvent::Open(open)
                if open.window_type == GENERIC_STORAGE_WINDOW_TYPE)
        }) {
            self.deliver(ledger);
        }
        self.storage = Some(window_id);
        ledger.apply(&storage_open(window_id));
    }

    /// Queues an authoritative close without consuming any separately outstanding reply.
    fn server_close(&mut self) {
        if self.personal_open {
            self.personal_open = false;
            self.replies
                .push_back(InventoryEvent::Close(ContainerCloseEvent {
                    container: ContainerIdentity::window(self.personal_window_id),
                    window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
                    server_initiated: true,
                }));
        }
    }

    /// Gives the protocol fair delivery and deadlines until every remaining transition settles.
    fn settle(&mut self, ledger: &mut PlayerInventoryLedger, now: &mut u64) {
        for _ in 0..64 {
            while !self.replies.is_empty() {
                self.deliver(ledger);
            }
            self.send(ledger, *now, false);
            while !self.replies.is_empty() {
                self.deliver(ledger);
            }
            if self.replies.is_empty()
                && ledger.pending_batch().unwrap().is_none()
                && ledger.controls.personal.sent.is_none()
                && ledger.controls.storage.sent.is_none()
                && !ledger.controls.personal.reset_required
            {
                return;
            }
            *now += INVENTORY_REQUEST_TIMEOUT_MILLIS;
            ledger.poll_timeout(*now);
        }
        panic!("the latest intent must converge without reconnecting");
    }
}

#[test]
fn randomized_server_interleavings_converge_and_always_allow_reopening() {
    for seed in 1..=64 {
        let mut random = seed;
        let mut now = 0;
        let word = random_word(&mut random);
        reused_ids::covering_storage_close_interleaving(
            word & 1 != 0,
            word & 2 != 0,
            word & 4 != 0,
            word & 8 != 0,
        );
        let mut ledger = ledger_with_slot_zero();
        let mut server = Server::default();
        for _ in 0..512 {
            let word = random_word(&mut random);
            match word % 11 {
                0 => {
                    ledger.request_personal_open(42);
                }
                1 => ledger.request_personal_close(),
                2 => server.open_storage(&mut ledger, i32::try_from((word >> 8) % 3).unwrap() + 3),
                3 => ledger.request_storage_close(),
                4 => server.send(&mut ledger, now, word & 0x100 != 0),
                5 | 6 => server.deliver(&mut ledger),
                7 => {
                    now += INVENTORY_REQUEST_TIMEOUT_MILLIS;
                    let desired = ledger.personal_inventory_desired_open();
                    ledger.poll_timeout(now);
                    assert_eq!(ledger.personal_inventory_desired_open(), desired);
                }
                8 => server.server_close(),
                9 => {
                    if ledger.storage.is_none()
                        && !ledger.personal_inventory_desired_open()
                        && ledger
                            .displayed_stack(0)
                            .is_some_and(|stack| stack.count > 1)
                    {
                        let _ = ledger.begin_world_drop(0, Some(1));
                    }
                }
                10 => server.replace_next_storage_close = true,
                _ => unreachable!(),
            }
        }
        // Stop external commands and preserve the last desires while the server catches up.
        while !server.replies.is_empty() {
            server.deliver(&mut ledger);
        }
        server.replace_next_storage_close = false;
        let desired_personal = ledger.controls.personal.desired_open;
        let desired_storage = ledger.controls.storage.desired_open;
        server.settle(&mut ledger, &mut now);
        assert_eq!(
            ledger.controls.personal.confirmed.is_some(),
            desired_personal
        );
        assert_eq!(ledger.controls.storage.confirmed.is_some(), desired_storage);
        assert_eq!(server.personal_open, desired_personal);
        assert_eq!(server.storage.is_some(), desired_storage);
        assert_eq!(
            ledger
                .controls
                .storage
                .confirmed
                .map(|window| window.window_id),
            server.storage
        );

        ledger.request_personal_close();
        ledger.request_storage_close();
        server.settle(&mut ledger, &mut now);
        assert!(!server.personal_open);
        assert!(server.storage.is_none());
        assert!(ledger.controls.personal.confirmed.is_none());
        assert!(ledger.controls.storage.confirmed.is_none());
        for _ in 0..3 {
            assert!(ledger.request_personal_open(42));
            server.settle(&mut ledger, &mut now);
            assert!(server.personal_open);
            assert!(ledger.controls.personal.confirmed.is_some());
            assert!(ledger.personal_inventory_desired_open());
            ledger.request_personal_close();
            server.settle(&mut ledger, &mut now);
            assert!(!server.personal_open);
            assert!(ledger.controls.personal.confirmed.is_none());
        }
        assert_eq!(server.duplicate_opens, 0);
    }
}

#[test]
fn queued_storage_replacement_precedes_a_new_server_window() {
    let mut ledger = ledger_with_slot_zero();
    let mut server = Server::default();
    server.open_storage(&mut ledger, 3);
    ledger.request_storage_close();
    server.replace_next_storage_close = true;
    server.send(&mut ledger, 0, false);
    server.open_storage(&mut ledger, 4);
    while !server.replies.is_empty() {
        server.deliver(&mut ledger);
    }
    assert_eq!(
        ledger
            .controls
            .storage
            .confirmed
            .map(|window| window.window_id),
        server.storage
    );
    assert_eq!(ledger.storage.as_ref().unwrap().window_id, 4);

    ledger.request_storage_close();
    let mut now = 0;
    server.settle(&mut ledger, &mut now);
    assert!(server.storage.is_none());
    assert!(ledger.controls.storage.confirmed.is_none());
    assert!(ledger.request_personal_open(42));
    server.settle(&mut ledger, &mut now);
    assert!(server.personal_open);
    assert_eq!(server.duplicate_opens, 0);
}

#[test]
fn a_superseded_open_is_not_repeated_when_the_server_suppresses_duplicates() {
    let mut ledger = ledger_with_slot_zero();
    let mut server = Server::default();
    assert!(ledger.request_personal_open(42));
    server.send(&mut ledger, 0, false);
    server.open_storage(&mut ledger, 3);
    ledger.request_storage_close();
    server.send(&mut ledger, 0, false);
    // The chest close can be observed before the delayed personal-open reply.
    let chest_reply = server.replies.pop_back().unwrap();
    ledger.apply(&chest_reply);
    assert!(ledger.request_personal_open(42));
    server.send(&mut ledger, 0, false);
    assert_eq!(server.duplicate_opens, 0);
    server.deliver(&mut ledger);
    assert!(ledger.personal_inventory_desired_open());
    assert!(ledger.controls.personal.confirmed.is_some());
    assert!(ledger.pending_control().is_none());
    ledger.request_personal_close();
    let mut now = 0;
    server.settle(&mut ledger, &mut now);
    assert!(ledger.request_personal_open(42));
    server.settle(&mut ledger, &mut now);
    assert!(server.personal_open);
    assert_eq!(server.duplicate_opens, 0);
}

#[test]
fn an_open_reply_after_a_lost_reset_reply_cannot_confirm_a_closed_server() {
    let mut ledger = ledger_with_slot_zero();
    let mut server = Server::default();
    let mut now = 0;
    assert!(ledger.request_personal_open(42));
    server.send(&mut ledger, now, false);
    now += INVENTORY_REQUEST_TIMEOUT_MILLIS;
    ledger.poll_timeout(now);
    server.send(&mut ledger, now, true);
    assert!(!server.personal_open);
    now += INVENTORY_REQUEST_TIMEOUT_MILLIS;
    ledger.poll_timeout(now);
    server.deliver(&mut ledger);
    server.settle(&mut ledger, &mut now);
    assert!(server.personal_open);
    assert!(ledger.controls.personal.confirmed.is_some());
    assert!(ledger.personal_inventory_desired_open());
    assert_eq!(server.duplicate_opens, 0);
}

#[test]
fn a_timed_out_storage_reply_cannot_release_the_personal_slot() {
    let mut ledger = ledger_with_slot_zero();
    let mut server = Server::default();
    let mut now = 0;
    server.open_storage(&mut ledger, 3);
    ledger.request_storage_close();
    server.send(&mut ledger, now, false);
    now += INVENTORY_REQUEST_TIMEOUT_MILLIS;
    ledger.poll_timeout(now);
    assert!(ledger.request_personal_open(42));
    server.send(&mut ledger, now, false);
    let open_reply = server.replies.pop_back().unwrap();
    ledger.apply(&open_reply);
    ledger.request_personal_close();
    server.send(&mut ledger, now, false);
    assert!(ledger.request_personal_open(42));
    server.deliver(&mut ledger);
    assert!(matches!(
        ledger.controls.personal.sent.unwrap().request,
        ControlRequest::Close(_)
    ));
    assert!(ledger.pending_control().is_none());
    server.settle(&mut ledger, &mut now);
    assert!(server.personal_open);
    assert_eq!(server.duplicate_opens, 0);
}
