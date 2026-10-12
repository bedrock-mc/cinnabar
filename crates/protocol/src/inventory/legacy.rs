use bedrock_protocol_1_26_51::enums::{
    InventorySourceInventorySourceFlags as Flags, InventorySourceType as SourceType,
};
use bedrock_protocol_1_26_51::packets::InventoryTransaction;
use bedrock_protocol_1_26_51::types::{
    InventoryAction, InventorySource, InventoryTransactionData, InventoryTransactionPacketData,
};
use bedrock_protocol_1_26_51::wire;

use super::{InventoryPacketError, MAX_CONTAINER_SLOTS, VerifiedNetworkItemStack};

/// A normal transaction's inventory write or dropped-item balancing leg.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NormalInventorySource {
    Container(i8),
    World { randomly: bool },
}

#[derive(Debug, Clone)]
pub struct NormalInventoryChange {
    pub source: NormalInventorySource,
    pub slot: u32,
    pub from: VerifiedNetworkItemStack,
    pub to: VerifiedNetworkItemStack,
}

/// Serializes exact old/new descriptors without item-stack request identifiers.
pub fn normal_inventory_transaction_packet(
    changes: Vec<NormalInventoryChange>,
) -> Result<crate::Packet, InventoryPacketError> {
    if changes.is_empty() || changes.len() > MAX_CONTAINER_SLOTS {
        return Err(InventoryPacketError::InvalidNormalTransactionActionCount(
            changes.len(),
        ));
    }
    let actions = changes
        .into_iter()
        .map(|change| {
            let source = match change.source {
                NormalInventorySource::Container(window_id) => InventorySource {
                    source_type: SourceType::ContainerInventory,
                    container_id: Some(wire::I8(window_id)),
                    bit_flags: None,
                },
                NormalInventorySource::World { randomly } => InventorySource {
                    source_type: SourceType::WorldInteraction,
                    container_id: None,
                    bit_flags: Some(if randomly {
                        Flags::WorldInteractionRandom
                    } else {
                        Flags::NoFlag
                    }),
                },
            };
            Ok(InventoryAction {
                source,
                slot: wire::VarUInt(change.slot),
                from_item: change.from.into_vendor_item()?,
                to_item: change.to.into_vendor_item()?,
            })
        })
        .collect::<Result<_, InventoryPacketError>>()?;
    Ok(crate::Packet::new(InventoryTransaction {
        transaction: InventoryTransactionPacketData::NormalTransactionData {
            actions: InventoryTransactionData { actions },
        },
        ..Default::default()
    }))
}
