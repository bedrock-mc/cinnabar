//! Compares item user data without treating compound entry order as identity.

use super::{MAX_ITEM_EXTRA_BYTES, read_i32_le, read_u8, read_u16_le, skip_le_payload};

/// Borrows a compound entry's name, tag type, and payload bytes.
type CompoundEntry<'a> = (&'a [u8], u8, &'a [u8]);

/// Compares parsed compound values and the unchanged item-data trailer.
/// Unsupported or malformed data retains strict byte comparison.
pub(super) fn same_extra(a: &[u8], b: &[u8]) -> bool {
    compare_extra(a, b).unwrap_or_else(|| a == b)
}

/// Separates framing failures from a well-formed value difference.
fn compare_extra(a: &[u8], b: &[u8]) -> Option<bool> {
    let (a, a_tail) = split_extra(a)?;
    let (b, b_tail) = split_extra(b)?;
    if a_tail != b_tail {
        return Some(false);
    }
    same_payload(10, a, b, 0)
}

/// Borrows the compound body and trailing lists, normalizing an absent compound.
fn split_extra(extra: &[u8]) -> Option<(&[u8], &[u8])> {
    if extra.len() > MAX_ITEM_EXTRA_BYTES {
        return None;
    }
    if extra.is_empty() {
        return Some((&[0], &[0; 8]));
    }
    let mut cursor = extra;
    match read_u16_le(&mut cursor)? {
        0 => Some((&[0], cursor)),
        u16::MAX => {
            if read_u8(&mut cursor)? != 1 || read_u8(&mut cursor)? != 10 {
                return None;
            }
            read_name(&mut cursor)?;
            let compound = take_payload(&mut cursor, 10, 0)?;
            Some((compound, cursor))
        }
        _ => None,
    }
}

/// Reads a bounded, borrowed fixed-little-endian name.
fn read_name<'a>(cursor: &mut &'a [u8]) -> Option<&'a [u8]> {
    let length = usize::from(read_u16_le(cursor)?);
    let name = cursor.get(..length)?;
    *cursor = cursor.get(length..)?;
    Some(name)
}

/// Borrows one payload using the existing bounded item NBT walker.
fn take_payload<'a>(cursor: &mut &'a [u8], tag: u8, depth: u32) -> Option<&'a [u8]> {
    let start = *cursor;
    skip_le_payload(cursor, tag, depth)?;
    start.get(..start.len() - cursor.len())
}

/// Reads unique compound entries in name order without copying payload bytes.
fn entries(mut cursor: &[u8], depth: u32) -> Option<Vec<CompoundEntry<'_>>> {
    let mut fields = Vec::new();
    loop {
        let tag = read_u8(&mut cursor)?;
        if tag == 0 {
            break;
        }
        let name = read_name(&mut cursor)?;
        let payload = take_payload(&mut cursor, tag, depth.checked_add(1)?)?;
        fields.push((name, tag, payload));
    }
    if !cursor.is_empty() {
        return None;
    }
    fields.sort_unstable_by_key(|field| field.0);
    if fields.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return None;
    }
    Some(fields)
}

/// Compares matching tag types, keeping list order and numeric float equality.
fn same_payload(tag: u8, a: &[u8], b: &[u8], depth: u32) -> Option<bool> {
    if depth > super::MAX_ITEM_NBT_WALK_DEPTH {
        return None;
    }
    match tag {
        5 => Some(f32::from_le_bytes(a.try_into().ok()?) == f32::from_le_bytes(b.try_into().ok()?)),
        6 => Some(f64::from_le_bytes(a.try_into().ok()?) == f64::from_le_bytes(b.try_into().ok()?)),
        9 => {
            let (mut a, mut b) = (a, b);
            let element = read_u8(&mut a)?;
            let other_element = read_u8(&mut b)?;
            let count = usize::try_from(read_i32_le(&mut a)?).ok()?;
            let other_count = usize::try_from(read_i32_le(&mut b)?).ok()?;
            if element != other_element || count != other_count {
                return Some(false);
            }
            for _ in 0..count {
                let next = depth.checked_add(1)?;
                let x = take_payload(&mut a, element, next)?;
                let y = take_payload(&mut b, element, next)?;
                if !same_payload(element, x, y, next)? {
                    return Some(false);
                }
            }
            Some(a.is_empty() && b.is_empty())
        }
        10 => {
            let a = entries(a, depth)?;
            let b = entries(b, depth)?;
            if a.len() != b.len() {
                return Some(false);
            }
            for ((a_name, a_tag, a_value), (b_name, b_tag, b_value)) in a.iter().zip(&b) {
                if a_name != b_name
                    || a_tag != b_tag
                    || !same_payload(*a_tag, a_value, b_value, depth.checked_add(1)?)?
                {
                    return Some(false);
                }
            }
            Some(true)
        }
        _ => Some(a == b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes one named fixed-little-endian tag for original comparison fixtures.
    fn field(tag: u8, name: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![tag];
        bytes.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(payload);
        bytes
    }

    /// Wraps compound fields with the item header and empty trailing block lists.
    fn extra(fields: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xff, 1, 10, 0, 0];
        bytes.extend_from_slice(fields);
        bytes.push(0);
        bytes.extend_from_slice(&[0; 8]);
        bytes
    }

    #[test]
    fn nested_compounds_ignore_order_but_retain_names_types_and_values() {
        let a = field(3, b"a", &2i32.to_le_bytes());
        let b = field(3, b"b", &4i32.to_le_bytes());
        let mut left = [a.clone(), b.clone()].concat();
        let mut right = [b.clone(), a.clone()].concat();
        left.push(0);
        right.push(0);
        assert!(same_extra(
            &extra(&field(10, b"nested", &left)),
            &extra(&field(10, b"nested", &right))
        ));
        for changed in [
            field(3, b"c", &2i32.to_le_bytes()),
            field(5, b"a", &2f32.to_le_bytes()),
            field(3, b"a", &3i32.to_le_bytes()),
        ] {
            assert!(!same_extra(
                &extra(&[a.clone(), b.clone()].concat()),
                &extra(&[changed, b.clone()].concat())
            ));
        }
    }

    #[test]
    fn lists_keep_order_and_element_type_even_when_empty() {
        let list = |element: u8, values: &[i32]| {
            let mut bytes = vec![element];
            bytes.extend_from_slice(&i32::try_from(values.len()).unwrap().to_le_bytes());
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            extra(&field(9, b"list", &bytes))
        };
        assert!(same_extra(&list(3, &[1, 2]), &list(3, &[1, 2])));
        assert!(!same_extra(&list(3, &[1, 2]), &list(3, &[2, 1])));
        assert!(!same_extra(&list(0, &[]), &list(3, &[])));
    }

    #[test]
    fn float_equality_retains_numeric_zero_and_nan_behavior() {
        for (tag, positive, negative, nan) in [
            (
                5,
                0f32.to_le_bytes().to_vec(),
                (-0f32).to_le_bytes().to_vec(),
                f32::NAN.to_le_bytes().to_vec(),
            ),
            (
                6,
                0f64.to_le_bytes().to_vec(),
                (-0f64).to_le_bytes().to_vec(),
                f64::NAN.to_le_bytes().to_vec(),
            ),
        ] {
            assert!(same_extra(
                &extra(&field(tag, b"f", &positive)),
                &extra(&field(tag, b"f", &negative))
            ));
            let nan = extra(&field(tag, b"f", &nan));
            assert!(!same_extra(&nan, &nan));
        }
    }

    #[test]
    fn root_names_are_framing_but_trailers_remain_identity() {
        let original = extra(&field(3, b"v", &7i32.to_le_bytes()));
        let mut renamed = original.clone();
        renamed.splice(4..6, [1, 0, b'x']);
        assert!(same_extra(&original, &renamed));
        let last = renamed.len() - 1;
        renamed[last] = 1;
        assert!(!same_extra(&original, &renamed));
        assert!(same_extra(&[], &extra(&[])));
        assert!(same_extra(&[0; 10], &extra(&[])));
    }

    #[test]
    fn unsupported_and_malformed_data_use_strict_bytes() {
        for bytes in [
            vec![0xff],
            vec![0xff, 0xff, 2],
            extra(&[3, 1, 0, b'x']),
            extra(&[3, 1, 0, b'x', 1, 0, 0, 0, 3, 1, 0, b'x', 2, 0, 0, 0]),
        ] {
            assert!(same_extra(&bytes, &bytes));
            let mut different = bytes.clone();
            different.push(1);
            assert!(!same_extra(&bytes, &different));
        }
        let mut nested = vec![0];
        for _ in 0..=super::super::MAX_ITEM_NBT_WALK_DEPTH {
            nested = [field(10, b"n", &nested), vec![0]].concat();
        }
        let too_deep = extra(&nested);
        assert_eq!(compare_extra(&too_deep, &too_deep), None);
        assert!(same_extra(&too_deep, &too_deep));
    }
}
