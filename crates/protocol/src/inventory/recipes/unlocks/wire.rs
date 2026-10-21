use std::{mem::size_of, sync::Arc};

use super::super::{
    budget::{Credits, MAX_RECORDS, MAX_STRING_BYTES, MAX_STRING_TOTAL_BYTES, MAX_UPDATE_BYTES},
    reader::{Reader, Refusal},
};
use super::{Names, RecipeUnlockOperation, RecipeUnlockUpdate};

/// Reads a packet body with bounded, allocation-free validation before retention.
pub fn decode_recipe_unlocks(
    body: &[u8],
) -> Result<RecipeUnlockUpdate, crate::InventoryPacketError> {
    decode_with_credits(body, &Credits::shared())
}

/// Reserves decoded strings from the same owner as recipe catalogs and authority.
pub(super) fn decode_with_credits(
    body: &[u8],
    credits: &Arc<Credits>,
) -> Result<RecipeUnlockUpdate, crate::InventoryPacketError> {
    let header = body
        .get(..4)
        .ok_or(crate::InventoryPacketError::MalformedWire)?;
    let operation = RecipeUnlockOperation::from_index(u32::from_le_bytes(
        header.try_into().expect("four-byte header"),
    ));
    let parse = || {
        let mut reader = Reader::new(&body[4..])?;
        let count = reader.count(MAX_RECORDS)?;
        let mut skipped = 0;
        for _ in 0..count {
            skipped += usize::from(reader.text()?.is_none());
        }
        reader.finish()?;
        let mut values = reserved_names(count, reader.string_bytes(), credits)?;
        let mut reader = Reader::new(&body[4..])?;
        reader.count(MAX_RECORDS)?;
        for _ in 0..count {
            if let Some(name) = reader.text()? {
                push_name(&mut values.0, name)?;
            }
        }
        if skipped > 0 {
            tracing::warn!(target: "cinnabar::inventory_requests", skipped, "skipped non-UTF-8 recipe unlock identifiers");
        }
        Ok(finish(values))
    };
    admit(operation, parse())
}

/// Normalizes already-decoded callers through the same bounds and credited representation.
pub(crate) fn normalize_recipe_unlocks(
    packet: bedrock_protocol_1_26_51::packets::UnlockedRecipes,
) -> RecipeUnlockUpdate {
    let operation = RecipeUnlockOperation::from_index(packet.packet_type.to_raw());
    let parse = || {
        let count = packet.unlocked_recipes_list.len();
        let bytes = packet
            .unlocked_recipes_list
            .iter()
            .try_fold(0usize, |sum, name| {
                if name.len() > MAX_STRING_BYTES {
                    return Err(Refusal::Policy);
                }
                sum.checked_add(name.len()).ok_or(Refusal::Policy)
            })?;
        let mut values = reserved_names(count, bytes, &Credits::shared())?;
        for name in &packet.unlocked_recipes_list {
            if let Ok(name) = name.to_str() {
                push_name(&mut values.0, name)?;
            } else {
                tracing::warn!(target: "cinnabar::inventory_requests", skipped = 1,
                    "skipped non-UTF-8 recipe unlock identifier");
            }
        }
        Ok(finish(values))
    };
    admit(operation, parse()).expect("owned wire strings cannot fail framing")
}

/// Reserves every vector element and string allocation before copying any names.
fn reserved_names(
    count: usize,
    bytes: usize,
    credits: &Arc<Credits>,
) -> Result<(Vec<String>, super::super::budget::Permit), Refusal> {
    if count > MAX_RECORDS || bytes > MAX_STRING_TOTAL_BYTES {
        return Err(Refusal::Policy);
    }
    let charge = count
        .checked_mul(size_of::<String>() + 64)
        .and_then(|n| n.checked_add(bytes))
        .and_then(|n| n.checked_add(128))
        .filter(|n| *n <= MAX_UPDATE_BYTES)
        .ok_or(Refusal::Policy)?;
    let permit = credits.reserve(charge).ok_or(Refusal::Policy)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| Refusal::Policy)?;
    Ok((values, permit))
}

/// Copies one exact string without namespace or case normalization.
fn push_name(values: &mut Vec<String>, name: &str) -> Result<(), Refusal> {
    let mut value = String::new();
    value
        .try_reserve_exact(name.len())
        .map_err(|_| Refusal::Policy)?;
    value.push_str(name);
    values.push(value);
    Ok(())
}

/// Converts duplicate wire names to a sorted set while retaining their original bytes.
fn finish((mut values, permit): (Vec<String>, super::super::budget::Permit)) -> Arc<Names> {
    values.sort_unstable();
    values.dedup();
    Arc::new(Names {
        values,
        _permit: permit,
    })
}

/// Keeps policy refusals observable without accepting partial authority.
fn admit(
    operation: RecipeUnlockOperation,
    result: Result<Arc<Names>, Refusal>,
) -> Result<RecipeUnlockUpdate, crate::InventoryPacketError> {
    if let RecipeUnlockOperation::Unknown(value) = operation {
        tracing::warn!(target: "cinnabar::inventory_requests", value, "received unfamiliar additive recipe unlock operation");
    }
    let names = match result {
        Ok(names) => Some(names),
        Err(Refusal::Policy) => {
            tracing::warn!(target: "cinnabar::inventory_requests", skipped = 1, "recipe unlock update exceeded retention policy");
            None
        }
        Err(Refusal::Wire) => return Err(crate::InventoryPacketError::MalformedWire),
    };
    Ok(RecipeUnlockUpdate { operation, names })
}
