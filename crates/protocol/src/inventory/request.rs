use bedrock_protocol_1_26_51::enums::{
    InteractAction, PacketViolationSeverity, PacketViolationType, TextProcessingEventOrigin,
};
use bedrock_protocol_1_26_51::packets::{
    ContainerClose, Interact, ItemStackRequest, PacketViolationWarning,
};
use bedrock_protocol_1_26_51::types::{
    ActorRuntimeID, ItemStackRequestID, ItemStackRequestPacketData,
};
use bedrock_protocol_1_26_51::wire;

use super::InventoryPacketError;
mod actions;
#[cfg(test)]
mod batch_tests;
pub(super) mod mining;

pub use actions::{
    ARMOR_SLOTS, AutoCraftIngredient, CRAFTING_INPUT_SLOTS, CREATED_OUTPUT_SLOT, CraftResult,
    MAX_STACK_REQUEST_AMOUNT, PLAYER_INVENTORY_SLOTS, StackItemDescriptor, StackRequestAction,
    StackRequestContainer, StackRequestSlot,
};

/// Actions one request may carry.
pub const MAX_STACK_REQUEST_ACTIONS: usize = 100;

/// Reports an excess container open while the same window is already open.
pub fn repeated_container_open_warning_packet() -> crate::Packet {
    crate::Packet::new(PacketViolationWarning {
        violation_type: PacketViolationType::Unknown,
        violation_severity: PacketViolationSeverity::Warning,
        // Bedrock names the warning packet itself in this acknowledgement.
        violation_packet_id: wire::ZigZag32(PacketViolationWarning::ID as i32),
        violation_context: wire::Str::from("Container is already open"),
    })
}

pub fn open_inventory_packet(
    target_runtime_id: u64,
) -> Result<crate::Packet, InventoryPacketError> {
    if target_runtime_id == 0 {
        return Err(InventoryPacketError::InvalidInventoryTargetRuntimeId);
    }
    Ok(crate::Packet::new(Interact {
        action: InteractAction::Openinventory,
        target_runtime_id: ActorRuntimeID(target_runtime_id),
        position: None,
    }))
}

/// Builds one request. Valid negative odd stack ids name an earlier sparse
/// prediction, or this request's own created output.
pub fn item_stack_request_packet(
    request_id: i32,
    actions: &[StackRequestAction],
) -> Result<crate::Packet, InventoryPacketError> {
    item_stack_request_packet_filtered(request_id, actions, &[])
}

/// Text an anvil rename carries for server-side filtering.
pub const MAX_FILTER_STRINGS: usize = 4;

/// Like [`item_stack_request_packet`], with the anvil-text strings a
/// craft-optional action's filter index points into.
pub fn item_stack_request_packet_filtered(
    request_id: i32,
    actions: &[StackRequestAction],
    filter_strings: &[String],
) -> Result<crate::Packet, InventoryPacketError> {
    Ok(crate::Packet::new(ItemStackRequest {
        requests: vec![encode_request(request_id, actions, filter_strings)?],
    }))
}

/// Builds the native request vector without combining actions or filtering text across requests.
pub fn item_stack_request_batch<'a>(
    requests: impl IntoIterator<Item = (i32, &'a [StackRequestAction], &'a [String])>,
) -> Result<Option<crate::Packet>, InventoryPacketError> {
    let requests = requests
        .into_iter()
        .map(|(id, actions, strings)| encode_request(id, actions, strings))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((!requests.is_empty()).then(|| crate::Packet::new(ItemStackRequest { requests })))
}

/// Validates each request independently, including its own created-output identifiers.
fn encode_request(
    request_id: i32,
    actions: &[StackRequestAction],
    filter_strings: &[String],
) -> Result<ItemStackRequestPacketData, InventoryPacketError> {
    if filter_strings.len() > MAX_FILTER_STRINGS {
        return Err(InventoryPacketError::InvalidStackRequestActionCount(
            filter_strings.len(),
        ));
    }
    if request_id >= -1 || request_id & 1 == 0 {
        return Err(InventoryPacketError::InvalidStackRequestId);
    }
    if actions.is_empty() || actions.len() > MAX_STACK_REQUEST_ACTIONS {
        return Err(InventoryPacketError::InvalidStackRequestActionCount(
            actions.len(),
        ));
    }
    for slot in actions.iter().flat_map(action_slots) {
        let created_output = slot.container == StackRequestContainer::CreatedOutput
            && slot.stack_network_id == request_id;
        let prior_request = slot.stack_network_id < -1
            && slot.stack_network_id & 1 != 0
            && slot.stack_network_id > request_id;
        // Legacy transactions and item stack requests have independent id sequences.
        let prior_legacy = super::is_legacy_request_id(slot.stack_network_id);
        if slot.stack_network_id < -1 && !created_output && !prior_request && !prior_legacy {
            return Err(InventoryPacketError::InvalidRequestStackNetworkId(
                slot.stack_network_id,
            ));
        }
    }
    Ok(ItemStackRequestPacketData {
        client_request_id: ItemStackRequestID(request_id),
        actions: actions
            .iter()
            .map(actions::encode)
            .collect::<Result<_, _>>()?,
        strings_to_filter: filter_strings
            .iter()
            .map(|text| wire::Str::from(text.as_str()))
            .collect(),
        strings_to_filter_origin: if filter_strings.is_empty() {
            TextProcessingEventOrigin::Unknown
        } else {
            TextProcessingEventOrigin::Anviltext
        },
    })
}

fn action_slots(action: &StackRequestAction) -> impl Iterator<Item = StackRequestSlot> {
    let (first, second) = match action {
        StackRequestAction::Take {
            source,
            destination,
            ..
        }
        | StackRequestAction::Place {
            source,
            destination,
            ..
        }
        | StackRequestAction::Swap {
            source,
            destination,
        } => (Some(*source), Some(*destination)),
        StackRequestAction::Drop { source, .. }
        | StackRequestAction::Destroy { source, .. }
        | StackRequestAction::Consume { source, .. } => (Some(*source), None),
        _ => (None, None),
    };
    first.into_iter().chain(second)
}

/// Closes the named window using the native client's unspecified container type.
pub fn container_close_packet(window_id: i32) -> Result<crate::Packet, InventoryPacketError> {
    let container_id = match window_id {
        -128..=-1 => (window_id as i8).to_ne_bytes()[0],
        0..=255 => window_id as u8,
        _ => {
            return Err(InventoryPacketError::InvalidContainerCloseWindowId(
                window_id,
            ));
        }
    };
    Ok(crate::Packet::new(ContainerClose {
        container_id: wire::U8(container_id),
        container_type: wire::U8(super::NO_CONTAINER_WINDOW_TYPE.to_ne_bytes()[0]),
        server_initiated_close: false,
    }))
}

#[cfg(test)]
mod tests {
    use bedrock_protocol_1_26_51::packets;
    use bedrock_protocol_1_26_51::wire::{Decode, Encode};

    use super::*;

    #[test]
    fn repeated_open_warning_round_trips_with_the_menu_acknowledgement_fields() {
        let packet = repeated_container_open_warning_packet();
        let packets::Packet::PacketViolationWarning(warning) = *packet.data else {
            panic!("expected a packet violation warning");
        };
        let decoded =
            PacketViolationWarning::decode_exact(&warning.encode_to_vec().unwrap()).unwrap();
        assert_eq!(decoded.violation_type, PacketViolationType::Unknown);
        assert_eq!(decoded.violation_severity, PacketViolationSeverity::Warning);
        assert_eq!(
            decoded.violation_packet_id.0,
            PacketViolationWarning::ID as i32
        );
    }

    #[test]
    fn client_close_uses_native_unspecified_type() {
        let packet = container_close_packet(4).unwrap();
        let packets::Packet::ContainerClose(close) = *packet.data else {
            panic!("expected ContainerClose packet");
        };
        assert_eq!(close.container_id.0, 4);
        assert_eq!(close.container_type.0, 0xf7);
        assert!(!close.server_initiated_close);
    }

    #[test]
    fn personal_inventory_open_targets_self_without_a_position() {
        let packet = open_inventory_packet(42).unwrap();
        let packets::Packet::Interact(interact) = *packet.data else {
            panic!("expected Interact packet");
        };
        assert_eq!(interact.action, InteractAction::Openinventory);
        assert_eq!(interact.target_runtime_id.0, 42);
        assert_eq!(interact.position, None);
        assert_eq!(
            open_inventory_packet(0).unwrap_err(),
            InventoryPacketError::InvalidInventoryTargetRuntimeId
        );
    }

    #[test]
    fn prior_sparse_request_ids_round_trip_on_both_slots() {
        let slot = |id| StackRequestSlot {
            container: StackRequestContainer::PlayerInventory,
            slot: 5,
            stack_network_id: id,
        };
        let packet = item_stack_request_packet(
            -7,
            &[StackRequestAction::Place {
                amount: 1,
                source: slot(-3),
                destination: slot(-5),
            }],
        )
        .unwrap();
        let packets::Packet::ItemStackRequest(request) = *packet.data else {
            panic!()
        };
        let bytes = request.encode_to_vec().unwrap();
        let decoded = ItemStackRequest::decode_exact(&bytes).unwrap();
        assert_eq!(decoded, request);
        let drop_from = |id| {
            item_stack_request_packet(
                -7,
                &[StackRequestAction::Drop {
                    amount: 1,
                    source: slot(id),
                    randomly: false,
                }],
            )
        };
        for invalid in [-9, -7] {
            assert!(drop_from(invalid).is_err());
        }
        // A stack an earlier drop transaction left behind is named by its legacy id.
        assert!(drop_from(-4).is_ok());
        assert!(drop_from(-8).is_ok());
    }
}
