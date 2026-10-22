//! Rejected and unanswered requests must never leave later gestures refused.

use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, CONTAINER_NAME_CURSOR, ContainerIdentity,
    ContainerOpenEvent, InventoryContentEvent, InventoryEvent, ItemRegistryEntry,
    ItemRegistryEvent, ItemRegistryVersion, ItemStackResponseEvent, NetworkItemStack,
    StackResponse, StackResponseContainer, StackResponseSlot, StackResponseStatus,
};

use super::*;

const SWORD: i32 = 318;
const COBBLE: i32 = 8;
const ANVIL_WINDOW: i32 = 3;

fn registry() -> ItemRegistryEvent {
    let entry = |network_id, identifier: &str, max| ItemRegistryEntry {
        identifier: Arc::from(identifier),
        network_id,
        component_based: false,
        version: ItemRegistryVersion::None,
        components: None,
        component_digest: [0; 32],
        negotiated_max_stack_size: Some(max),
        canonical_empty_component_data: true,
        item_tags: Arc::from([]),
    };
    ItemRegistryEvent {
        entries: vec![
            entry(SWORD, "minecraft:diamond_sword", 1),
            entry(COBBLE, "minecraft:cobblestone", 64),
        ]
        .into(),
    }
}

fn stack(network_id: i32, stack_network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        stack_network_id,
        count,
        ..NetworkItemStack::default()
    }
}

/// A creative player holding two swords, with the anvil screen open.
fn anvil_ledger() -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&registry());
    ledger.set_permissions(InventoryPermissions {
        creative: true,
        instabuild: Some(true),
    });
    ledger.apply(&InventoryEvent::Creative(protocol::CreativeContentEvent {
        groups: Arc::from([]),
        items: Arc::from([protocol::CreativeItem {
            creative_network_id: 44,
            stack: stack(COBBLE, -1, 1),
            group: 0,
        }]),
        skipped: 0,
    }));
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = stack(SWORD, 19, 1);
    slots[1] = stack(SWORD, 18, 1);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
        slots: Arc::from(vec![NetworkItemStack::default(); protocol::UI_SLOT_COUNT]),
        storage_item: NetworkItemStack::default(),
    }));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(ANVIL_WINDOW),
        window_type: protocol::WINDOW_TYPE_ANVIL,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger
}

fn row(name: u8, slot: u8, count: u8, id: i32) -> StackResponseContainer {
    StackResponseContainer {
        container: ContainerIdentity {
            window_id: None,
            slot_type: Some(name),
            dynamic_id: None,
        },
        slots: Arc::from([StackResponseSlot {
            slot,
            hotbar_slot: slot,
            count,
            item_stack_id: id,
            custom_name: Arc::from(""),
            filtered_custom_name: Arc::from(""),
            durability_correction: 0,
        }]),
    }
}

fn respond(
    ledger: &mut PlayerInventoryLedger,
    request_id: i32,
    status: StackResponseStatus,
    containers: Vec<StackResponseContainer>,
) {
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status,
            request_id,
            containers: Arc::from(containers),
        }]),
    }));
}

fn send_all(ledger: &mut PlayerInventoryLedger, now: u64) {
    while matches!(ledger.pending_batch(), Ok(Some(_))) {
        assert!(ledger.mark_transport_enqueued(now));
    }
}

/// Moves both swords into the anvil inputs through accepted requests.
fn fill_inputs(ledger: &mut PlayerInventoryLedger) {
    let target = protocol::ui_slot_container_name(ANVIL_TARGET_SLOT).unwrap();
    let material = protocol::ui_slot_container_name(ANVIL_MATERIAL_SLOT).unwrap();
    for (slot, input, name, id) in [
        (1, ANVIL_TARGET_SLOT, target, 18),
        (0, ANVIL_MATERIAL_SLOT, material, 19),
    ] {
        let take = ledger.begin_click(slot).unwrap();
        send_all(ledger, 0);
        respond(
            ledger,
            take,
            StackResponseStatus::Accepted,
            vec![
                row(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, slot, 0, -1),
                row(CONTAINER_NAME_CURSOR, 0, 1, id),
            ],
        );
        let place = ledger
            .begin_target_gesture(InventoryTarget::Craft(input), CellGesture::Click)
            .unwrap();
        send_all(ledger, 0);
        respond(
            ledger,
            place,
            StackResponseStatus::Accepted,
            vec![
                row(CONTAINER_NAME_CURSOR, 0, 0, -1),
                row(name, input, 1, id),
            ],
        );
    }
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
            .map(|held| held.stack_network_id),
        Some(18)
    );
    assert!(!ledger.resync_required());
}

/// Takes the predicted anvil result into the cursor.
fn take_result(ledger: &mut PlayerInventoryLedger) -> i32 {
    ledger
        .begin_screen_output(&ScreenCraft::Anvil {
            rename: None,
            multi_recipe_id: 4766,
            output: Some(stack(SWORD, 0, 1)),
            material_used: 1,
        })
        .unwrap()
}

/// The inventory is back where the rejected requests started and accepts new gestures.
fn assert_recovered(ledger: &mut PlayerInventoryLedger) {
    assert!(ledger.cursor_stack().is_none());
    assert!(ledger.displayed_stack(5).is_none());
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
            .map(|held| held.stack_network_id),
        Some(18)
    );
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(ANVIL_MATERIAL_SLOT))
            .map(|held| held.stack_network_id),
        Some(19)
    );
    assert!(!ledger.resync_required());
    ledger
        .begin_target_gesture(
            InventoryTarget::Craft(ANVIL_TARGET_SLOT),
            CellGesture::Click,
        )
        .expect("the restored input can be picked up again");
}

#[test]
fn a_rejected_anvil_take_and_its_dependent_place_restore_the_inputs() {
    let mut ledger = anvil_ledger();
    fill_inputs(&mut ledger);
    let take = take_result(&mut ledger);
    send_all(&mut ledger, 10);
    let place = ledger.begin_click(5).unwrap();
    send_all(&mut ledger, 20);
    respond(&mut ledger, take, StackResponseStatus::Rejected, vec![]);
    respond(&mut ledger, place, StackResponseStatus::Rejected, vec![]);
    assert_recovered(&mut ledger);
}

#[test]
fn a_rejected_anvil_take_discards_its_unsent_dependent_place() {
    let mut ledger = anvil_ledger();
    fill_inputs(&mut ledger);
    let take = take_result(&mut ledger);
    send_all(&mut ledger, 10);
    ledger.begin_click(5).unwrap();
    respond(&mut ledger, take, StackResponseStatus::Rejected, vec![]);
    assert_eq!(ledger.pending_request_count(), 0);
    assert_recovered(&mut ledger);
}

#[test]
fn creative_takes_work_after_a_rejected_anvil_take_and_close() {
    let mut ledger = anvil_ledger();
    fill_inputs(&mut ledger);
    let take = take_result(&mut ledger);
    send_all(&mut ledger, 10);
    respond(&mut ledger, take, StackResponseStatus::Rejected, vec![]);
    ledger.request_storage_close();
    send_all(&mut ledger, 20);
    ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
        container: ContainerIdentity::window(ANVIL_WINDOW),
        window_type: protocol::WINDOW_TYPE_ANVIL,
        server_initiated: false,
    }));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(30));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger
        .begin_creative_take(44, CreativeDestination::Cursor, None)
        .expect("creative picks stay available");
}

/// Vanilla keeps an unanswered prediction without refusing later gestures; Cinnabar refuses
/// them only until the recovery window passes.
#[test]
fn an_unanswered_anvil_take_stops_refusing_gestures_after_the_recovery_window() {
    let mut ledger = anvil_ledger();
    fill_inputs(&mut ledger);
    let take = take_result(&mut ledger);
    send_all(&mut ledger, 10);
    let timed_out = 10 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
    ledger.poll_timeout(timed_out);
    assert_eq!(
        ledger.begin_creative_take(44, CreativeDestination::Player(8), None),
        Err(InventoryGestureError::ResyncRequired)
    );
    assert!(!ledger.timeout_work_due(timed_out + 1));
    let mut saturated = ledger.clone();
    saturated.poll_timeout(u64::MAX);
    saturated.poll_timeout(u64::MAX);
    assert!(
        saturated.resync_required(),
        "the window starts at its first poll, even at the clock's limit"
    );
    let lifted = timed_out + INVENTORY_RECOVERY_WINDOW_MILLIS;
    assert!(ledger.timeout_work_due(lifted));
    ledger.poll_timeout(lifted);
    assert!(!ledger.resync_required());
    assert_eq!(
        ledger.cursor_stack().map(|held| held.network_id),
        Some(SWORD),
        "the unanswered prediction stays, as in vanilla"
    );
    let creative = ledger
        .begin_creative_take(44, CreativeDestination::Player(8), None)
        .expect("creative picks resume");
    send_all(&mut ledger, lifted);
    respond(&mut ledger, take, StackResponseStatus::Rejected, vec![]);
    respond(&mut ledger, creative, StackResponseStatus::Rejected, vec![]);
    assert_recovered(&mut ledger);
}

/// Deterministic xorshift so failures replay by seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % bound
    }
}

/// A creative player with mixed stacks and the personal inventory open.
fn personal_ledger() -> PlayerInventoryLedger {
    let mut ledger = anvil_ledger();
    ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
        container: ContainerIdentity::window(ANVIL_WINDOW),
        window_type: protocol::WINDOW_TYPE_ANVIL,
        server_initiated: true,
    }));
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = stack(COBBLE, 10, 32);
    slots[1] = stack(COBBLE, 11, 5);
    slots[2] = stack(SWORD, 12, 1);
    slots[9] = stack(COBBLE, 13, 64);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    }));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    assert!(!ledger.resync_required());
    ledger
}

/// Every presented cell a random gesture can reach.
fn presented(ledger: &PlayerInventoryLedger) -> Vec<Option<NetworkItemStack>> {
    (0..PLAYER_INVENTORY_SLOT_COUNT as u8)
        .map(|slot| ledger.displayed_stack(slot).cloned())
        .chain(std::iter::once(ledger.cursor_stack().cloned()))
        .chain((28..32).map(|slot| ledger.target_stack(InventoryTarget::Craft(slot)).cloned()))
        .collect()
}

/// Issues one random gesture; refusals are part of the exercise.
fn random_gesture(ledger: &mut PlayerInventoryLedger, rng: &mut Rng) {
    let slot = [0, 1, 2, 3, 9, 10][rng.next(6) as usize];
    let target = if rng.next(4) == 0 {
        InventoryTarget::Craft(28 + rng.next(4) as u8)
    } else {
        InventoryTarget::Player(slot)
    };
    let amount = 1 + rng.next(16) as u16;
    let _ = match rng.next(11) {
        0 | 1 => ledger.begin_target_gesture(target, CellGesture::Click),
        2 => ledger.begin_target_gesture(target, CellGesture::TakeCount(amount)),
        3 => ledger.begin_target_gesture(target, CellGesture::PlaceCount(amount)),
        4 => ledger.begin_quick_move(target),
        5 => ledger.begin_hotbar_swap(target, rng.next(9) as u8),
        6 => ledger.begin_drop(DropSource::Target(target), Some(amount)),
        7 => ledger.begin_drop(DropSource::Cursor, None),
        8 => ledger.begin_destroy_cursor(None),
        9 => ledger.begin_creative_take(
            44,
            if rng.next(2) == 0 {
                CreativeDestination::Cursor
            } else {
                CreativeDestination::Player(slot)
            },
            None,
        ),
        _ => ledger.begin_gather(),
    };
}

/// Rejects every request in server order, interleaved with new gestures.
#[test]
fn rejecting_every_request_restores_the_start_and_never_gates_gestures() {
    for seed in 1..400_u64 {
        let mut ledger = personal_ledger();
        let start = presented(&ledger);
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        for _ in 0..16 {
            match rng.next(3) {
                0 => random_gesture(&mut ledger, &mut rng),
                1 => {
                    while matches!(ledger.pending_batch(), Ok(Some(_))) {
                        assert!(ledger.mark_transport_enqueued(10));
                    }
                }
                _ => {
                    let oldest = ledger
                        .gestures()
                        .find(|request| request.state == InventoryPendingState::AwaitingResponse)
                        .map(|request| request.request_id);
                    if let Some(id) = oldest {
                        respond(&mut ledger, id, StackResponseStatus::Rejected, vec![]);
                    }
                }
            }
        }
        while matches!(ledger.pending_batch(), Ok(Some(_))) {
            assert!(ledger.mark_transport_enqueued(10));
        }
        while let Some(id) = ledger.pending_request_id() {
            respond(&mut ledger, id, StackResponseStatus::Rejected, vec![]);
        }
        assert_eq!(presented(&ledger), start, "seed {seed}");
        assert!(!ledger.resync_required(), "seed {seed}");
        ledger
            .begin_creative_take(44, CreativeDestination::Cursor, None)
            .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
    }
}
