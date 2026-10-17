use std::sync::Arc;

use protocol::{ContainerIdentity, InventoryContentEvent, InventoryEvent};

use super::*;
use crate::{InventoryIngressError, InventorySession};

/// Addresses a retained bundle without assigning a legacy window.
fn dynamic(id: u32) -> ContainerIdentity {
    ContainerIdentity {
        window_id: None,
        slot_type: Some(CONTAINER_NAME_DYNAMIC),
        dynamic_id: Some(id),
    }
}

/// Restates one bundle with a known stack identity.
fn content(id: u32) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: dynamic(id),
        slots: Arc::from([NetworkItemStack {
            network_id: 6,
            stack_network_id: 81,
            count: 3,
            ..Default::default()
        }]),
        storage_item: NetworkItemStack::empty(),
    })
}

/// Creates one ordered registry retirement event.
fn cleanup(containers: &[ContainerIdentity]) -> InventoryEvent {
    InventoryEvent::RegistryCleanup(Arc::from(containers))
}

#[test]
fn container_registry_cleanup_removes_only_the_matching_bundle() {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.apply(&content(7));
    ledger.apply(&content(8));
    let other = ledger.bundle_contents(8).unwrap().to_vec();
    let mut extra_data = vec![255, 255, 1, 10, 0, 0, 3, 9, 0];
    extra_data.extend(b"bundle_id");
    extra_data.extend(7_i32.to_le_bytes());
    extra_data.push(0);
    let player = NetworkItemStack {
        network_id: 6,
        stack_network_id: 90,
        count: 1,
        extra_data: Arc::from(extra_data),
        ..Default::default()
    };
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
            slot: 0,
        },
        stack: player.clone(),
        storage_item: None,
    }));
    let remove = cleanup(&[dynamic(7), dynamic(7), dynamic(999)]);
    ledger.apply(&remove);
    assert!(ledger.bundle_contents(7).is_none());
    assert_eq!(ledger.bundle_contents(8), Some(other.as_slice()));
    assert_eq!(ledger.displayed_stack(0), Some(&player));
    assert_eq!(ledger.bundle_id_at(InventoryTarget::Player(0)), Some(7));
    ledger.apply(&remove);
    assert!(ledger.bundle_contents(7).is_none());
    assert_eq!(ledger.bundle_contents(8), Some(other.as_slice()));
    ledger.apply(&content(7));
    assert!(ledger.bundle_contents(7).is_some(), "an ID can be reused");
}

#[test]
fn container_registry_cleanup_does_not_alias_other_full_names() {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.apply(&content(7));
    ledger.apply(&content(0));
    let before = ledger.bundles.clone();
    ledger.apply(&cleanup(&[
        ContainerIdentity {
            dynamic_id: None,
            ..dynamic(7)
        },
        ContainerIdentity {
            slot_type: Some(u8::MAX),
            ..dynamic(7)
        },
        ContainerIdentity {
            slot_type: Some(protocol::CONTAINER_NAME_INVENTORY),
            ..dynamic(7)
        },
    ]));
    assert_eq!(ledger.bundles, before);
    assert_eq!(ledger.skipped_unknown_containers(), 3);
    ledger.apply(&cleanup(&[dynamic(0)]));
    assert!(ledger.bundle_contents(0).is_none());
    assert!(ledger.bundle_contents(7).is_some());
}

#[test]
fn container_registry_cleanup_reclaims_capacity_and_session_reset_clears_contents() {
    let mut ledger = PlayerInventoryLedger::default();
    for id in 0..MAX_BUNDLES as u32 {
        ledger.apply(&content(id));
    }
    let next = MAX_BUNDLES as u32;
    ledger.apply(&content(next));
    assert!(ledger.bundle_contents(next).is_none());
    for id in next..next * 3 {
        ledger.apply(&cleanup(&[dynamic(id - next)]));
        ledger.apply(&content(id));
        assert!(ledger.bundle_contents(id).is_some());
        assert_eq!(ledger.bundles.len(), MAX_BUNDLES);
    }
    ledger.begin_session(2);
    assert!(ledger.bundles.is_empty());
    ledger.apply(&content(7));
    assert!(ledger.bundle_contents(7).is_some());
}

#[test]
fn container_registry_cleanup_obeys_session_fifo_and_rejects_stale_sessions() {
    let mut session = InventorySession::new(1);
    for (sequence, event) in [
        (1, content(7)),
        (2, content(8)),
        (3, cleanup(&[dynamic(7)])),
        (4, content(7)),
    ] {
        session.enqueue_inventory_event(1, sequence, event).unwrap();
    }
    session.apply_next().unwrap();
    session.apply_next().unwrap();
    assert!(session.ledger().bundle_contents(7).is_some());
    session.apply_next().unwrap();
    assert!(session.ledger().bundle_contents(7).is_none());
    assert!(session.ledger().bundle_contents(8).is_some());
    session.apply_next().unwrap();
    assert!(session.ledger().bundle_contents(7).is_some());
    session
        .enqueue_inventory_event(1, 5, cleanup(&[dynamic(7)]))
        .unwrap();
    session.begin_session(2);
    assert!(session.apply_next().is_none());
    assert!(session.ledger().bundle_contents(7).is_none());
    session.enqueue_inventory_event(2, 1, content(7)).unwrap();
    session.drain_pending_inventory();
    assert!(matches!(
        session.enqueue_inventory_event(1, 6, cleanup(&[dynamic(7)])),
        Err(InventoryIngressError::WrongSession { .. })
    ));
    assert!(session.ledger().bundle_contents(7).is_some());
}
