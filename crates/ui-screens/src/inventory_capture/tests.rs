use super::*;

/// Supplies one known player stack; every other slot remains unknown.
fn ledger() -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&protocol::InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    ledger.apply(&protocol::InventoryEvent::Content(
        protocol::InventoryContentEvent {
            container: protocol::ContainerIdentity::window(0),
            slots: Arc::from([NetworkItemStack {
                network_id: 1,
                count: 1,
                stack_network_id: 9,
                ..NetworkItemStack::empty()
            }]),
            storage_item: NetworkItemStack::empty(),
        },
    ));
    ledger
}

#[test]
fn closed_inventory_keeps_slot_art_without_resolving_recipes_or_hover_names() {
    let ledger = ledger();
    let icon = IconRef {
        color_mask: None,
        page: 0,
        uv: [0, 0, 16, 16],
        glint: true,
    };
    let contents = capture_contents(
        &ledger,
        false,
        || panic!("closed inventory must not match crafting"),
        |stack, slot| {
            assert_eq!(stack.stack_network_id, 9);
            assert_eq!(slot, Some(0));
            Some(icon)
        },
        |_| panic!("closed inventory must not resolve hover names"),
    );
    assert_eq!(contents.inventory_icons.0[0], Some(icon));
    assert!(contents.inventory_icons.0[1..].iter().all(Option::is_none));
    assert!(contents.storage_icons.0.iter().all(Option::is_none));
    assert!(contents.crafting.icons.iter().all(Option::is_none));
    assert!(contents.crafting.output.is_none());
    assert!(contents.item_names.is_empty());
}

#[test]
fn decorations_preserve_authoritative_durability_corrections() {
    let mut ledger = ledger();
    let request = ledger.begin_mining_request(0, 7).unwrap();
    ledger.apply(&protocol::InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                status: protocol::StackResponseStatus::Accepted,
                request_id: request,
                containers: Arc::from([protocol::StackResponseContainer {
                    container: protocol::ContainerIdentity {
                        window_id: None,
                        slot_type: Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                        dynamic_id: None,
                    },
                    slots: Arc::from([protocol::StackResponseSlot {
                        slot: 0,
                        hotbar_slot: 0,
                        count: 1,
                        item_stack_id: 9,
                        custom_name: Arc::from(""),
                        filtered_custom_name: Arc::from(""),
                        durability_correction: 7,
                    }]),
                }]),
            }]),
        },
    ));
    let (durability, icons) = capture_decorations(
        &ledger,
        |stack, correction| {
            assert_eq!(stack.stack_network_id, 9);
            assert_eq!(correction, Some(7));
            Some(0.75)
        },
        |_| panic!("unknown UI slots must not resolve artwork"),
    );
    assert_eq!(durability.player[0], Some(0.75));
    assert!(durability.player[1..].iter().all(Option::is_none));
    assert!(durability.storage.iter().all(Option::is_none));
    assert!(durability.ui.iter().all(Option::is_none));
    assert!(icons.iter().all(Option::is_none));
    assert_eq!(ledger.predicted_slot_damage(0), Some(7));
    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, 9);
}
