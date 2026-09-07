use bytes::Bytes;
use protocol::{
    ItemActorEvent, ItemPacketError, WorldEvent, WorldPacketError, WorldWireError,
    into_world_event, vanilla_item_registry,
};
use sha2::{Digest, Sha256};
use valentine::bedrock::{
    codec::Nbt,
    version::v1_26_44::{EnumsItemVersion as ItemVersion, ItemData, ItemRegistryPacket},
};

const COMPOUND: u8 = 10;
const INT: u8 = 3;

fn write_var_u32(bytes: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut next = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            next |= 0x80;
        }
        bytes.push(next);
        if value == 0 {
            break;
        }
    }
}

fn write_name(bytes: &mut Vec<u8>, name: &str) {
    write_var_u32(bytes, name.len() as u32);
    bytes.extend_from_slice(name.as_bytes());
}

fn int_tag(name: &str, value: i32) -> Vec<u8> {
    let mut bytes = vec![INT];
    write_name(&mut bytes, name);
    write_var_u32(&mut bytes, ((value << 1) ^ (value >> 31)) as u32);
    bytes
}

fn byte_tag(name: &str, value: u8) -> Vec<u8> {
    let mut bytes = vec![1];
    write_name(&mut bytes, name);
    bytes.push(value);
    bytes
}

fn compound_tag(name: &str, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![COMPOUND];
    write_name(&mut bytes, name);
    for entry in entries {
        bytes.extend_from_slice(entry);
    }
    bytes.push(0);
    bytes
}

fn root_compound(entries: &[Vec<u8>]) -> Nbt {
    let mut bytes = vec![COMPOUND, 0];
    for entry in entries {
        bytes.extend_from_slice(entry);
    }
    bytes.push(0);
    Nbt(Bytes::from(bytes))
}

fn capacity_component(value: i32) -> Nbt {
    root_compound(&[compound_tag(
        "components",
        &[compound_tag(
            "item_properties",
            &[int_tag("max_stack_size", value)],
        )],
    )])
}

fn registry_entry(component_data: Nbt, component_based: bool, version: ItemVersion) -> ItemData {
    ItemData {
        item_name: "minecraft:test_item".into(),
        item_id: 5,
        is_component_based: component_based,
        item_version: version,
        item_component_data: component_data,
    }
}

fn normalize(entry: ItemData) -> Result<protocol::ItemRegistryEntry, WorldPacketError> {
    let packet = ItemRegistryPacket {
        item_data: vec![entry],
    };
    let WorldEvent::ItemActor(ItemActorEvent::Registry(registry)) =
        into_world_event(packet.into(), 0)?.expect("item registry must produce an event")
    else {
        panic!("expected item registry")
    };
    Ok(registry.entries[0].clone())
}

#[test]
fn negotiated_capacity_retains_exact_positive_int_values_and_original_digest() {
    for capacity in [1u8, 16, 64] {
        let component_data = capacity_component(i32::from(capacity));
        let original = component_data.0.clone();
        let entry = normalize(registry_entry(
            component_data,
            false,
            ItemVersion::DataDriven,
        ))
        .unwrap();

        assert_eq!(entry.negotiated_max_stack_size, Some(capacity));
        assert!(!entry.canonical_empty_component_data);
        let expected_digest: [u8; 32] = Sha256::digest(&original).into();
        assert_eq!(entry.component_digest, expected_digest);
    }
}

#[test]
fn canonical_empty_component_data_is_exact_not_semantic() {
    let canonical = normalize(registry_entry(Nbt::default(), false, ItemVersion::Legacy)).unwrap();
    assert!(canonical.canonical_empty_component_data);
    assert_eq!(canonical.negotiated_max_stack_size, None);

    let named_empty_root = normalize(registry_entry(
        Nbt(Bytes::from_static(&[COMPOUND, 1, b'x', 0])),
        false,
        ItemVersion::Legacy,
    ))
    .unwrap();
    assert!(!named_empty_root.canonical_empty_component_data);
    assert_eq!(named_empty_root.negotiated_max_stack_size, None);

    let builtins = vanilla_item_registry();
    assert!(!builtins.is_empty());
    assert!(
        builtins
            .iter()
            .all(|entry| entry.canonical_empty_component_data
                && entry.negotiated_max_stack_size.is_none())
    );
}

#[test]
fn only_the_exact_component_property_path_supplies_capacity() {
    let shadow = compound_tag(
        "unrelated",
        &[compound_tag(
            "components",
            &[compound_tag(
                "item_properties",
                &[int_tag("max_stack_size", 16)],
            )],
        )],
    );
    let valid = compound_tag(
        "components",
        &[compound_tag(
            "item_properties",
            &[int_tag("max_stack_size", 64), int_tag("unrelated_value", 3)],
        )],
    );
    let entry = normalize(registry_entry(
        root_compound(&[shadow, valid]),
        true,
        ItemVersion::DataDriven,
    ))
    .unwrap();
    assert_eq!(entry.negotiated_max_stack_size, Some(64));
}

#[test]
fn ambiguous_or_invalid_capacity_evidence_is_ignored_without_rejecting_registry() {
    let duplicate_ancestor = root_compound(&[
        compound_tag(
            "components",
            &[compound_tag(
                "item_properties",
                &[int_tag("max_stack_size", 64)],
            )],
        ),
        compound_tag("components", &[]),
    ]);
    let wrong_tag = root_compound(&[compound_tag(
        "components",
        &[compound_tag(
            "item_properties",
            &[byte_tag("max_stack_size", 64)],
        )],
    )]);
    let wrong_components_tag = root_compound(&[int_tag("components", 64)]);
    let wrong_properties_tag = root_compound(&[compound_tag(
        "components",
        &[int_tag("item_properties", 64)],
    )]);
    let duplicate_value = root_compound(&[compound_tag(
        "components",
        &[compound_tag(
            "item_properties",
            &[int_tag("max_stack_size", 64), int_tag("max_stack_size", 16)],
        )],
    )]);

    for component_data in [
        duplicate_ancestor,
        duplicate_value,
        wrong_tag,
        wrong_components_tag,
        wrong_properties_tag,
        capacity_component(0),
        capacity_component(-1),
        capacity_component(256),
    ] {
        let entry = normalize(registry_entry(
            component_data,
            true,
            ItemVersion::DataDriven,
        ))
        .unwrap();
        assert_eq!(entry.negotiated_max_stack_size, None);
    }
}

#[test]
fn unknown_version_and_excessive_walk_depth_do_not_supply_capacity() {
    let unknown = normalize(registry_entry(
        capacity_component(64),
        true,
        ItemVersion::Unknown(99),
    ))
    .unwrap();
    assert_eq!(unknown.negotiated_max_stack_size, None);

    let mut nested = int_tag("leaf", 1);
    for _ in 0..20 {
        nested = compound_tag("nested", &[nested]);
    }
    let deep = normalize(registry_entry(
        root_compound(&[
            compound_tag(
                "components",
                &[compound_tag(
                    "item_properties",
                    &[int_tag("max_stack_size", 64)],
                )],
            ),
            nested,
        ]),
        true,
        ItemVersion::DataDriven,
    ))
    .unwrap();
    assert_eq!(deep.negotiated_max_stack_size, None);
}

#[test]
fn malformed_registry_nbt_remains_a_wire_error() {
    let truncated = Nbt(Bytes::from_static(&[
        COMPOUND, 0, COMPOUND, 10, b'c', b'o', b'm', b'p', b'o', b'n', b'e', b'n', b't', b's',
    ]));
    let error = normalize(registry_entry(truncated, true, ItemVersion::DataDriven)).unwrap_err();
    assert!(matches!(
        error,
        WorldPacketError::Wire(WorldWireError::Item(ItemPacketError::InvalidItemNbt))
    ));
}
