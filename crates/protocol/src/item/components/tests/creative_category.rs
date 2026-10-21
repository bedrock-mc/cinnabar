use super::{byte, compound, float, int, name, string};
use crate::item::components::parse_components;
use crate::item::registry_properties::{CREATIVE_CATEGORY_PROPERTY, negotiated_max_stack_size};

/// Encodes one item-properties compound through the existing original NBT fixtures.
fn properties(entries: &[Vec<u8>]) -> Vec<u8> {
    compound(
        "",
        &[compound(
            "components",
            &[compound("item_properties", entries)],
        )],
    )
}

#[test]
fn creative_category_retains_the_int_property_low_byte() {
    for value in [0, 1, 2, 3, 4, 5, 257, -1] {
        let data = properties(&[int(CREATIVE_CATEGORY_PROPERTY, value)]);
        assert_eq!(
            parse_components(&data).unwrap().creative_category,
            Some(value as u8)
        );
    }
}

#[test]
fn creative_category_wrong_integer_widths_use_the_property_default() {
    let mut short = name(2, CREATIVE_CATEGORY_PROPERTY);
    short.extend(3_i16.to_le_bytes());
    let mut long = int(CREATIVE_CATEGORY_PROPERTY, 3);
    long[0] = 4;
    for entry in [
        byte(CREATIVE_CATEGORY_PROPERTY, 3),
        short,
        long,
        float(CREATIVE_CATEGORY_PROPERTY, 3.0),
        string(CREATIVE_CATEGORY_PROPERTY, "3"),
    ] {
        assert_eq!(
            parse_components(&properties(&[entry]))
                .unwrap()
                .creative_category,
            Some(0)
        );
    }
}

#[test]
fn creative_category_keeps_missing_ambiguous_and_unrelated_properties_separate() {
    assert_eq!(
        parse_components(&compound("", &[compound("components", &[])]))
            .unwrap()
            .creative_category,
        None
    );
    assert_eq!(
        parse_components(&properties(&[]))
            .unwrap()
            .creative_category,
        Some(0)
    );
    let duplicate = properties(&[
        int(CREATIVE_CATEGORY_PROPERTY, 1),
        int(CREATIVE_CATEGORY_PROPERTY, 2),
    ]);
    assert_eq!(
        parse_components(&duplicate).unwrap().creative_category,
        None
    );
    let independent = properties(&[
        int(CREATIVE_CATEGORY_PROPERTY, 2),
        string("max_stack_size", "16"),
    ]);
    assert_eq!(
        parse_components(&independent).unwrap().creative_category,
        Some(2)
    );
    assert_eq!(negotiated_max_stack_size(&independent), None);
    let data = properties(&[
        string(CREATIVE_CATEGORY_PROPERTY, "3"),
        int("max_stack_size", 16),
    ]);
    assert_eq!(parse_components(&data).unwrap().creative_category, Some(0));
    assert_eq!(negotiated_max_stack_size(&data), Some(16));
}
