//! Bounded comparison of retained item user data.

use std::collections::{BTreeMap, BTreeSet};

use bedrock_protocol_1_26_51::wire::MAX_NBT_DEPTH;
use sha2::{Digest, Sha256};

use super::{read_i32_le, read_u8, read_u16_le, skip_le_payload, skip_le_string};

/// Compares typed NBT independently of compound order and restriction-list order.
/// Restriction names are compared directly; resolving block-name aliases needs a block registry.
/// Malformed, excessive or duplicate-key data returns `None`.
#[must_use]
pub fn item_user_data_equal(left: &[u8], right: &[u8]) -> Option<bool> {
    let (left, right) = (read_extra(left)?, read_extra(right)?);
    Some(left.0.reflexive && right.0.reflexive && left == right)
}

/// A compact typed fingerprint; NaN-containing data never compares equal.
#[derive(Debug, Eq, PartialEq)]
struct Identity {
    digest: [u8; 32],
    reflexive: bool,
}

type Restrictions<'a> = BTreeSet<&'a [u8]>;
type Extra<'a> = (Identity, Restrictions<'a>, Restrictions<'a>, i64);

/// Reads a complete item-data envelope and normalizes its restriction lists.
fn read_extra(extra: &[u8]) -> Option<Extra<'_>> {
    if extra.len() > crate::MAX_ITEM_NBT_BYTES {
        return None;
    }
    let extra: &[u8] = if extra.is_empty() {
        &super::nbt_edit::EMPTY_USER_DATA
    } else {
        extra
    };
    let marker = i16::from_le_bytes(extra.get(..2)?.try_into().ok()?);
    let mut cursor = extra.get(2..)?;
    let value = match marker {
        0 => read_identity(&mut &[0][..], 10, 0)?,
        -1 => {
            if read_u8(&mut cursor)? != 1 || read_u8(&mut cursor)? != 10 {
                return None;
            }
            skip_le_string(&mut cursor)?;
            read_identity(&mut cursor, 10, 0)?
        }
        _ => return None,
    };
    let place = read_restrictions(&mut cursor)?;
    let destroy = read_restrictions(&mut cursor)?;
    let blocking = match cursor {
        [] => 0,
        bytes if bytes.len() == size_of::<i64>() => i64::from_le_bytes(bytes.try_into().ok()?),
        _ => return None,
    };
    Some((value, place, destroy, blocking))
}

/// Reads a bounded set of length-prefixed restriction names without copying them.
fn read_restrictions<'a>(cursor: &mut &'a [u8]) -> Option<Restrictions<'a>> {
    let count = usize::try_from(read_i32_le(cursor)?).ok()?;
    if count > cursor.len() / size_of::<u16>() {
        return None;
    }
    let mut names = BTreeSet::new();
    for _ in 0..count {
        let length = usize::from(read_u16_le(cursor)?);
        names.insert(cursor.get(..length)?);
        *cursor = cursor.get(length..)?;
    }
    Some(names)
}

/// Hashes typed contents once, retaining only compound keys and child fingerprints.
fn read_identity(cursor: &mut &[u8], kind: u8, depth: u32) -> Option<Identity> {
    if depth > MAX_NBT_DEPTH {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update([kind]);
    let mut reflexive = true;
    match kind {
        5 | 6 => hash_float(&mut hash, cursor, kind, &mut reflexive)?,
        9 => {
            let element = read_u8(cursor)?;
            let count = usize::try_from(read_i32_le(cursor)?).ok()?;
            if element > 12
                || (element == 0 && count != 0)
                || count > cursor.len()
                || (count != 0 && depth == MAX_NBT_DEPTH)
            {
                return None;
            }
            hash.update([element]);
            hash.update((count as u32).to_le_bytes());
            match element {
                1..=4 => {
                    let width = match element {
                        1 => 1,
                        2 => 2,
                        3 => 4,
                        _ => 8,
                    };
                    let length = count.checked_mul(width)?;
                    hash.update(cursor.get(..length)?);
                    *cursor = cursor.get(length..)?;
                }
                5 | 6 => {
                    for _ in 0..count {
                        hash_float(&mut hash, cursor, element, &mut reflexive)?;
                    }
                }
                _ => {
                    for _ in 0..count {
                        let child = read_identity(cursor, element, depth.checked_add(1)?)?;
                        hash.update(child.digest);
                        reflexive &= child.reflexive;
                    }
                }
            }
        }
        10 => {
            let mut fields = BTreeMap::new();
            loop {
                let field = read_u8(cursor)?;
                if field == 0 {
                    break;
                }
                let length = usize::from(read_u16_le(cursor)?);
                let name = cursor.get(..length)?;
                *cursor = cursor.get(length..)?;
                let child = read_identity(cursor, field, depth.checked_add(1)?)?;
                if fields.insert(name, child).is_some() {
                    return None;
                }
            }
            hash.update((fields.len() as u32).to_le_bytes());
            for (name, child) in fields {
                hash.update((name.len() as u32).to_le_bytes());
                hash.update(name);
                hash.update(child.digest);
                reflexive &= child.reflexive;
            }
        }
        _ => {
            let start = *cursor;
            skip_le_payload(cursor, kind, 0)?;
            hash.update(start.get(..start.len() - cursor.len())?);
        }
    }
    Some(Identity {
        digest: hash.finalize().into(),
        reflexive,
    })
}

/// Hashes numeric float equality, normalizing signed zero and recording NaN.
fn hash_float(hash: &mut Sha256, cursor: &mut &[u8], kind: u8, reflexive: &mut bool) -> Option<()> {
    if kind == 5 {
        let value = f32::from_le_bytes(cursor.get(..4)?.try_into().ok()?);
        *cursor = cursor.get(4..)?;
        *reflexive &= !value.is_nan();
        hash.update(if value == 0.0 { 0 } else { value.to_bits() }.to_le_bytes());
    } else {
        let value = f64::from_le_bytes(cursor.get(..8)?.try_into().ok()?);
        *cursor = cursor.get(8..)?;
        *reflexive &= !value.is_nan();
        hash.update(if value == 0.0 { 0 } else { value.to_bits() }.to_le_bytes());
    }
    Some(())
}

#[cfg(test)]
mod tests;
