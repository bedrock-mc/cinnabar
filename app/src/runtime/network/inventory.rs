use protocol::{InventoryEvent, ItemRegistryEvent, WorldEvent};

use client_ui::ui_runtime::{UiRuntime, UiRuntimeError};

use super::session;

pub(crate) fn publish_bootstrap_inventory(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    registry: Option<ItemRegistryEvent>,
    inventory: InventoryEvent,
) -> bool {
    let InventoryEvent::Authority(authority) = inventory else {
        return false;
    };
    runtime.publish_crafting_bootstrap(player_runtime, registry.as_ref(), authority);
    if let Some(registry) = registry {
        runtime
            .inventory_ledger_mut(player_runtime)
            .apply_registry(&registry);
    }
    runtime.publish_inventory_authority(player_runtime, authority);
    true
}

pub(crate) fn route_inventory_ingress(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    sequenced: session::SequencedWorldEvent,
) -> Result<u64, UiRuntimeError> {
    let session::SequencedWorldEvent {
        session_generation,
        sequence,
        event: WorldEvent::Inventory(event),
    } = sequenced
    else {
        unreachable!("inventory routing accepts only inventory world events")
    };
    super::item_diagnostics::inventory(&event);
    runtime.enqueue_inventory_event(player_runtime, session_generation, sequence, event)?;
    Ok(sequence)
}

pub(crate) fn route_item_registry_ingress(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    sequenced: &session::SequencedWorldEvent,
) -> Result<(), UiRuntimeError> {
    let WorldEvent::ItemActor(protocol::ItemActorEvent::Registry(event)) = &sequenced.event else {
        unreachable!("item-registry routing accepts only registry world events")
    };
    runtime.enqueue_item_registry_event(
        player_runtime,
        sequenced.session_generation,
        sequenced.sequence,
        event.clone(),
    )
}
