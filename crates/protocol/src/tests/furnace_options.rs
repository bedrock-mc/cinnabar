use crate::{Packet, WorldEvent, into_world_event};
use bedrock_protocol_1_26_51::{packets, types};

#[test]
fn furnace_options_reach_the_inventory_owner() {
    let packet = Packet::new(packets::SetPlayerFurnaceOptions {
        furnace_type: bedrock_protocol_1_26_51::enums::FurnaceType::Smoker,
        furnace_options: types::FurnaceOptions::default(),
    });
    assert!(
        matches!(
            into_world_event(packet, 0).unwrap(),
            Some(WorldEvent::Inventory(_))
        ),
        "server furnace options must enter the ordered inventory stream"
    );
}

/// A packet with one explicit set of furnace preferences.
fn options() -> crate::FurnaceOptions {
    crate::FurnaceOptions {
        left_tab: crate::FurnaceTab::Search,
        filtering: true,
        layout: crate::FurnaceLayout::RecipeBook,
    }
}

#[test]
fn furnace_options_wire_shape_and_kind_are_preserved_in_both_directions() {
    for (kind, wire_kind) in [
        (crate::FurnaceKind::Furnace, 1),
        (crate::FurnaceKind::BlastFurnace, 2),
        (crate::FurnaceKind::Smoker, 3),
    ] {
        let packet = crate::furnace_options_packet(kind, options());
        // Batch header, packet size, packet ID, furnace type, then signed tab,
        // filtering and signed layout. These bytes are independent of the builder.
        assert_eq!(
            crate::encode(&packet).unwrap().as_ref(),
            &[0xfe, 6, 0xdf, 0x02, wire_kind, 8, 1, 4]
        );
        let encoded = crate::encode(&packet).unwrap();
        let decoded = crate::decode_batch(encoded).unwrap().pop().unwrap();
        let event = into_world_event(decoded, 0).unwrap().unwrap();
        assert_eq!(
            event,
            WorldEvent::Inventory(crate::InventoryEvent::FurnaceOptions(
                crate::FurnaceOptionsEvent {
                    kind,
                    options: options()
                }
            ))
        );
    }
}

#[test]
fn unfamiliar_furnace_preferences_survive_and_unsupported_types_are_skipped() {
    for index in [i32::MIN, -1, 0, 1, 2, 3, 4, 5, i32::MAX] {
        let options = crate::FurnaceOptions {
            left_tab: crate::FurnaceTab::from_index(index),
            layout: crate::FurnaceLayout::from_index(index),
            filtering: false,
        };
        let packet = crate::furnace_options_packet(crate::FurnaceKind::Furnace, options);
        let Some(WorldEvent::Inventory(crate::InventoryEvent::FurnaceOptions(event))) =
            into_world_event(packet, 0).unwrap()
        else {
            panic!("missing preferences")
        };
        assert_eq!(event.options.left_tab.index(), index);
        assert_eq!(event.options.layout.index(), index);
    }
    for furnace_type in [
        bedrock_protocol_1_26_51::enums::FurnaceType::None,
        bedrock_protocol_1_26_51::enums::FurnaceType::Unknown(255),
    ] {
        let packet = Packet::new(packets::SetPlayerFurnaceOptions {
            furnace_type,
            furnace_options: types::FurnaceOptions::default(),
        });
        assert!(into_world_event(packet, 0).unwrap().is_none());
    }
}
