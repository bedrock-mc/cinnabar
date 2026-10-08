//! Map image identities retain the full signed NBT long, independently of item metadata.

use super::{decode_extra_nbt, root_tag};

/// Reads a filled map's root `map_uuid` long; malformed or missing tags read as `None`.
#[must_use]
pub fn item_map_id(extra_data: &[u8]) -> Option<i64> {
    let nbt = decode_extra_nbt(extra_data)?;
    let mut cursor = &nbt[..];
    let payload = root_tag(&mut cursor, 4, b"map_uuid")?;
    Some(i64::from_le_bytes(payload.get(..8)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_identity_preserves_long_precision_and_rejects_wrong_or_truncated_tags() {
        let mut extra = vec![255, 255, 1, 10, 0, 0, 4, 8, 0];
        extra.extend_from_slice(b"map_uuid");
        let header = extra.len();
        for id in [0, -1, i64::MIN, i64::MAX, (1_i64 << 53) + 1] {
            extra.truncate(header);
            extra.extend_from_slice(&id.to_le_bytes());
            extra.push(0);
            assert_eq!(item_map_id(&extra), Some(id));
            assert_eq!(item_map_id(&extra[..header + 7]), None);
        }
        extra[6] = 3;
        assert_eq!(item_map_id(&extra), None);
        assert_eq!(item_map_id(&[]), None);
    }
}
