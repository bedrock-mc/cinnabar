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
use crate::ui_runtime::inventory_ledger::{GENERIC_STORAGE_WINDOW_TYPE, SMALL_STORAGE_SLOT_COUNT};

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
fn combined_player_name_on_a_foreign_window_is_skipped_by_ledger_and_hud() {
    let foreign = identity(
        6,
        Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
    );
    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut runtime);
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(0, None), 3, stack(3)));
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(foreign, 3, stack(63)));
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(3)
            .map(|stack| stack.network_id),
        Some(3),
    );
    assert_eq!(runtime.inventory_ledger().skipped_unknown_containers(), 1);

    runtime
        .enqueue_inventory_event(1, 1, slot_event(identity(0, None), 3, stack(3)))
        .unwrap();
    runtime
        .enqueue_inventory_event(1, 2, content(foreign, vec![stack(60)]))
        .unwrap();
    runtime.drain_pending_inventory();
    assert_eq!(
        runtime
            .gameplay_hud()
            .hotbar_stack(0)
            .map(|stack| stack.network_id),
        None,
    );
    assert_eq!(
        runtime
            .gameplay_hud()
            .diagnostics()
            .unknown_container_events,
        1
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

/// The pinned gophertunnel `InventorySlot` fixture (`tools/fixturegen`)
/// writes exactly this shape: legacy window id 0 carrying a full container
/// name whose byte is `InventoryContainer` (29). Prior admission applied
/// such events to player cells in both the ledger and the HUD hotbar
/// mirror; the canonical projection must keep routing them there.
#[test]
fn fixture_named_inventory_slot_updates_land_in_player_cells_in_ledger_and_hud() {
    const INVENTORY_CONTAINER_NAME: u8 = 29;

    // Ledger admission.
    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut runtime);
    runtime.inventory_ledger_mut().apply(&slot_event(
        identity(0, Some(INVENTORY_CONTAINER_NAME)),
        4,
        stack(29),
    ));
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(4)
            .map(|stack| stack.network_id),
        Some(29),
        "the fixture-shaped Slot event lands in canonical player cell 4"
    );
    assert_eq!(
        runtime.inventory_ledger().skipped_unknown_containers(),
        0,
        "routed fixture traffic never counts as leniency"
    );

    // HUD ingestion through the production queue.
    let mut hud = UiRuntime::new(1);
    hud.enqueue_inventory_event(
        1,
        1,
        slot_event(identity(0, Some(INVENTORY_CONTAINER_NAME)), 4, stack(29)),
    )
    .unwrap();
    hud.drain_pending_inventory();
    assert_eq!(
        hud.gameplay_hud()
            .hotbar_stack(4)
            .map(|stack| stack.network_id),
        Some(29),
        "the fixture-shaped Slot event mirrors into hotbar cell 4"
    );
    assert_eq!(hud.gameplay_hud().diagnostics().unknown_container_events, 0);

    // A named full-content rewrite rides the same alias.
    let slots: Vec<NetworkItemStack> = (1..=36).map(stack).collect();
    hud.enqueue_inventory_event(
        1,
        2,
        content(identity(0, Some(INVENTORY_CONTAINER_NAME)), slots),
    )
    .unwrap();
    hud.drain_pending_inventory();
    assert!(hud.gameplay_hud().hotbar_known());
    assert_eq!(
        hud.gameplay_hud()
            .hotbar_stack(8)
            .map(|stack| stack.network_id),
        Some(9)
    );

    // The prior window-0 exclusions still hold: generic-storage-named
    // traffic belongs to no player cell regardless of its legacy window id.
    // With no storage window open it is counted leniency (Minor 6).
    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut runtime);
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(0, None), 4, stack(4)));
    runtime.inventory_ledger_mut().apply(&slot_event(
        identity(0, Some(protocol::CONTAINER_NAME_LEVEL_ENTITY)),
        4,
        stack(777),
    ));
    runtime.inventory_ledger_mut().apply(&slot_event(
        identity(6, Some(protocol::CONTAINER_NAME_LEVEL_ENTITY)),
        4,
        stack(778),
    ));
    assert_eq!(
        runtime
            .inventory_ledger()
            .displayed_stack(4)
            .map(|stack| stack.network_id),
        Some(4),
        "storage-named events never reach player cells"
    );
    assert_eq!(
        runtime.inventory_ledger().skipped_unknown_containers(),
        2,
        "both storage events with no matching open window were counted"
    );

    // With a matching open window the same surface lands in storage, while a
    // wrong-window storage identity stays the prior silent targeted drop.
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
            container: ContainerIdentity::window(4),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            position: [0, 0, 0],
            runtime_entity_id: 1,
        }));
    runtime.inventory_ledger_mut().apply(&content(
        ContainerIdentity {
            window_id: Some(4),
            slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
            dynamic_id: Some(9),
        },
        vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT],
    ));
    runtime.inventory_ledger_mut().apply(&slot_event(
        identity(4, Some(protocol::CONTAINER_NAME_INVENTORY)),
        3,
        stack(33),
    ));
    assert_eq!(
        runtime
            .inventory_ledger()
            .storage_stack(3)
            .map(|stack| stack.network_id),
        None,
        "the player-inventory alias never reaches an open storage window"
    );
    assert_eq!(
        runtime.inventory_ledger().skipped_unknown_containers(),
        3,
        "the off-window alias resolved onto no retained cell and was counted"
    );
    runtime.inventory_ledger_mut().apply(&slot_event(
        identity(9, Some(protocol::CONTAINER_NAME_LEVEL_ENTITY)),
        3,
        stack(99),
    ));
    assert_eq!(
        runtime
            .inventory_ledger()
            .storage_stack(3)
            .map(|stack| stack.network_id),
        None,
        "a wrong-window storage identity is dropped without mutation"
    );
    assert_eq!(
        runtime.inventory_ledger().skipped_unknown_containers(),
        3,
        "the targeted mismatch drop stays distinct from unrouted leniency"
    );
}

/// Prior admission matched an open generic-storage window through its bare
/// legacy window id alone whenever a Slot update carried no decoded
/// container name at all. That reach is restored through the same
/// projection boundary instead of being narrowed to `None`.
#[test]
fn bare_window_slot_updates_still_reach_the_open_generic_storage_window() {
    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut runtime);
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
            container: ContainerIdentity::window(4),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            position: [0, 0, 0],
            runtime_entity_id: 1,
        }));
    runtime.inventory_ledger_mut().apply(&content(
        ContainerIdentity {
            window_id: Some(4),
            slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
            dynamic_id: Some(9),
        },
        vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT],
    ));

    // A bare-window Slot update — the optional container name absent on the
    // wire — addresses the same open window exactly as before.
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(4, None), 5, stack(55)));
    assert_eq!(
        runtime
            .inventory_ledger()
            .storage_stack(5)
            .map(|stack| stack.network_id),
        Some(55),
        "bare-window updates reach the open generic-storage window like before"
    );
    assert_eq!(runtime.inventory_ledger().skipped_unknown_containers(), 0);

    // A bare window id that matches no open window stays unrouted leniency.
    runtime
        .inventory_ledger_mut()
        .apply(&slot_event(identity(9, None), 5, stack(99)));
    assert_eq!(
        runtime
            .inventory_ledger()
            .storage_stack(5)
            .map(|stack| stack.network_id),
        Some(55)
    );
    assert_eq!(runtime.inventory_ledger().skipped_unknown_containers(), 1);

    // Without any open window the same bare shape cannot land anywhere and
    // is counted; the session keeps accepting routed traffic.
    let mut closed = UiRuntime::new(1);
    server_ledger(&mut closed);
    closed
        .inventory_ledger_mut()
        .apply(&slot_event(identity(4, None), 5, stack(55)));
    assert_eq!(closed.inventory_ledger().skipped_unknown_containers(), 1);
    closed
        .inventory_ledger_mut()
        .apply(&slot_event(identity(0, None), 5, stack(5)));
    assert_eq!(
        closed
            .inventory_ledger()
            .displayed_stack(5)
            .map(|stack| stack.network_id),
        Some(5)
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
    // Premise: an accepted-response container decodes without any window id
    // at all (`normalize_response` emits only the decoded container name),
    // so the reachable converged shape is the combined player-inventory
    // name carrying `window_id: None`. It must project onto exactly the
    // canonical cell the equivalent named Slot ingress addresses.
    let named_no_window = ContainerIdentity {
        window_id: None,
        slot_type: Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
        dynamic_id: Some(7),
    };
    assert_eq!(
        project_container_cell(&named_no_window, 3),
        Some(CanonicalCell::PlayerInventory(3))
    );

    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut runtime);
    for (slot, network_id) in [(3, 33), (4, 44), (5, 55)] {
        runtime.inventory_ledger_mut().apply(&slot_event(
            identity(
                0,
                Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
            ),
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
                    // The named window-less response container corrects the
                    // same canonical player cell the named Slot event
                    // addressed.
                    StackResponseContainer {
                        container: named_no_window,
                        slots: Arc::from([zero_count_correction(3)]),
                    },
                    // An unknown container name resolves onto no retained
                    // cell: skipped whole and counted, never a mutation.
                    // Responses decode unknown names with no window id, so
                    // this is the wire-reachable unrouted shape.
                    StackResponseContainer {
                        container: ContainerIdentity {
                            window_id: None,
                            slot_type: Some(211),
                            dynamic_id: None,
                        },
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
        "the named response cleared the same canonical cell"
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
