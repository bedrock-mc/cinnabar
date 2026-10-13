use protocol::{ContainerCloseEvent, ContainerIdentity, ContainerOpenEvent, InventoryEvent};

use super::*;
use crate::inventory_ledger::{
    GENERIC_STORAGE_WINDOW_TYPE, PERSONAL_INVENTORY_WINDOW_TYPE,
    lifecycle_tests::{ledger_with_slot_zero, personal_open},
};

/// Checks that lifecycle routing preserves the modeled requests, ownership and deadlines.
fn assert_controls(actual: &SentControls, expected: &SentControls) {
    assert_eq!(actual.entries.len(), expected.entries.len());
    for (actual, expected) in actual.entries.iter().zip(&expected.entries) {
        assert_eq!(actual.request, expected.request);
        assert_eq!(actual.superseded, expected.superseded);
        assert_eq!(actual.deadline_millis, expected.deadline_millis);
    }
}

/// Sends an authoritative command for the oldest close without answering any client request.
pub(super) fn server_close(queue: &mut SentControls, word: u64) {
    let Some(close) = queue.entries.iter().find_map(|sent| match sent.request {
        ControlRequest::Close(close) => Some(close),
        ControlRequest::PersonalOpen(_) => None,
    }) else {
        return;
    };
    let mut ledger = PlayerInventoryLedger {
        sent_controls: queue.clone(),
        ..PlayerInventoryLedger::default()
    };
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(close.window_id),
        window_type: close.window_type,
        server_initiated: true,
    }));
    if word & 0x100 != 0 {
        // Repeated commands also leave the outstanding reply untouched.
        ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(close.window_id),
            window_type: close.window_type,
            server_initiated: true,
        }));
    }
    assert_controls(&ledger.sent_controls, queue);
    *queue = ledger.sent_controls;
}

impl Transcript {
    /// Compares a real transport admission with an independently recorded expected send.
    fn admit(&mut self, ledger: &mut PlayerInventoryLedger, request: ControlRequest, now: u64) {
        let mut expected = ledger.sent_controls.clone();
        let stream = usize::from(request.kind() == ControlKind::Close);
        self.send(&mut expected, request, now, stream);
        assert!(ledger.mark_transport_enqueued(now));
        assert_controls(&ledger.sent_controls, &expected);
    }

    /// Routes an actual wire reply and checks that only its oldest modeled receipt was consumed.
    fn apply_reply(
        &mut self,
        ledger: &mut PlayerInventoryLedger,
        event: InventoryEvent,
        stream: usize,
    ) {
        let mut expected = ledger.sent_controls.clone();
        self.reply(&mut expected, stream);
        ledger.apply(&event);
        assert_controls(&ledger.sent_controls, &expected);
    }
}

/// Interleaves superseded cleanup, an unsent world drop and a cancelled newer personal open.
pub(super) fn world_drop_blocked_close(
    queue: &mut SentControls,
    transcript: &mut Transcript,
    now: u64,
    word: u64,
) {
    assert!(queue.entries.is_empty());
    let mut ledger = ledger_with_slot_zero();
    // Keep model generations distinct from earlier synthetic requests.
    ledger.next_open_generation = u64::try_from(transcript.requests.len()).unwrap() + 1;
    let personal_id = i32::try_from((word >> 8) % 4).unwrap() + 1;
    let storage_id = personal_id + 1;
    assert!(ledger.request_personal_open(42));
    let old = ledger.personal.unwrap().generation();
    let receipt = transcript.requests.len();
    transcript.admit(&mut ledger, ControlRequest::PersonalOpen(old), now);
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(storage_id),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    transcript.superseded.insert(receipt);
    let storage = ledger.storage_generation().unwrap();
    ledger.request_storage_close();
    transcript.admit(
        &mut ledger,
        ControlRequest::Close(PendingClose {
            window_id: storage_id,
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            owner: PendingCloseOwner::Storage(storage),
        }),
        now,
    );
    transcript.apply_reply(
        &mut ledger,
        InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(storage_id),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            server_initiated: false,
        }),
        1,
    );

    ledger.begin_world_drop(0, Some(1)).unwrap();
    assert!(ledger.request_personal_open(42));
    let current = ledger.personal.unwrap().generation();
    transcript.apply_reply(
        &mut ledger,
        InventoryEvent::Open(personal_open(personal_id)),
        0,
    );
    assert!(!ledger.close_ready());
    transcript.admit(&mut ledger, ControlRequest::PersonalOpen(current), now);
    ledger.request_personal_close();
    transcript.apply_reply(
        &mut ledger,
        InventoryEvent::Open(personal_open(personal_id)),
        0,
    );

    let expected = ledger.sent_controls.clone();
    assert!(
        ledger.mark_transport_enqueued(now),
        "the world drop leaves before the close"
    );
    assert_controls(&ledger.sent_controls, &expected);
    transcript.admit(
        &mut ledger,
        ControlRequest::Close(PendingClose {
            window_id: personal_id,
            window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
            owner: PendingCloseOwner::Personal(current),
        }),
        now,
    );
    assert!(matches!(
        ledger.personal,
        Some(PersonalWindow::Closing { admitted: true, .. })
    ));
    transcript.apply_reply(
        &mut ledger,
        InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(personal_id),
            window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
            server_initiated: false,
        }),
        1,
    );
    assert!(ledger.personal.is_none());
    assert!(ledger.request_personal_open(42));
    assert!(ledger.pending_control_packet().unwrap().is_some());
    *queue = ledger.sent_controls;
}
