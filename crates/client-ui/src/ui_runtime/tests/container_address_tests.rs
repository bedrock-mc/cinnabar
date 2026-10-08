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

fn server_ledger(player_runtime: &mut player_state::PlayerState, runtime: &mut UiRuntime) {
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
}

fn decode_inventory_event(bytes: Vec<u8>) -> InventoryEvent {
    let mut packets = decode_batch(bytes.into(), &BedrockSession { shield_item_id: 0 }).unwrap();
    match into_world_event(packets.pop().unwrap(), 0).unwrap() {
        Some(WorldEvent::Inventory(event)) => event,
        other => panic!("expected inventory event, got {other:?}"),
    }
}

fn default_descriptor_full_inventory_fixture() -> Vec<u8> {
    // One batch packet containing InventoryContent(window 0), thirty-six
    // empty item descriptors, the zero/default full-container descriptor,
    // and an empty storage item descriptor.
    let mut packet = vec![0xb1, 0x48, 0, 36];
    packet.resize(packet.len() + 36 * 8, 0);
    packet.extend_from_slice(&[0, 0]);
    packet.resize(packet.len() + 8, 0);
    let mut batch = vec![0xfe, 0xae, 0x02];
    assert_eq!(packet.len(), 302);
    batch.extend(packet);
    batch
}

fn default_descriptor_present_content_fixture() -> Vec<u8> {
    let mut bytes = include_bytes!("../../../../protocol/fixtures/inventory_content.bin").to_vec();
    assert_eq!(bytes.len(), 58);
    assert_eq!(&bytes[44..50], &[12, 1, 7, 0, 0, 0]);
    bytes[1] = 0x34;
    bytes.splice(44..50, [0, 0]);
    bytes
}

fn default_descriptor_slot_fixture() -> Vec<u8> {
    let mut bytes = include_bytes!("../../../../protocol/fixtures/inventory_slot.bin").to_vec();
    assert_eq!(&bytes[6..9], &[1, 29, 0]);
    bytes[7] = 0;
    bytes
}

#[test]
fn default_descriptor_packets_establish_selected_stack_authority_in_the_ledger() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    server_ledger(&mut player_runtime, &mut runtime);

    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            1,
            1,
            decode_inventory_event(default_descriptor_full_inventory_fixture()),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player_runtime);
    assert_eq!(
        player_runtime.selected_stack_snapshot().unwrap().state,
        crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Empty,
        "the complete empty inventory establishes selected slot 0 as known empty"
    );

    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            1,
            2,
            decode_inventory_event(default_descriptor_present_content_fixture()),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player_runtime);
    assert!(matches!(
        player_runtime.selected_stack_snapshot().unwrap().state,
        crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Present(_)
    ));

    player_runtime.inventory.set_local_selected_slot(4);
    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            1,
            3,
            decode_inventory_event(default_descriptor_slot_fixture()),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player_runtime);
    let selected = player_runtime.selected_stack_snapshot().unwrap();
    assert_eq!(selected.slot, 4);
    assert!(matches!(
        selected.state,
        crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Present(_)
    ));
}

#[test]
fn cursor_slot_type_events_only_reach_the_cursor_cell() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    let mut slots = vec![NetworkItemStack::empty(); 36];
    slots[0] = stack(11);
    slots[8] = stack(19);
    runtime
        .enqueue_inventory_event(&mut player_runtime, 1, 1, content(identity(0, None), slots))
        .unwrap();
    // Cursor Slot and Content events ride the UI window naming the cursor container.
    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            1,
            2,
            slot_event(identity(124, Some(59)), 0, stack(777)),
        )
        .unwrap();
    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            1,
            3,
            content(identity(124, Some(59)), vec![stack(888)]),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player_runtime);

    // The cursor events belong to the cursor cell only.
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .cursor_stack()
            .map(|stack| stack.network_id),
        Some(888)
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(0)
            .map(|stack| stack.network_id),
        Some(11)
    );
}

/// A server's arbitrary container name on the player window still fills player inventory cells.
#[test]
fn foreign_container_name_on_the_player_window_fills_player_inventory_cells() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut player_runtime, &mut runtime);
    let anvil_material = Some(1);
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&content(
            identity(0, anvil_material),
            vec![NetworkItemStack::empty(); 36],
        ));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(0, anvil_material), 0, stack(20_329)));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(0)
            .map(|stack| stack.network_id),
        Some(20_329)
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        0
    );
}

#[test]
fn offhand_container_events_never_pollute_player_inventory_cells() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut player_runtime, &mut runtime);
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(0, None), 20, stack(20)));

    // Offhand traffic rides the offhand window and never reaches a player cell.
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&content(identity(119, Some(34)), vec![stack(34)]));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(119, Some(34)), 0, stack(340)));

    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(0)
            .map(|stack| stack.network_id),
        None
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(20)
            .map(|stack| stack.network_id),
        Some(20)
    );

    // The session continues: ordinary player-inventory traffic still lands.
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(0, None), 0, stack(5)));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(0)
            .map(|stack| stack.network_id),
        Some(5)
    );
}

#[test]
fn unknown_container_identities_skip_without_mutating_cells_or_ending_the_session() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut player_runtime, &mut runtime);

    // A well-formed Slot event naming an unroutable container is odd data:
    // skipped whole, never written into any player-inventory cell, and
    // counted as typed leniency.
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(5, Some(211)), 3, stack(999)));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(-777, None), 4, stack(998)));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        2,
        "both unrouted identities were counted"
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(3)
            .map(|stack| stack.network_id),
        None
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(4)
            .map(|stack| stack.network_id),
        None
    );

    // The session continues and later well-formed traffic still applies.
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(0, None), 3, stack(3)));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        2,
        "routed traffic never inflates the skip counter"
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(3)
            .map(|stack| stack.network_id),
        Some(3)
    );
}

#[test]
fn combined_player_name_on_a_foreign_window_is_skipped_by_ledger_and_hud() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let foreign = identity(
        6,
        Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
    );
    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut player_runtime, &mut runtime);
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(0, None), 3, stack(3)));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(foreign, 3, stack(63)));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(3)
            .map(|stack| stack.network_id),
        Some(3),
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        1
    );

    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
            1,
            1,
            slot_event(identity(0, None), 3, stack(3)),
        )
        .unwrap();
    runtime
        .enqueue_inventory_event(&mut player_runtime, 1, 2, content(foreign, vec![stack(60)]))
        .unwrap();
    runtime.drain_pending_inventory(&mut player_runtime);
    assert_eq!(
        runtime
            .gameplay_hud()
            .diagnostics()
            .unknown_container_events,
        1
    );
}

/// The pinned gophertunnel `InventorySlot` fixture (`tools/fixturegen`)
/// writes exactly this shape: legacy window id 0 carrying a full container
/// name whose byte is `InventoryContainer` (29). The canonical projection
/// must route such events to player cells, never counting them as unknown.
#[test]
fn fixture_named_inventory_slot_updates_land_in_player_cells() {
    let mut player_runtime = player_state::PlayerState::new(1);

    const INVENTORY_CONTAINER_NAME: u8 = 29;

    // Ledger admission.
    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut player_runtime, &mut runtime);
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(
            identity(0, Some(INVENTORY_CONTAINER_NAME)),
            4,
            stack(29),
        ));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(4)
            .map(|stack| stack.network_id),
        Some(29),
        "the fixture-shaped Slot event lands in canonical player cell 4"
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        0,
        "routed fixture traffic never counts as leniency"
    );

    // HUD ingestion through the production queue.
    let mut hud = UiRuntime::new(1);
    hud.enqueue_inventory_event(
        &mut player_runtime,
        1,
        1,
        slot_event(identity(0, Some(INVENTORY_CONTAINER_NAME)), 4, stack(29)),
    )
    .unwrap();
    hud.drain_pending_inventory(&mut player_runtime);
    assert_eq!(hud.gameplay_hud().diagnostics().unknown_container_events, 0);

    // A named full-content rewrite rides the same alias.
    let slots: Vec<NetworkItemStack> = (1..=36).map(stack).collect();
    hud.enqueue_inventory_event(
        &mut player_runtime,
        1,
        2,
        content(identity(0, Some(INVENTORY_CONTAINER_NAME)), slots),
    )
    .unwrap();
    hud.drain_pending_inventory(&mut player_runtime);
    assert_eq!(hud.gameplay_hud().diagnostics().unknown_container_events, 0);

    // Generic-storage-named traffic on a non-player window belongs to no
    // player cell; with no storage window open it is counted leniency.
    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut player_runtime, &mut runtime);
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(0, None), 4, stack(4)));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(
            identity(5, Some(protocol::CONTAINER_NAME_LEVEL_ENTITY)),
            4,
            stack(777),
        ));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(
            identity(6, Some(protocol::CONTAINER_NAME_LEVEL_ENTITY)),
            4,
            stack(778),
        ));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(4)
            .map(|stack| stack.network_id),
        Some(4),
        "storage-named events never reach player cells"
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        2,
        "both storage events with no matching open window were counted"
    );

    // With a matching open window the same surface lands in storage, while a
    // wrong-window storage identity stays the prior silent targeted drop.
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
            container: ContainerIdentity::window(4),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            position: [0, 0, 0],
            runtime_entity_id: 1,
        }));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&content(
            ContainerIdentity {
                window_id: Some(4),
                slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
                dynamic_id: Some(9),
            },
            vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT],
        ));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(
            identity(4, Some(protocol::CONTAINER_NAME_INVENTORY)),
            3,
            stack(33),
        ));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .storage_stack(3)
            .map(|stack| stack.network_id),
        None,
        "the player-inventory alias never reaches an open storage window"
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        3,
        "the off-window alias resolved onto no retained cell and was counted"
    );
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(
            identity(9, Some(protocol::CONTAINER_NAME_LEVEL_ENTITY)),
            3,
            stack(99),
        ));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .storage_stack(3)
            .map(|stack| stack.network_id),
        None,
        "a wrong-window storage identity is dropped without mutation"
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
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
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    server_ledger(&mut player_runtime, &mut runtime);
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
            container: ContainerIdentity::window(4),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            position: [0, 0, 0],
            runtime_entity_id: 1,
        }));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&content(
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
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(4, None), 5, stack(55)));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .storage_stack(5)
            .map(|stack| stack.network_id),
        Some(55),
        "bare-window updates reach the open generic-storage window like before"
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        0
    );

    // A bare window id that matches no open window stays unrouted leniency.
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(9, None), 5, stack(99)));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .storage_stack(5)
            .map(|stack| stack.network_id),
        Some(55)
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        1
    );

    // Without any open window the same bare shape cannot land anywhere and
    // is counted; the session keeps accepting routed traffic.
    let mut closed = UiRuntime::new(1);
    server_ledger(&mut player_runtime, &mut closed);
    closed
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(4, None), 5, stack(55)));
    assert_eq!(
        closed
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        1
    );
    closed
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(0, None), 5, stack(5)));
    assert_eq!(
        closed
            .inventory_ledger(&player_runtime)
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
    let mut player_runtime = player_state::PlayerState::new(1);

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
    server_ledger(&mut player_runtime, &mut runtime);
    for (slot, network_id) in [(3, 33), (4, 44), (5, 55)] {
        let mut server_stack = stack(network_id);
        server_stack.stack_network_id = network_id;
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .apply(&slot_event(
                identity(
                    0,
                    Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                ),
                slot,
                server_stack,
            ));
    }
    assert!(
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .request_personal_open(42)
    );
    assert!(
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .mark_transport_enqueued(0)
    );
    // One in-flight gesture so an accepted response can reconcile at all.
    let request = runtime
        .inventory_ledger_mut(&mut player_runtime)
        .begin_click(3)
        .unwrap();
    runtime
        .inventory_ledger_mut(&mut player_runtime)
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
            .inventory_ledger(&player_runtime)
            .displayed_stack(3)
            .map(|stack| stack.network_id),
        None,
        "the named response cleared the same canonical cell"
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(4)
            .map(|stack| stack.network_id),
        Some(44),
        "the unrouted correction mutated nothing"
    );
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .skipped_unknown_containers(),
        1,
        "exactly the unrouted correction was counted"
    );

    // The session continues: later well-formed traffic still applies.
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&slot_event(identity(0, None), 4, stack(404)));
    assert_eq!(
        runtime
            .inventory_ledger(&player_runtime)
            .displayed_stack(4)
            .map(|stack| stack.network_id),
        Some(404)
    );
}
