use super::banner_tests::loom_fixture;
use super::*;

/// The anvil bindings `cost` sets with no target placed.
fn anvil_bindings(cost: Option<crate::container_source::AnvilCost>) -> DataSource {
    let mut source = loom_fixture();
    source.screen.anvil_cost = cost;
    let mut data = DataSource::new();
    anvil_cost(&source, &mut data);
    data
}

/// The bindings expected for cost line `text` in the given colours, without a cross.
fn expected(text: &str, green: bool, red: bool) -> DataSource {
    let mut data = DataSource::new();
    data.set_global("#cost_text", Scalar::Text(text.to_owned()));
    data.set_global("#cost_text_green", Scalar::Bool(green));
    data.set_global("#cost_text_red", Scalar::Bool(red));
    data.set_global("#cross_out_icon", Scalar::Bool(false));
    data
}

#[test]
fn the_anvil_cost_line_colours_by_affordability_and_names_too_expensive() {
    use crate::container_source::AnvilCost;
    assert_eq!(anvil_bindings(None), expected("", false, false));
    let affordable = AnvilCost {
        levels: 5,
        too_expensive: false,
        affordable: true,
    };
    assert_eq!(anvil_bindings(Some(affordable)), expected("5", true, false));
    let short = AnvilCost {
        affordable: false,
        ..affordable
    };
    assert_eq!(anvil_bindings(Some(short)), expected("5", false, true));
    let expensive = AnvilCost {
        levels: 41,
        too_expensive: true,
        affordable: false,
    };
    assert_eq!(
        anvil_bindings(Some(expensive)),
        expected("Too Expensive!", false, true)
    );
}

#[test]
fn server_outputs_suppress_local_cost_and_invalid_result_indicators() {
    use crate::container_source::AnvilCost;
    for cost in [
        None,
        Some(AnvilCost {
            levels: 41,
            too_expensive: true,
            affordable: false,
        }),
        Some(AnvilCost {
            levels: 5,
            too_expensive: false,
            affordable: false,
        }),
    ] {
        let mut source = loom_fixture();
        source.screen.anvil_cost = cost;
        let ledger = source.player.inventory.ledger_mut();
        ledger.apply(&protocol::InventoryEvent::Open(
            protocol::ContainerOpenEvent {
                container: protocol::ContainerIdentity::window(5),
                window_type: protocol::WINDOW_TYPE_ANVIL,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            },
        ));
        let mut slots = vec![protocol::NetworkItemStack::empty(); protocol::UI_SLOT_COUNT];
        slots[usize::from(ANVIL_TARGET_SLOT)] = protocol::NetworkItemStack {
            network_id: 1,
            count: 1,
            ..protocol::NetworkItemStack::empty()
        };
        slots[usize::from(protocol::CREATED_OUTPUT_SLOT)] = protocol::NetworkItemStack {
            network_id: 2,
            count: 1,
            ..protocol::NetworkItemStack::empty()
        };
        ledger.apply(&protocol::InventoryEvent::Content(
            protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity {
                    window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
                    slot_type: Some(0),
                    dynamic_id: None,
                },
                slots: slots.into(),
                storage_item: protocol::NetworkItemStack::empty(),
            },
        ));
        assert!(ledger.created_output_stack().is_some());
        let mut data = DataSource::new();
        anvil_cost(&source, &mut data);
        assert_eq!(data, expected("", false, false));
        source
            .player
            .inventory
            .ledger_mut()
            .apply(&protocol::InventoryEvent::Slot(
                protocol::InventorySlotEvent {
                    identity: protocol::SlotIdentity {
                        container: protocol::ContainerIdentity {
                            window_id: None,
                            slot_type: Some(protocol::CONTAINER_NAME_CREATED_OUTPUT),
                            dynamic_id: None,
                        },
                        slot: u16::from(protocol::CREATED_OUTPUT_SLOT),
                    },
                    stack: protocol::NetworkItemStack::empty(),
                    storage_item: None,
                },
            ));
        let mut predicted = DataSource::new();
        anvil_cost(&source, &mut predicted);
        let mut expected = match cost {
            None => expected("", false, false),
            Some(cost) if cost.too_expensive => expected("Too Expensive!", false, true),
            Some(_) => expected("5", false, true),
        };
        expected.set_global(
            "#cross_out_icon",
            Scalar::Bool(cost.is_none_or(|cost| cost.too_expensive)),
        );
        assert_eq!(
            predicted, expected,
            "local indicators return when the server output is removed"
        );
    }
}

#[test]
fn authoritative_anvil_results_hide_prediction_failure_indicators() {
    use crate::container_source::AnvilCost;
    for cost in [
        None,
        Some(AnvilCost {
            levels: 50,
            too_expensive: true,
            affordable: false,
        }),
    ] {
        let mut source = loom_fixture();
        source.screen.anvil_cost = cost;
        let ledger = source.player.inventory.ledger_mut();
        ledger.apply(&protocol::InventoryEvent::Open(
            protocol::ContainerOpenEvent {
                container: protocol::ContainerIdentity::window(5),
                window_type: protocol::WINDOW_TYPE_ANVIL,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            },
        ));
        for slot in [ANVIL_TARGET_SLOT, protocol::CREATED_OUTPUT_SLOT] {
            ledger.apply(&protocol::InventoryEvent::Slot(
                protocol::InventorySlotEvent {
                    identity: protocol::SlotIdentity {
                        container: protocol::ContainerIdentity {
                            window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
                            slot_type: Some(0),
                            dynamic_id: None,
                        },
                        slot: u16::from(slot),
                    },
                    stack: protocol::NetworkItemStack {
                        network_id: 1,
                        count: 1,
                        stack_network_id: 30 + i32::from(slot),
                        ..Default::default()
                    },
                    storage_item: None,
                },
            ));
        }
        assert!(ledger.created_output_stack().is_some());
        assert!(
            ledger
                .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
                .is_some()
        );
        let mut data = DataSource::new();
        anvil_cost(&source, &mut data);
        assert_eq!(data, expected("", false, false));
    }
}
