use protocol::WorldEvent;

use crate::ui_runtime::{UiRuntime, UiRuntimeError};

use super::session;

pub(crate) fn route_inventory_ingress(
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
    runtime.enqueue_inventory_event(session_generation, sequence, event)?;
    Ok(sequence)
}

pub(crate) fn route_item_registry_ingress(
    runtime: &mut UiRuntime,
    sequenced: &session::SequencedWorldEvent,
) -> Result<(), UiRuntimeError> {
    let WorldEvent::ItemActor(protocol::ItemActorEvent::Registry(event)) = &sequenced.event else {
        unreachable!("item-registry routing accepts only registry world events")
    };
    runtime.enqueue_item_registry_event(
        sequenced.session_generation,
        sequenced.sequence,
        event.clone(),
    )
}
