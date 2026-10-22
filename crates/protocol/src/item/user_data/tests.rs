use super::*;

/// Encodes original named scalar fields inside a persistent root compound.
fn extra(fields: &[(u8, &str, &[u8])]) -> Vec<u8> {
    let mut bytes = vec![255, 255, 1, 10, 0, 0];
    for (kind, name, value) in fields {
        bytes.push(*kind);
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(value);
    }
    bytes.push(0);
    bytes.extend_from_slice(&[0; 8]);
    bytes
}

#[test]
fn item_user_data_compares_typed_contents_and_compound_order() {
    let a = extra(&[(3, "a", &1i32.to_le_bytes()), (1, "b", &[2])]);
    let b = extra(&[(1, "b", &[2]), (3, "a", &1i32.to_le_bytes())]);
    assert_eq!(item_user_data_equal(&a, &b), Some(true));
    assert_eq!(
        item_user_data_equal(&a, &extra(&[(1, "a", &[1]), (1, "b", &[2])])),
        Some(false)
    );
    assert_eq!(
        item_user_data_equal(&a, &extra(&[(3, "a", &2i32.to_le_bytes()), (1, "b", &[2])])),
        Some(false)
    );
    assert_eq!(item_user_data_equal(&[], &[0; 10]), Some(true));
    assert_eq!(item_user_data_equal(&extra(&[]), &[]), Some(true));
}

#[test]
fn item_user_data_keeps_list_order_and_float_value_equality() {
    let list = [1, 2, 0, 0, 0, 1, 2];
    let reverse = [1, 2, 0, 0, 0, 2, 1];
    assert_eq!(
        item_user_data_equal(&extra(&[(9, "l", &list)]), &extra(&[(9, "l", &reverse)])),
        Some(false)
    );
    for kind in [5, 6] {
        let (positive, negative, nan) = if kind == 5 {
            (
                0f32.to_le_bytes().to_vec(),
                (-0f32).to_le_bytes().to_vec(),
                f32::NAN.to_le_bytes().to_vec(),
            )
        } else {
            (
                0f64.to_le_bytes().to_vec(),
                (-0f64).to_le_bytes().to_vec(),
                f64::NAN.to_le_bytes().to_vec(),
            )
        };
        assert_eq!(
            item_user_data_equal(
                &extra(&[(kind, "v", &positive)]),
                &extra(&[(kind, "v", &negative)])
            ),
            Some(true)
        );
        let bytes = extra(&[(kind, "v", &nan)]);
        assert_eq!(item_user_data_equal(&bytes, &bytes), Some(false));
    }
}

#[test]
fn item_user_data_refuses_invalid_or_excessive_shapes_and_compares_trailers() {
    for bad in [
        vec![1],
        vec![0; crate::MAX_ITEM_NBT_BYTES + 1],
        extra(&[(1, "a", &[1]), (1, "a", &[1])]),
        extra(&[(9, "l", &[1, 255, 255, 255, 127])]),
    ] {
        assert_eq!(item_user_data_equal(&bad, &bad), None);
    }
    let good = extra(&[(1, "a", &[1])]);
    let mut changed = good.clone();
    changed.extend_from_slice(&1i64.to_le_bytes());
    assert_eq!(item_user_data_equal(&good, &changed), Some(false));
}

/// Encodes original placement restrictions with an empty destruction list.
fn restrictions(names: &[&str]) -> Vec<u8> {
    let mut bytes = vec![0, 0];
    bytes.extend_from_slice(&(names.len() as i32).to_le_bytes());
    for name in names {
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
    }
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes
}

#[test]
fn item_user_data_restriction_order_and_duplicates_do_not_change_identity() {
    let first = restrictions(&["minecraft:stone", "minecraft:dirt"]);
    let reordered = restrictions(&["minecraft:dirt", "minecraft:stone", "minecraft:stone"]);
    assert_eq!(item_user_data_equal(&first, &reordered), Some(true));
    assert_eq!(
        item_user_data_equal(&first, &restrictions(&["minecraft:stone"])),
        Some(false)
    );
    for malformed in [vec![0, 0], vec![0, 0, 255], vec![0, 0, 255, 255, 255, 127]] {
        assert_eq!(item_user_data_equal(&malformed, &malformed), None);
    }
}

#[test]
fn item_user_data_compares_wide_primitive_lists() {
    let mut list = vec![1];
    list.extend_from_slice(&60_000i32.to_le_bytes());
    list.resize(60_005, 7);
    let left = extra(&[(9, "values", &list)]);
    assert_eq!(item_user_data_equal(&left, &left), Some(true));
    *list.last_mut().unwrap() = 8;
    let right = extra(&[(9, "values", &list)]);
    assert_eq!(item_user_data_equal(&left, &right), Some(false));
}

#[test]
fn item_user_data_uses_the_wire_nesting_bound() {
    for depth in [MAX_NBT_DEPTH, MAX_NBT_DEPTH + 1] {
        let mut bytes = vec![255, 255, 1, 10, 0, 0];
        for _ in 0..depth {
            bytes.extend_from_slice(&[10, 0, 0]);
        }
        bytes.extend(std::iter::repeat_n(0, depth as usize + 1));
        bytes.extend_from_slice(&[0; 8]);
        assert_eq!(
            item_user_data_equal(&bytes, &bytes),
            (depth == MAX_NBT_DEPTH).then_some(true)
        );
    }
}

/// Appends one restriction list: an `int32` count of `int16`-prefixed names.
fn push_list(bytes: &mut Vec<u8>, names: &[&str]) {
    bytes.extend_from_slice(&(names.len() as i32).to_le_bytes());
    for name in names {
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
    }
}

#[test]
fn item_block_lists_read_after_the_compound_or_its_absence() {
    let mut with_compound = vec![255, 255, 1, 10, 0, 0, 3, 1, 0, b'a'];
    with_compound.extend_from_slice(&7i32.to_le_bytes());
    with_compound.push(0);
    push_list(&mut with_compound, &["minecraft:dirt"]);
    push_list(&mut with_compound, &["stone", "minecraft:glass"]);
    assert_eq!(
        item_block_list(&with_compound, ItemBlockList::CanPlaceOn),
        Some(vec!["minecraft:dirt"])
    );
    assert_eq!(
        item_block_list(&with_compound, ItemBlockList::CanDestroy),
        Some(vec!["minecraft:glass", "stone"])
    );

    let mut bare = 0i16.to_le_bytes().to_vec();
    push_list(&mut bare, &[]);
    push_list(&mut bare, &["minecraft:sand"]);
    assert_eq!(
        item_block_list(&bare, ItemBlockList::CanDestroy),
        Some(vec!["minecraft:sand"])
    );
    assert_eq!(
        item_block_list(&[], ItemBlockList::CanDestroy),
        Some(vec![])
    );
    assert_eq!(item_block_list(&[1, 0], ItemBlockList::CanPlaceOn), None);
    assert_eq!(
        item_block_list(&bare[..bare.len() - 1], ItemBlockList::CanDestroy),
        None
    );
}
