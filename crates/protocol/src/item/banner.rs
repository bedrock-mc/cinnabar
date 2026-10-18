//! Bounded banner pattern metadata from an item's fixed little-endian NBT.

use super::{decode_extra_nbt, read_i32_le, read_u8, read_u16_le, root_tag, skip_le_payload};
use std::sync::Arc;

/// One banner overlay in its original NBT order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemBannerPattern {
    pub pattern: Arc<str>,
    pub color: u8,
}

/// Reads a root Int banner Type; absent and differently typed values remain unspecified.
pub fn item_banner_type(extra_data: &[u8]) -> Option<i32> {
    let mut nbt = decode_extra_nbt(extra_data)?;
    let mut value = root_tag(&mut nbt, 3, b"Type")?;
    read_i32_le(&mut value)
}

/// Reads up to sixteen valid banner overlays; malformed metadata returns no overlays.
pub fn item_banner_patterns(extra_data: &[u8]) -> Vec<ItemBannerPattern> {
    parse(extra_data).unwrap_or_default()
}

/// Validates the pattern list without allocating from untrusted counts or string lengths.
fn parse(extra_data: &[u8]) -> Option<Vec<ItemBannerPattern>> {
    let nbt = decode_extra_nbt(extra_data)?;
    let mut cursor = nbt;
    let mut list = root_tag(&mut cursor, 9, b"Patterns")?;
    if read_u8(&mut list)? != 10 {
        return None;
    }
    let count = usize::try_from(read_i32_le(&mut list)?).ok()?;
    if count > 16 {
        return None;
    }
    let mut patterns = Vec::with_capacity(count);
    for _ in 0..count {
        let (mut pattern, mut color) = (None, None);
        loop {
            let tag = read_u8(&mut list)?;
            if tag == 0 {
                break;
            }
            let len = usize::from(read_u16_le(&mut list)?);
            let name = list.get(..len)?;
            list = list.get(len..)?;
            match (tag, name) {
                (8, b"Pattern") => {
                    let len = usize::from(read_u16_le(&mut list)?);
                    if len > 32 {
                        return None;
                    }
                    pattern = Some(Arc::from(std::str::from_utf8(list.get(..len)?).ok()?));
                    list = list.get(len..)?;
                }
                (3, b"Color") => {
                    color = u8::try_from(read_i32_le(&mut list)?)
                        .ok()
                        .filter(|color| *color < 16)
                }
                _ => skip_le_payload(&mut list, tag, 1)?,
            }
        }
        if let (Some(pattern), Some(color)) = (pattern, color) {
            patterns.push(ItemBannerPattern { pattern, color });
        }
    }
    Some(patterns)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Encodes a small fixed little-endian banner item fixture.
    fn fixture(count: i32) -> Vec<u8> {
        let mut out = vec![255, 255, 1, 10, 0, 0, 9, 8, 0];
        out.extend(b"Patterns");
        out.push(10);
        out.extend(count.to_le_bytes());
        for (pattern, color) in [("bs", 1_i32), ("bo", 4)] {
            out.extend([8, 7, 0]);
            out.extend(b"Pattern");
            out.extend((pattern.len() as u16).to_le_bytes());
            out.extend(pattern.as_bytes());
            out.extend([3, 5, 0]);
            out.extend(b"Color");
            out.extend(color.to_le_bytes());
            out.push(0);
        }
        out.push(0);
        out
    }
    #[test]
    fn banner_patterns_preserve_nbt_order_and_reject_malformed_lists() {
        assert_eq!(
            item_banner_patterns(&fixture(2)),
            vec![
                ItemBannerPattern {
                    pattern: Arc::from("bs"),
                    color: 1
                },
                ItemBannerPattern {
                    pattern: Arc::from("bo"),
                    color: 4
                },
            ]
        );
        assert!(item_banner_patterns(&fixture(17)).is_empty());
        let mut truncated = fixture(2);
        truncated.truncate(truncated.len() - 5);
        assert!(item_banner_patterns(&truncated).is_empty());
        assert!(item_banner_patterns(&[]).is_empty());
    }
}
