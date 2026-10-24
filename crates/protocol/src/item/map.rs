//! Map identity retained in an item's user data.

use super::{decode_extra_nbt, read_u16_le, root_tag};

/// Reads a signed map identity from a root long or a decimal string.
#[must_use]
pub fn item_map_id(extra_data: &[u8]) -> Option<i64> {
    let nbt = decode_extra_nbt(extra_data)?;
    if let Some(payload) = root_tag(&mut &*nbt, 4, b"map_uuid") {
        return Some(i64::from_le_bytes(payload.get(..8)?.try_into().ok()?));
    }
    let mut payload = root_tag(&mut &*nbt, 8, b"map_uuid")?;
    let length = usize::from(read_u16_le(&mut payload)?);
    decimal_prefix(payload.get(..length)?)
}

/// Parses a signed decimal prefix after ASCII whitespace, rejecting missing digits and overflow.
fn decimal_prefix(mut bytes: &[u8]) -> Option<i64> {
    while bytes
        .first()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
    {
        bytes = &bytes[1..];
    }
    let negative = bytes.first() == Some(&b'-');
    if negative || bytes.first() == Some(&b'+') {
        bytes = &bytes[1..];
    }
    let mut digits = bytes
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .peekable();
    digits.peek()?;
    let mut value = 0i64;
    for digit in digits {
        let digit = i64::from(digit - b'0');
        value = if negative {
            value.checked_mul(10)?.checked_sub(digit)?
        } else {
            value.checked_mul(10)?.checked_add(digit)?
        };
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wraps one typed map identity in an original fixed little-endian item compound.
    fn extra(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut bytes = vec![255, 255, 1, 10, 0, 0, tag, 8, 0];
        bytes.extend_from_slice(b"map_uuid");
        bytes.extend_from_slice(value);
        bytes.push(0);
        bytes
    }

    /// Encodes a string payload without requiring a game pack or server fixture.
    fn string(value: &[u8]) -> Vec<u8> {
        let mut payload = (value.len() as u16).to_le_bytes().to_vec();
        payload.extend_from_slice(value);
        extra(8, &payload)
    }

    #[test]
    fn signed_map_identities_keep_the_entire_long_range() {
        for id in [i64::MIN, -266, -1, 0, 266, i64::MAX] {
            assert_eq!(item_map_id(&extra(4, &id.to_le_bytes())), Some(id));
        }
    }

    #[test]
    fn decimal_map_identities_keep_signed_prefixes_without_overflow() {
        for (source, expected) in [
            ("266", Some(266)),
            (" \t\u{b}+266suffix", Some(266)),
            ("-266\0ignored", Some(-266)),
            ("-9223372036854775808", Some(i64::MIN)),
            ("9223372036854775807", Some(i64::MAX)),
            ("9223372036854775808", None),
            ("-9223372036854775809", None),
            ("", None),
            (" \t-", None),
            ("no digits", None),
        ] {
            assert_eq!(
                item_map_id(&string(source.as_bytes())),
                expected,
                "{source:?}"
            );
        }
    }

    #[test]
    fn absent_malformed_and_other_typed_ids_do_not_create_a_map() {
        assert_eq!(item_map_id(&[]), None);
        assert_eq!(item_map_id(&extra(3, &266i32.to_le_bytes())), None);
        let complete = extra(4, &266i64.to_le_bytes());
        for end in 0..complete.len() - 1 {
            assert_eq!(item_map_id(&complete[..end]), None, "truncated at {end}");
        }
    }
}
