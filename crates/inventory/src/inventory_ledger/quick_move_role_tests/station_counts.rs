use super::*;

/// Reads a named run from the same retained station layout used by packet routing.
fn segment(name: protocol_ids::ContainerName) -> protocol::WindowSegment {
    let Some(protocol::OpenCells::Named { segments, .. }) =
        protocol::WindowKind::Brewing.open_cells()
    else {
        panic!("brewing must retain named cells");
    };
    *segments
        .iter()
        .find(|segment| segment.name == name.wire_value())
        .unwrap()
}

/// Gives a station a counted source stack without replacing its registry or storage cells.
fn source_count(ledger: &mut PlayerInventoryLedger, count: u16) {
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[usize::from(SOURCE_SLOT)] = NetworkItemStack {
        count,
        ..ledger.displayed_stack(SOURCE_SLOT).unwrap().clone()
    };
    ledger.apply(&content(
        ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        slots,
    ));
    ledger.begin_click(SOURCE_SLOT).unwrap();
}

#[test]
fn brewing_bottle_cells_never_receive_a_count_above_one() {
    let bottles = segment(protocol_ids::ContainerName::BrewingStandResult);
    for slot in bottles.first..bottles.first + bottles.count {
        // This negative case tests the count bound, not bottle membership.
        let entry = registered("minecraft:glass_bottle");
        let mut ledger = station_ledger(&entry, protocol::WINDOW_TYPE_BREWING_STAND);
        source_count(&mut ledger, 2);
        let _ = ledger.begin_storage_place_count(slot, 2);
        let placed = ledger
            .target_stack(InventoryTarget::Storage(slot))
            .map_or(0, |stack| stack.count);
        assert!(placed <= 1);
        assert_eq!(
            ledger.cursor_stack().map_or(0, |stack| stack.count) + placed,
            2
        );
    }
}

#[test]
fn brewing_bottle_cells_accept_one_stock_potion() {
    let bottles = segment(protocol_ids::ContainerName::BrewingStandResult);
    let entry = registered("minecraft:potion");
    for slot in bottles.first..bottles.first + bottles.count {
        let mut ledger = station_ledger(&entry, protocol::WINDOW_TYPE_BREWING_STAND);
        source_count(&mut ledger, 1);
        ledger.begin_storage_place_count(slot, 1).unwrap();
        assert_eq!(
            ledger
                .target_stack(InventoryTarget::Storage(slot))
                .unwrap()
                .count,
            1
        );
        assert!(ledger.cursor_stack().is_none());
    }
}

#[test]
fn brewing_fuel_retains_ordinary_item_capacity() {
    let fuel = segment(protocol_ids::ContainerName::BrewingStandFuel);
    let entry = registered("minecraft:blaze_powder");
    let mut ledger = station_ledger(&entry, protocol::WINDOW_TYPE_BREWING_STAND);
    source_count(&mut ledger, 6);
    ledger.begin_storage_place_count(fuel.first, 6).unwrap();
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Storage(fuel.first))
            .unwrap()
            .count,
        6
    );
    assert!(ledger.cursor_stack().is_none());
}
