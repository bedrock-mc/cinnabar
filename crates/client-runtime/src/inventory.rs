use protocol::{InventoryEvent, ItemRegistryEvent, WorldEvent};

use client_ui::ui_runtime::{UiRuntime, UiRuntimeError};

use client_session::SequencedWorldEvent;

/// Publishes initial inventory authority and its optional item registry as one bootstrap.
pub fn publish_bootstrap_inventory(
    player_runtime: &mut client_state::PlayerRuntime,
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

/// Queues a sequenced inventory event and returns its FIFO commit position.
pub fn route_inventory_ingress(
    player_runtime: &mut client_state::PlayerRuntime,
    runtime: &mut UiRuntime,
    sequenced: SequencedWorldEvent,
) -> Result<u64, UiRuntimeError> {
    let SequencedWorldEvent {
        session_generation,
        sequence,
        event: WorldEvent::Inventory(event),
    } = sequenced
    else {
        unreachable!("inventory routing accepts only inventory world events")
    };
    super::item_diagnostics::inventory(&event);
    runtime.enqueue_inventory_event(
        player_runtime,
        session_generation.get(),
        sequence.get(),
        event,
    )?;
    Ok(sequence.get())
}

/// Queues item registry facts at the same generation and sequence as their world event.
pub fn route_item_registry_ingress(
    player_runtime: &mut client_state::PlayerRuntime,
    runtime: &mut UiRuntime,
    sequenced: &SequencedWorldEvent,
) -> Result<(), UiRuntimeError> {
    let WorldEvent::ItemActor(protocol::ItemActorEvent::Registry(event)) = &sequenced.event else {
        unreachable!("item-registry routing accepts only registry world events")
    };
    runtime.enqueue_item_registry_event(
        player_runtime,
        sequenced.session_generation.get(),
        sequenced.sequence.get(),
        event.clone(),
    )
}

/// Queues crafting rules alongside their world packet without moving its FIFO position.
pub fn route_crafting_rules_ingress(
    player_runtime: &mut client_state::PlayerRuntime,
    runtime: &mut UiRuntime,
    sequenced: &SequencedWorldEvent,
) -> Result<(), UiRuntimeError> {
    let WorldEvent::GameRules(rules) = &sequenced.event else {
        unreachable!("crafting-rule routing accepts only game-rule world events")
    };
    if rules.crafting.is_empty() {
        return Ok(());
    }
    runtime.enqueue_inventory_event(
        player_runtime,
        sequenced.session_generation.get(),
        sequenced.sequence.get(),
        InventoryEvent::CraftingRules(rules.crafting),
    )
}
