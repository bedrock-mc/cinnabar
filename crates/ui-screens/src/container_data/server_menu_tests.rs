use super::*;

#[test]
fn container_collection_shows_exactly_the_admitted_menu_cells() {
    let mut source = super::banner_tests::loom_fixture();
    let ledger = source.player.inventory.ledger_mut();
    ledger.apply(&protocol::InventoryEvent::Open(
        protocol::ContainerOpenEvent {
            container: protocol::ContainerIdentity::window(8),
            window_type: protocol::WINDOW_TYPE_CONTAINER,
            position: [0; 3],
            runtime_entity_id: -1,
        },
    ));
    let kind = crate::container_kinds::storage_kind(200, None);
    let capacity = kind.collections[0].1.len();
    for count in [1, 27, 54, 200, protocol::MAX_GENERIC_STORAGE_SLOTS] {
        source
            .player
            .inventory
            .ledger_mut()
            .apply(&protocol::InventoryEvent::Content(
                protocol::InventoryContentEvent {
                    container: protocol::ContainerIdentity::window(8),
                    slots: vec![protocol::NetworkItemStack::empty(); count].into(),
                    storage_item: protocol::NetworkItemStack::empty(),
                },
            ));
        let shown = collection_len(&source.player, "container_items", capacity);
        assert_eq!(shown, count);
        assert_eq!(
            kind.cell("container_items", shown - 1),
            Some(crate::container_kinds::Cell::Storage((count - 1) as u8)),
        );
    }
}

#[test]
fn classic_trade_selection_keeps_an_occupied_second_payment_slot_visible() {
    use json_ui::vanilla::Binding;
    use std::sync::Arc;
    let mut source = super::banner_tests::loom_fixture();
    let item = protocol::TradeItem {
        identifier: Arc::from("minecraft:banner"),
        metadata: 0,
        count: 1,
        block_descriptor: None,
        extra_data: Arc::from([0; 10]),
    };
    let offer = protocol::TradeOffer {
        network_id: 91,
        inputs: [Some(item.clone()), Some(item.clone())],
        output: item,
        tier: 0,
        uses: 0,
        max_uses: 16,
    };
    let mut single = offer.clone();
    single.network_id += 1;
    single.inputs[1] = None;
    let mut locked = single.clone();
    locked.network_id += 1;
    locked.tier = 1;
    source
        .player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Trade(protocol::TradeEvent {
            container: protocol::ContainerIdentity::window(8),
            actor_unique_id: 90,
            title: Arc::from("Trader"),
            modern: false,
            tier: 0,
            offers: Arc::from([offer, locked, single]),
        }));
    for occupied in [true, false] {
        let ledger = source.player.inventory.ledger_mut();
        ledger.apply(&protocol::InventoryEvent::Slot(
            protocol::InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: protocol::ContainerIdentity::window(
                        protocol::UI_INVENTORY_WINDOW_ID,
                    ),
                    slot: u16::from(protocol::LEGACY_TRADE_SLOTS[1]),
                },
                stack: if occupied {
                    protocol::NetworkItemStack {
                        network_id: 1,
                        count: 1,
                        ..protocol::NetworkItemStack::empty()
                    }
                } else {
                    protocol::NetworkItemStack::empty()
                },
                storage_item: None,
            },
        ));
        ledger.select_trade(0).unwrap();
        let mut data = json_ui::DataSource::new();
        crate::trading::data(&source, &mut data);
        assert_eq!(
            data.global_value(Binding::RightButtonEnabled.as_str()),
            Some(&json_ui::Scalar::Bool(true))
        );
        assert_eq!(
            data.global_value(Binding::LeftButtonEnabled.as_str()),
            Some(&json_ui::Scalar::Bool(false))
        );
        source
            .player
            .inventory
            .ledger_mut()
            .cycle_trade(true)
            .unwrap();
        let mut data = json_ui::DataSource::new();
        crate::trading::data(&source, &mut data);
        assert_eq!(
            data.global_value(Binding::ShowSecondTradeSlot.as_str()),
            Some(&json_ui::Scalar::Bool(occupied))
        );
        assert_eq!(
            data.global_value(Binding::RightButtonEnabled.as_str()),
            Some(&json_ui::Scalar::Bool(false))
        );
        assert_eq!(
            data.global_value(Binding::LeftButtonEnabled.as_str()),
            Some(&json_ui::Scalar::Bool(true))
        );
    }
}
