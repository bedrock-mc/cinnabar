use bedrock_protocol_1_26_51::enums::{
    ContainerEnumName, InventorySourceInventorySourceFlags as Flags,
    InventorySourceType as SourceType,
};
use bedrock_protocol_1_26_51::packets::InventoryTransaction;
use bedrock_protocol_1_26_51::types::{
    InventoryAction, InventorySource, InventoryTransactionData, InventoryTransactionPacketData,
    ItemStackLegacyRequestID, LegacySetSlot,
};
use bedrock_protocol_1_26_51::wire;
use std::sync::atomic::{AtomicI32, Ordering};

use super::{InventoryPacketError, MAX_CONTAINER_SLOTS, VerifiedNetworkItemStack};

/// One process-wide namespace for drops, placements and air-use transactions.
static LEGACY_REQUEST_IDS: AtomicI32 = AtomicI32::new(FIRST_LEGACY_REQUEST_ID);
const FIRST_LEGACY_REQUEST_ID: i32 = -4;

/// Reserves a unique negative even transaction id, or refuses when the counter cannot advance.
/// Unsent transactions consume their reservation too, so retries never reuse another owner's id.
pub fn next_legacy_request_id() -> Option<i32> {
    allocate_legacy_request_id(&LEGACY_REQUEST_IDS)
}

/// Advances a legacy counter without wrapping into a previously allocated id.
fn allocate_legacy_request_id(counter: &AtomicI32) -> Option<i32> {
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_sub(2))
        .ok()
}

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

/// Whether `id` can name a legacy transaction; the native client numbers them negative and even,
/// apart from the odd item stack request ids.
#[must_use]
pub const fn is_legacy_request_id(id: i32) -> bool {
    id < -1 && id & 1 == 0
}

/// Serializes exact old/new descriptors without item-stack request identifiers.
pub fn normal_inventory_transaction_packet(
    changes: Vec<NormalInventoryChange>,
) -> Result<crate::Packet, InventoryPacketError> {
    normal_transaction(changes).map(crate::Packet::new)
}

/// A drop from player inventory `slot` with no window open, which the native client sends as a
/// normal transaction even when the server owns the inventory. A remainder stamped with
/// `legacy_request_id` asks the server to restate that slot; an emptied slot carries none.
pub fn inventory_drop_transaction_packet(
    slot: u8,
    legacy_request_id: Option<i32>,
    changes: Vec<NormalInventoryChange>,
) -> Result<crate::Packet, InventoryPacketError> {
    let mut transaction = normal_transaction(changes)?;
    if let Some(id) = legacy_request_id {
        if !is_legacy_request_id(id) {
            return Err(InventoryPacketError::InvalidLegacyRequestId(id));
        }
        transaction.legacy_request_id = ItemStackLegacyRequestID(id);
        transaction.legacy_set_item_slots = Some(vec![LegacySetSlot {
            container_enum: ContainerEnumName::Inventorycontainer,
            slots: vec![slot].into(),
        }]);
    }
    Ok(crate::Packet::new(transaction))
}

fn normal_transaction(
    changes: Vec<NormalInventoryChange>,
) -> Result<InventoryTransaction, InventoryPacketError> {
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
    Ok(InventoryTransaction {
        transaction: InventoryTransactionPacketData::NormalTransactionData {
            actions: InventoryTransactionData { actions },
        },
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_ids_are_even_and_never_reused_at_exhaustion() {
        let counter = AtomicI32::new(FIRST_LEGACY_REQUEST_ID);
        assert!(is_legacy_request_id(FIRST_LEGACY_REQUEST_ID));
        assert_eq!(
            allocate_legacy_request_id(&counter),
            Some(FIRST_LEGACY_REQUEST_ID)
        );
        assert_eq!(
            allocate_legacy_request_id(&counter),
            Some(FIRST_LEGACY_REQUEST_ID - 2)
        );
        counter.store(i32::MIN + 2, Ordering::Relaxed);
        assert_eq!(allocate_legacy_request_id(&counter), Some(i32::MIN + 2));
        assert_eq!(allocate_legacy_request_id(&counter), None);
        assert_eq!(allocate_legacy_request_id(&counter), None);
    }
}
