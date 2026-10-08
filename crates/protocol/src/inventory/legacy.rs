use valentine::bedrock::version::v1_26_51::{
    EnumsInventorySourceInventorySourceFlags as Flags, EnumsInventorySourceType as SourceType,
    InventoryAction, InventorySource, InventoryTransaction, InventoryTransactionPacket,
    InventoryTransactionPacketTransaction, NormalTransactionData,
};

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
                    source_type: SourceType::Containerinventory,
                    container_id: Some(window_id),
                    bit_flags: None,
                },
                NormalInventorySource::World { randomly } => InventorySource {
                    source_type: SourceType::Worldinteraction,
                    container_id: None,
                    bit_flags: Some(if randomly {
                        Flags::Worldinteractionrandom
                    } else {
                        Flags::Noflag
                    }),
                },
            };
            Ok(InventoryAction {
                source,
                slot: change.slot,
                from_item: change.from.into_vendor_item(0)?,
                to_item: change.to.into_vendor_item(0)?,
            })
        })
        .collect::<Result<_, InventoryPacketError>>()?;
    Ok(InventoryTransactionPacket {
        transaction: InventoryTransactionPacketTransaction::NormalTransactionData(
            NormalTransactionData {
                actions: InventoryTransaction { actions },
            },
        ),
        ..Default::default()
    }
    .into())
}
