use bedrock_protocol_1_26_51::wire;

use super::{InventoryPacketError, MAX_ITEM_NBT_BYTES};

/// Bounds an item's user-data buffer and checks the compound it may carry.
///
/// 1.26.40 hands this over as opaque bytes, so the header is read exactly as
/// gophertunnel's `Writer.itemUserData` writes it
/// (`minecraft/protocol/writer.go`): an `int16` of `-1` introduces a `uint8`
/// version and a fixed little-endian compound, `0` means no compound. The
/// trailing canPlaceOn/canBreak lists and shield blocking tick are carried
/// through verbatim and never re-encoded field-by-field, which is what made the
/// protocol-1001 per-string length checks necessary.
pub(super) fn validate_item_user_data(extra: &[u8]) -> Result<(), InventoryPacketError> {
    if extra.len() > MAX_ITEM_NBT_BYTES {
        return Err(InventoryPacketError::ItemExtraTooLarge {
            bytes: extra.len(),
            max: MAX_ITEM_NBT_BYTES,
        });
    }
    if extra.is_empty() {
        return Ok(());
    }
    let header = extra
        .get(..2)
        .ok_or(InventoryPacketError::InvalidItemExtra)?;
    match i16::from_le_bytes([header[0], header[1]]) {
        0 => Ok(()),
        -1 => {
            let version = *extra.get(2).ok_or(InventoryPacketError::InvalidItemExtra)?;
            if version != 1 {
                return Err(InventoryPacketError::UnsupportedItemNbtVersion(version));
            }
            // Only the compound is validated; the lists that follow it in the
            // same buffer mean trailing bytes are expected here.
            wire::Reader::new(&extra[3..])
                .read_nbt(wire::NbtVariant::Persistent)
                .map_err(|_| InventoryPacketError::InvalidItemExtra)?;
            Ok(())
        }
        _ => Err(InventoryPacketError::InvalidItemExtra),
    }
}
