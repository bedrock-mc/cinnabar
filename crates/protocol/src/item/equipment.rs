use super::{HOTBAR_SLOT_COUNT, NetworkItemStack};
use crate::inventory::{InventoryPacketError, VerifiedNetworkItemStack};
use bedrock_protocol_1_26_51::{
    packets::MobEquipment as MobEquipmentPacket, types::ActorRuntimeID, wire,
};

const AUXILIARY_WILDCARD: u32 = protocol_ids::ITEM_AUXILIARY_WILDCARD as u32;

/// Compares held item descriptors independently of count and stack network IDs.
/// Either auxiliary may be a wildcard. Compound entry order is ignored; trailer bytes
/// and other item fields remain exact.
#[must_use]
pub fn held_items_match(a: &NetworkItemStack, b: &NetworkItemStack) -> bool {
    a.network_id == b.network_id
        && (a.metadata == b.metadata
            || a.metadata == AUXILIARY_WILDCARD
            || b.metadata == AUXILIARY_WILDCARD)
        && a.block_runtime_id == b.block_runtime_id
        && super::nbt_equal::same_extra(&a.extra_data, &b.extra_data)
}

/// Builds the vanilla outbound packet announcing a local hotbar-slot selection.
///
/// The vanilla Bedrock client owns hotbar-slot selection locally and notifies the server with a
/// `MobEquipment` packet against the inventory window (`PlayerHotbar` is server->client and is not
/// what a client sends). The selected stack must be the exact known player-inventory value: this
/// boundary verifies its retained bytes and shape before converting it to the vendor wire type.
/// `runtime_id` must be the local player's StartGame-assigned runtime id — servers reject a
/// foreign runtime id on this packet.
pub fn select_hotbar_slot_packet(
    runtime_id: u64,
    slot: u8,
    stack: &NetworkItemStack,
) -> Result<crate::Packet, InventoryPacketError> {
    if slot >= HOTBAR_SLOT_COUNT {
        return Err(InventoryPacketError::InvalidSelectedSlot(i32::from(slot)));
    }
    held_equipment_packet(
        runtime_id,
        slot,
        slot,
        crate::PLAYER_INVENTORY_WINDOW_ID as u8,
        stack,
    )
}

/// Builds an offhand equipment update with the offhand equipment address.
pub fn offhand_equipment_packet(
    runtime_id: u64,
    stack: &NetworkItemStack,
) -> Result<crate::Packet, InventoryPacketError> {
    held_equipment_packet(runtime_id, 1, 0, crate::OFFHAND_WINDOW_ID as u8, stack)
}

/// Validates a held stack and builds an equipment announcement for either hand.
fn held_equipment_packet(
    runtime_id: u64,
    slot: u8,
    selected_slot: u8,
    container: u8,
    stack: &NetworkItemStack,
) -> Result<crate::Packet, InventoryPacketError> {
    // The equipment descriptor omits a stack network id, so a
    // predicted request id is neither validated nor sent.
    let mut untracked = stack.clone();
    if !untracked.is_empty() {
        untracked.stack_network_id = -1;
    }
    let item =
        VerifiedNetworkItemStack::try_new(untracked, stack.nbt_digest)?.into_vendor_item()?;
    Ok(crate::Packet::new(MobEquipmentPacket {
        target_runtime_id: ActorRuntimeID(runtime_id),
        item,
        slot: wire::U8(slot),
        selected_slot: wire::U8(selected_slot),
        container_id: wire::U8(container),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Either descriptor may carry a wildcard without hiding a different item or block state.
    #[test]
    fn held_item_auxiliary_wildcard_preserves_other_identity_checks() {
        let ordinary = NetworkItemStack {
            network_id: 7,
            count: 1,
            metadata: 5,
            ..NetworkItemStack::empty()
        };
        let wildcard = NetworkItemStack {
            metadata: AUXILIARY_WILDCARD,
            ..ordinary.clone()
        };
        assert!(held_items_match(&ordinary, &wildcard));
        assert!(held_items_match(&wildcard, &ordinary));
        let mut different = ordinary.clone();
        different.metadata += 1;
        assert!(!held_items_match(&ordinary, &different));
        different.network_id += 1;
        assert!(!held_items_match(&wildcard, &different));
        different = ordinary;
        different.block_runtime_id = 1;
        assert!(!held_items_match(&wildcard, &different));
    }
}
