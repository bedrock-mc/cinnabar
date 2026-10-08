//! Requests missing held and framed map images through the shared retry budget.

use super::*;

/// Seconds before an unanswered map request is repeated.
const MAP_REQUEST_RETRY_SECONDS: f64 = 5.0;

/// Asks the server for the pixels of held and framed maps whose images have not arrived.
pub(crate) fn request_missing_maps(
    mut runtime: ResMut<BlockEntityRuntime>,
    network: Option<Res<crate::runtime::network::NetworkHandle>>,
    time: Res<Time<Real>>,
    world: Res<ClientWorld>,
    player: Res<crate::player_runtime::PlayerRuntime>,
    ui: Res<UiRuntime>,
) {
    let Some(network) = network else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let runtime = &mut *runtime;
    if let Some(stream) = world.stream.as_ref() {
        let offhand = ui
            .inventory_ledger(&player)
            .target_stack(client_ui::ui_runtime::inventory_ledger::InventoryTarget::Offhand);
        for stack in [player.selected_stack(), offhand].into_iter().flatten() {
            let Some(item) = stream.authority().canonical_item_stack(stack) else {
                continue;
            };
            if item.identifier.as_deref() == Some("minecraft:filled_map")
                && let Some(id) = item
                    .map_id
                    .filter(|id| stream.authority().map_image(*id).is_none())
            {
                runtime.missing_maps.push(id);
            }
        }
    }
    runtime.missing_maps.sort_unstable();
    runtime.missing_maps.dedup();
    for id in std::mem::take(&mut runtime.missing_maps) {
        let due = runtime
            .map_requests
            .get(&id)
            .is_none_or(|last| now - last >= MAP_REQUEST_RETRY_SECONDS);
        if due
            && network
                .send_inventory_packet(protocol::map_info_request_packet(id))
                .is_ok()
        {
            runtime.map_requests.insert(id, now);
        }
    }
    if runtime.map_requests.len() > 256 {
        runtime
            .map_requests
            .retain(|_, last| now - *last < MAP_REQUEST_RETRY_SECONDS);
    }
}
