use inventory::PlayerInventoryLedger;
use inventory::inventory_ledger::{PLAYER_INVENTORY_SLOT_COUNT, SMALL_STORAGE_SLOT_COUNT};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, InventorySlotEvent, NetworkItemStack, SlotIdentity,
};

/// Builds a plain original stack with a positive server identity.
fn item(id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        count,
        stack_network_id: id,
        ..NetworkItemStack::empty()
    }
}

/// Makes one content event with no storage-item payload.
fn content(container: ContainerIdentity, slots: Vec<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    })
}

/// Addresses one wire slot without a storage-item payload.
fn update(container: ContainerIdentity, slot: u16, stack: NetworkItemStack) -> InventorySlotEvent {
    InventorySlotEvent {
        identity: SlotIdentity { container, slot },
        stack,
        storage_item: None,
    }
}

/// Creates an empty, fully observed player inventory with the selected authority.
fn ready(authority: InventoryAuthority) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(authority));
    ledger.apply(&content(
        ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT],
    ));
    ledger
}

/// Names a container on an ordinary window, retaining an arbitrary dynamic id.
fn named(window: i32, name: u8, dynamic: Option<u32>) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(window),
        slot_type: Some(name),
        dynamic_id: dynamic,
    }
}

/// Opens an ordinary chest using its fixed, non-dynamic native container key.
fn fixed_chest() -> PlayerInventoryLedger {
    let mut l = ready(InventoryAuthority::Server);
    l.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: 0,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let mut slots = vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT];
    slots[2] = item(80, 3);
    l.apply(&content(
        named(7, protocol::CONTAINER_NAME_LEVEL_ENTITY, None),
        slots,
    ));
    l
}

/// Registry-window Content resolves the chest's registered key without legacy-id equality.
#[test]
fn registry_content_reaches_ordinary_chest() {
    let mut l = fixed_chest();
    let mut slots = vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT];
    slots[2] = item(200, 9);
    l.apply(&content(
        named(125, protocol::CONTAINER_NAME_LEVEL_ENTITY, None),
        slots,
    ));
    assert_eq!(l.storage_stack(2), Some(&item(200, 9)));
}

/// Registry-window Slot resolves the chest's registered key without legacy-id equality.
#[test]
fn registry_slot_reaches_ordinary_chest() {
    let mut l = fixed_chest();
    l.apply(&InventoryEvent::Slot(update(
        named(125, protocol::CONTAINER_NAME_LEVEL_ENTITY, None),
        2,
        item(200, 9),
    )));
    assert_eq!(l.storage_stack(2), Some(&item(200, 9)));
}

/// The registered main-inventory model maps its relative slot two to player slot eleven.
#[test]
fn registry_inventory_name_uses_relative_player_offset() {
    let mut l = fixed_chest();
    l.apply(&InventoryEvent::Slot(update(
        named(125, protocol::CONTAINER_NAME_INVENTORY, None),
        2,
        item(200, 9),
    )));
    assert_eq!(l.displayed_stack(11), Some(&item(200, 9)));
    assert!(l.displayed_stack(2).is_none());
}

/// A present dynamic id creates a different key from the registered fixed cursor.
#[test]
fn registry_fixed_cursor_dynamic_id_is_not_ignored() {
    let mut l = fixed_chest();
    l.apply(&InventoryEvent::Slot(update(
        named(125, protocol::CONTAINER_NAME_CURSOR, Some(9)),
        0,
        item(200, 9),
    )));
    assert!(l.cursor_stack().is_none());
}

/// UI-window Slot uses raw slot zero despite a conflicting packet name.
#[test]
fn ui_slot_zero_is_cursor_despite_offhand_name() {
    let mut l = ready(InventoryAuthority::Server);
    l.apply(&InventoryEvent::Slot(update(
        named(124, protocol::CONTAINER_NAME_OFFHAND, None),
        0,
        item(200, 9),
    )));
    assert_eq!(l.cursor_stack(), Some(&item(200, 9)));
    assert!(
        l.target_stack(inventory::inventory_ledger::InventoryTarget::Offhand)
            .is_none()
    );
}

/// UI Content needs all 54 slots even if its name claims a one-cell surface.
#[test]
fn ui_short_cursor_content_is_not_authority() {
    let mut l = ready(InventoryAuthority::Server);
    l.apply(&content(
        named(124, protocol::CONTAINER_NAME_CURSOR, None),
        vec![item(200, 9)],
    ));
    assert!(l.cursor_stack().is_none());
}

/// UI Content ignores names and writes its cursor and crafting cells.
#[test]
fn ui_full_content_ignores_armor_name() {
    let mut l = ready(InventoryAuthority::Server);
    let mut slots = vec![NetworkItemStack::empty(); protocol::UI_SLOT_COUNT];
    slots[0] = item(200, 9);
    slots[28] = item(201, 4);
    l.apply(&content(
        named(124, protocol::CONTAINER_NAME_ARMOR, None),
        slots,
    ));
    assert_eq!(l.cursor_stack(), Some(&item(200, 9)));
    assert_eq!(
        l.target_stack(inventory::inventory_ledger::InventoryTarget::Craft(28)),
        Some(&item(201, 4))
    );
    assert!(
        l.target_stack(inventory::inventory_ledger::InventoryTarget::Armor(0))
            .is_none()
    );
}

/// A Slot after a one-cell player Content cannot regrow the retired tail.
#[test]
fn player_slot_respects_short_content_extent() {
    let mut l = ready(InventoryAuthority::Server);
    l.apply(&content(
        ContainerIdentity::window(0),
        vec![NetworkItemStack::empty()],
    ));
    l.apply(&InventoryEvent::Slot(update(
        ContainerIdentity::window(0),
        35,
        item(200, 9),
    )));
    assert!(l.displayed_stack(35).is_none());
}

/// Standard registry cursor and real UI packets are independent positive controls.
#[test]
fn registry_and_ui_positive_controls() {
    let mut l = fixed_chest();
    l.apply(&InventoryEvent::Slot(update(
        named(125, protocol::CONTAINER_NAME_CURSOR, None),
        0,
        item(200, 9),
    )));
    assert_eq!(l.cursor_stack(), Some(&item(200, 9)));
    let mut l = ready(InventoryAuthority::Server);
    l.apply(&InventoryEvent::Slot(update(
        ContainerIdentity::window(124),
        28,
        item(201, 4),
    )));
    assert_eq!(
        l.target_stack(inventory::inventory_ledger::InventoryTarget::Craft(28)),
        Some(&item(201, 4))
    );
}

/// UI result previews retain the raw slot used by existing result-click consumers.
#[test]
fn ui_output_slot_retains_the_server_preview() {
    let mut l = ready(InventoryAuthority::Server);
    l.apply(&InventoryEvent::Slot(update(
        named(124, protocol::CONTAINER_NAME_CREATED_OUTPUT, None),
        50,
        item(200, 9),
    )));
    assert_eq!(l.created_output_stack(), Some(&item(200, 9)));
}

#[test]
fn registry_named_furnace_routes_content_and_slot() {
    let mut observations = Vec::new();
    for full in [false, true] {
        let mut l = ready(InventoryAuthority::Server);
        l.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(7),
            window_type: 2,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
        let name = protocol_ids::ContainerName::FurnaceIngredient.wire_value();
        let id = named(125, name, None);
        if full {
            l.apply(&content(id, vec![item(200, 9)]));
        } else {
            l.apply(&InventoryEvent::Slot(update(id, 0, item(200, 9))));
        }
        observations.push(l.storage_stack(0).cloned());
        eprintln!("FURNACE full={full} stack={:?}", l.storage_stack(0));
    }
    assert_eq!(observations, vec![Some(item(200, 9)); 2]);
}

/// Fixed player windows cannot be redirected to a bundle by the optional packet name.
#[test]
fn fixed_player_dynamic_name_content_and_slot() {
    let mut observations = Vec::new();
    for full in [false, true] {
        let mut l = ready(InventoryAuthority::Server);
        let id = named(0, protocol::CONTAINER_NAME_DYNAMIC, Some(9));
        if full {
            l.apply(&content(id, vec![item(200, 9)]));
        } else {
            l.apply(&InventoryEvent::Slot(update(id, 0, item(200, 9))));
        }
        observations.push(l.displayed_stack(0).cloned());
        eprintln!(
            "FIXED_DYNAMIC full={full} player={:?} bundle={:?}",
            l.displayed_stack(0),
            l.bundle_contents(9)
        );
    }
    assert_eq!(observations, vec![Some(item(200, 9)); 2]);
}

/// A dynamic container cannot appear without an existing entry or valid storage-item registration.
#[test]
fn unregistered_bundle_content_does_not_create_authority() {
    let mut l = ready(InventoryAuthority::Server);
    l.apply(&content(
        named(125, protocol::CONTAINER_NAME_DYNAMIC, Some(901)),
        vec![item(200, 9)],
    ));
    assert!(l.bundle_contents(901).is_none());
}

/// A normal packet must be verified against the current extent before publishing any leg.
#[test]
fn short_extent_normal_packet_is_whole_invalid() {
    let mut l = ready(InventoryAuthority::Server);
    l.apply(&content(
        ContainerIdentity::window(0),
        vec![NetworkItemStack::empty()],
    ));
    let changes = [0, 35]
        .into_iter()
        .map(|slot| {
            let old = NetworkItemStack::empty();
            let new = item(200 + slot as i32, 9);
            protocol::NormalInventoryChange {
                source: protocol::NormalInventorySource::Container(0),
                slot,
                from: protocol::VerifiedNetworkItemStack::try_new(old.clone(), old.nbt_digest)
                    .unwrap(),
                to: protocol::VerifiedNetworkItemStack::try_new(new.clone(), new.nbt_digest)
                    .unwrap(),
            }
        })
        .collect();
    let packet = protocol::normal_inventory_transaction_packet(changes).unwrap();
    let protocol::WorldEvent::Inventory(event) =
        protocol::into_world_event(packet, 0).unwrap().unwrap()
    else {
        panic!()
    };
    l.apply(&event);
    assert!(
        l.displayed_stack(0).is_none(),
        "invalid second leg must prevent the valid first leg"
    );
}

/// Offhand Slot packets have no fixed-window branch and cannot bypass the open manager id.
#[test]
fn offhand_slot_without_matching_manager_is_ignored() {
    let mut l = fixed_chest();
    l.apply(&InventoryEvent::Slot(update(
        ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
        0,
        item(200, 9),
    )));
    assert!(
        l.target_stack(inventory::inventory_ledger::InventoryTarget::Offhand)
            .is_none()
    );
}

/// A complete UI receipt publishes both its cursor and server result preview.
#[test]
fn ui_content_retains_the_server_preview() {
    let mut ledger = ready(InventoryAuthority::Server);
    let mut slots = vec![NetworkItemStack::empty(); protocol::UI_SLOT_COUNT];
    slots[0] = item(200, 9);
    slots[usize::from(protocol::CREATED_OUTPUT_SLOT)] = item(201, 1);
    ledger.apply(&content(
        ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
        slots,
    ));
    assert_eq!(ledger.cursor_stack(), Some(&item(200, 9)));
    assert_eq!(ledger.created_output_stack(), Some(&item(201, 1)));
}

/// A complete registered storage model releases the same recovery scope as ordinary Content.
#[test]
fn registry_full_content_recovers_a_storage_snapshot() {
    let mut ledger = fixed_chest();
    ledger.begin_storage_click(2).unwrap();
    assert!(ledger.mark_transport_enqueued(0));
    ledger.poll_timeout(inventory::inventory_ledger::INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.resync_required());
    let mut slots = vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT];
    slots[2] = item(201, 4);
    ledger.apply(&content(
        named(
            protocol::DYNAMIC_STORAGE_WINDOW_ID,
            protocol::CONTAINER_NAME_LEVEL_ENTITY,
            None,
        ),
        slots,
    ));
    ledger.apply(&InventoryEvent::Slot(update(
        ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
        0,
        NetworkItemStack::empty(),
    )));
    assert!(!ledger.resync_required());
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(ledger.begin_storage_click(2).is_ok());
}

/// Personal screens register the fixed player models without an ordinary storage window.
#[test]
fn review_regression_personal_registry_models_do_not_require_storage() {
    let mut ledger = ready(InventoryAuthority::Server);
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    assert!(ledger.storage_generation().is_none());
    for (name, relative, combined) in [
        (protocol::CONTAINER_NAME_HOTBAR, 2, 2),
        (protocol::CONTAINER_NAME_INVENTORY, 2, 11),
        (
            protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            12,
            12,
        ),
    ] {
        ledger.apply(&InventoryEvent::Slot(update(
            named(125, name, None),
            relative,
            item(200, 3),
        )));
        assert_eq!(ledger.displayed_stack(combined), Some(&item(200, 3)));
    }
    ledger.apply(&content(
        named(125, protocol::CONTAINER_NAME_CURSOR, None),
        vec![item(201, 4)],
    ));
    assert_eq!(ledger.cursor_stack(), Some(&item(201, 4)));
}

/// Registry-only furnace models establish the same interactive request address as Content.
#[test]
fn registry_only_furnace_contents_support_ordinary_clicks() {
    use protocol_ids::ContainerName;
    for full in [false, true] {
        let mut ledger = ready(InventoryAuthority::Server);
        ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(7),
            window_type: protocol::WINDOW_TYPE_FURNACE,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
        for (name, held) in [
            (ContainerName::FurnaceIngredient, item(200, 9)),
            (ContainerName::FurnaceFuel, NetworkItemStack::empty()),
            (ContainerName::FurnaceResult, NetworkItemStack::empty()),
        ] {
            let container = named(protocol::DYNAMIC_STORAGE_WINDOW_ID, name.wire_value(), None);
            if full {
                ledger.apply(&content(container, vec![held]));
            } else {
                ledger.apply(&InventoryEvent::Slot(update(container, 0, held)));
            }
        }
        assert_eq!(ledger.storage_slot_count(), Some(3));
        ledger.begin_storage_click(0).unwrap();
        assert_eq!(ledger.cursor_stack().unwrap().count, 9);
        assert!(ledger.pending_batch().unwrap().is_some());
        assert_eq!(
            ledger.storage_identity(),
            Some(named(
                7,
                ContainerName::FurnaceIngredient.wire_value(),
                None
            ))
        );
    }
}
