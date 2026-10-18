use crate::{
    InventoryEvent, InventoryLayout, InventoryOptions, InventoryRightTab, InventoryTab, WorldEvent,
};

#[test]
fn inventory_options_wire_values_round_trip_without_screen_normalization() {
    for index in [i32::MIN, -1, 0, 1, 2, 3, 4, 5, 6, i32::MAX] {
        let options = InventoryOptions {
            left_tab: InventoryTab::from_index(index),
            right_tab: InventoryRightTab::from_index(index),
            filtering: true,
            inventory_layout: InventoryLayout::from_index(index),
            crafting_layout: InventoryLayout::from_index(!index),
        };
        let bytes = crate::encode(&crate::inventory_options_packet(options)).unwrap();
        let packet = crate::decode_batch(bytes).unwrap().pop().unwrap();
        assert_eq!(
            crate::into_world_event(packet, 0).unwrap(),
            Some(WorldEvent::Inventory(InventoryEvent::Options(options)))
        );
        assert_eq!(options.left_tab.index(), index);
        assert_eq!(options.right_tab.index(), index);
        assert_eq!(options.inventory_layout.index(), index);
        assert_eq!(options.crafting_layout.index(), !index);
    }
}

#[test]
fn inventory_options_have_the_expected_packet_shape() {
    let options = InventoryOptions {
        filtering: true,
        inventory_layout: InventoryLayout::InventoryOnly,
        crafting_layout: InventoryLayout::RecipeBook,
        ..Default::default()
    };
    assert_eq!(
        crate::encode(&crate::inventory_options_packet(options))
            .unwrap()
            .as_ref(),
        &[0xfe, 7, 0xb3, 2, 10, 4, 1, 2, 4]
    );
}
