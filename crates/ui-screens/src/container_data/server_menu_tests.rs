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
