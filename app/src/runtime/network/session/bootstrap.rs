use protocol::{
    InventoryEvent, ItemActorEvent, ItemRegistryEvent, WorldEvent, into_world_event,
    normalize_authority,
};

pub(super) fn start_game_inventory_authority(game_data: &protocol::GameData) -> InventoryEvent {
    normalize_authority(game_data.start_game.enable_item_stack_net_manager)
}

pub(super) fn start_game_item_registry(
    game_data: &protocol::GameData,
    current_dimension: i32,
) -> Option<ItemRegistryEvent> {
    match into_world_event(game_data.item_registry.clone().into(), current_dimension) {
        Ok(Some(WorldEvent::ItemActor(ItemActorEvent::Registry(registry)))) => Some(registry),
        // A semantically unsupported registry removes merge authority but does
        // not turn an otherwise valid StartGame into a transport failure.
        Ok(_) | Err(_) => None,
    }
}
