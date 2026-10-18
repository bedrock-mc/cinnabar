//! Projects held stacks into the item identity used by tick-time hover reporting.
use crate::runtime::world::ClientWorld;

/// Ignores stack counts and inventory IDs; damageable armour uses its retained damage value.
pub(super) fn held_item_key(
    player: &client_state::PlayerRuntime,
    world: &ClientWorld,
) -> (i32, u16) {
    let Some(stack) = player.selected_stack().filter(|stack| !stack.is_empty()) else {
        return (0, 0);
    };
    let armour_damage = world.stream.as_ref().is_some_and(|stream| {
        let world = stream.authority();
        matches!(
            world.item_armour(stack.network_id),
            assets::item::armour::ItemArmour::Wearable { .. }
        ) && world
            .item_durability(stack.network_id)
            .maximum()
            .is_some_and(|maximum| maximum > 0)
    });
    let aux = if armour_damage {
        protocol::item_extra_damage(&stack.extra_data).unwrap_or(0)
    } else {
        stack.metadata
    };
    (stack.network_id, aux as u16)
}
