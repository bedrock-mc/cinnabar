//! Canonical container-address regressions (VPA-122): every wire path naming
//! one physical cell must resolve to exactly one canonical cell, distinct
//! surfaces can never collide, and unknown container identities are typed
//! counted skips that never mutate retained cells or end the session.

use std::sync::Arc;

use protocol::{
    CanonicalCell, ContainerIdentity, InventoryAuthority, InventoryContentEvent, InventoryEvent,
    InventorySlotEvent, ItemStackResponseEvent, NetworkItemStack, SlotIdentity, StackResponse,
    StackResponseContainer, StackResponseSlot, StackResponseStatus, project_container_cell,
};
use sha2::{Digest, Sha256};

use super::*;

fn stack(network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: -1,
        count: 1,
        nbt_digest: Sha256::digest([]).into(),
        block_runtime_id: 0,
        extra_data: Arc::from([]),
    }
}

fn identity(window_id: i32, slot_type: Option<u8>) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(window_id),
        slot_type,
        dynamic_id: None,
    }
}

fn content(container: ContainerIdentity, stacks: Vec<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: stacks.into(),
        storage_item: NetworkItemStack::empty(),
    })
}

fn slot_event(container: ContainerIdentity, slot: u16, stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity { container, slot },
        stack,
        storage_item: None,
    })
}

fn server_ledger(runtime: &mut UiRuntime) {
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
}

#[test]
fn cursor_slot_type_events_cannot_reach_the_hotbar_mirror_through_any_admission_path() {
    let mut runtime = UiRuntime::new(1);
    let mut slots = vec![NetworkItemStack::empty(); 36];
    slots[0] = stack(11);
    slots[8] = stack(19);
    runtime
        .enqueue_inventory_event(1, 1, content(identity(0, None), slots))
        .unwrap();
    // A cursor Slot event names the cursor container explicitly even though
    // its legacy window id is still 0.
    runtime
        .enqueue_inventory_event(1, 2, slot_event(identity(0, Some(59)), 0, stack(777)))
        .unwrap();
    // A cursor Content event likewise carries the named cursor container.
    runtime
        .enqueue_inventory_event(1, 3, content(identity(0, Some(59)), vec![stack(888)]))
        .unwrap();
    runtime.drain_pending_inventory();

    // The cursor events belong to the cursor cell only.
    assert_eq!(
        runtime
            .inventory_ledger()
            .cursor_stack()
            .map(|stack| stack.network_id),
        Some(888)
    );
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(0)
            .map(|stack| stack.network_id),
        Some(11)
    );
    // The hotbar mirror keeps every established cell untouched.
    assert_eq!(
        runtime
            .gameplay_hud()
            .hotbar_stack(0)
            .map(|stack| stack.network_id),
        Some(11)
    );
    assert_eq!(
        runtime
            .gameplay_hud()
            .hotbar_stack(8)
            .map(|stack| stack.network_id),
        Some(19)
    );
    assert_eq!(runtime.gameplay_hud().hotbar_stack(1), None);
}

#[test]
fn offhand_container_events_never_pollute_player_inventory_cells() {
    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut runtime);
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(0, None), 20, stack(20)));

    // An offhand Content event may arrive on the legacy player window while
    // naming the offhand container; it belongs to no player-inventory cell.
    runtime
        .inventory_ledger_mut()
        .apply(&content(identity(0, Some(34)), vec![stack(34)]));
    // An offhand Slot event on the same window is equally foreign.
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(0, Some(34)), 0, stack(340)));

    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(0)
            .map(|stack| stack.network_id),
        None
    );
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(20)
            .map(|stack| stack.network_id),
        Some(20)
    );

    // The session continues: ordinary player-inventory traffic still lands.
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(0, None), 0, stack(5)));
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(0)
            .map(|stack| stack.network_id),
        Some(5)
    );
}

#[test]
fn unknown_container_identities_skip_without_mutating_cells_or_ending_the_session() {
    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut runtime);

    // A well-formed Slot event naming an unroutable container is odd data:
    // skipped whole, never written into any player-inventory cell, and
    // counted as typed leniency.
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(0, Some(211)), 3, stack(999)));
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(-777, None), 4, stack(998)));
    assert_eq!(
        runtime.inventory_ledger().skipped_unknown_containers(),
        2,
        "both unrouted identities were counted"
    );
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(3)
            .map(|stack| stack.network_id),
        None
    );
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(4)
            .map(|stack| stack.network_id),
        None
    );

    // The session continues and later well-formed traffic still applies.
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(0, None), 3, stack(3)));
    assert_eq!(
        runtime.inventory_ledger().skipped_unknown_containers(),
        2,
        "routed traffic never inflates the skip counter"
    );
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(3)
            .map(|stack| stack.network_id),
        Some(3)
    );
}

#[test]
fn partial_window_zero_content_preserves_unseen_hotbar_mirror_cells() {
    let mut runtime = UiRuntime::new(1);
    let slots: Vec<NetworkItemStack> = (1..=36).map(stack).collect();
    runtime
        .enqueue_inventory_event(1, 1, content(identity(0, None), slots))
        .unwrap();
    runtime.drain_pending_inventory();

    // A partial authoritative rewrite states only the cells it carries; the
    // unseen mirror cells keep their last authoritative values.
    runtime
        .enqueue_inventory_event(1, 2, content(identity(0, None), vec![stack(50)]))
        .unwrap();
    runtime.drain_pending_inventory();

    assert_eq!(
        runtime
            .gameplay_hud()
            .hotbar_stack(0)
            .map(|stack| stack.network_id),
        Some(50)
    );
    assert_eq!(
        runtime
            .gameplay_hud()
            .hotbar_stack(8)
            .map(|stack| stack.network_id),
        Some(9)
    );
    assert_eq!(
        runtime
            .gameplay_hud()
            .hotbar_stack(1)
            .map(|stack| stack.network_id),
        Some(2)
    );
}

fn zero_count_correction(slot: u8) -> StackResponseSlot {
    StackResponseSlot {
        slot,
        hotbar_slot: slot,
        count: 0,
        item_stack_id: 0,
        custom_name: Arc::from(""),
        filtered_custom_name: Arc::from(""),
        durability_correction: 0,
    }
}

#[test]
fn accepted_response_corrections_resolve_through_the_same_canonical_projection() {
    // Premise: an accepted-response container carrying no decoded container
    // name still projects onto the player-inventory surface through its
    // legacy window id, exactly like Content and Slot ingress.
    let unnamed_window = ContainerIdentity {
        window_id: Some(0),
        slot_type: None,
        dynamic_id: None,
    };
    assert_eq!(
        project_container_cell(&unnamed_window, 3),
        Some(CanonicalCell::PlayerInventory(3))
    );

    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut runtime);
    for (slot, network_id) in [(3, 33), (4, 44), (5, 55)] {
        runtime.inventory_ledger_mut().apply(&slot_event(
            identity(0, None),
            slot,
            stack(network_id),
        ));
    }
    // One in-flight gesture so an accepted response can reconcile at all.
    let request = runtime.inventory_ledger_mut().begin_click(5).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Response(ItemStackResponseEvent {
            responses: Arc::from([StackResponse {
                status: StackResponseStatus::Accepted,
                request_id: request,
                containers: Arc::from([
                    // An unnamed legacy-window container corrects the same
                    // player cell the equivalent Slot event addresses.
                    StackResponseContainer {
                        container: unnamed_window,
                        slots: Arc::from([zero_count_correction(3)]),
                    },
                    // An unknown container name takes precedence over its
                    // window id and resolves onto no retained cell: skipped
                    // whole and counted, never a mutation.
                    StackResponseContainer {
                        container: identity(0, Some(211)),
                        slots: Arc::from([zero_count_correction(4)]),
                    },
                ]),
            }]),
        }));

    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(3)
            .map(|stack| stack.network_id),
        None,
        "the unnamed-window correction cleared the same canonical cell"
    );
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(4)
            .map(|stack| stack.network_id),
        Some(44),
        "the unrouted correction mutated nothing"
    );
    assert_eq!(
        runtime.inventory_ledger().skipped_unknown_containers(),
        1,
        "exactly the unrouted correction was counted"
    );

    // The session continues: later well-formed traffic still applies.
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(0, None), 4, stack(404)));
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(4)
            .map(|stack| stack.network_id),
        Some(404)
    );
}
