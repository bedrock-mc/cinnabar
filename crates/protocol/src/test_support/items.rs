//! Startup item fixtures pass through the same wire and login projection as a real join.

use bedrock_protocol_1_26_51::{
    enums::ItemVersion,
    packets::{ItemRegistry, StartGame},
    types::ItemData,
    wire::{self, Decode, Encode},
};

/// Projects a DataDriven entry with an explicit stack size and unrelated nested compounds
/// through wire admission. Panics if the requested fixture exceeds the wire limits.
pub fn component_item_registry(
    identifier: &str,
    network_id: i16,
    max_stack_size: u8,
    unrelated_depth: usize,
) -> crate::ItemRegistryEvent {
    let mut bytes = Vec::new();
    let mut writer = wire::Writer::new(&mut bytes);
    nbt_name(&mut writer, 10, "");
    nbt_name(&mut writer, 10, "components");
    nbt_name(&mut writer, 10, "item_properties");
    nbt_name(&mut writer, 3, "max_stack_size");
    writer.write_zigzag_i32(i32::from(max_stack_size)).unwrap();
    writer.write_u8(0).unwrap();
    for _ in 0..unrelated_depth {
        nbt_name(&mut writer, 10, "unrelated");
    }
    for _ in 0..unrelated_depth + 2 {
        writer.write_u8(0).unwrap();
    }
    component_item_registry_payload(identifier, network_id, bytes)
}

/// Projects a DataDriven component payload through wire admission and startup normalization.
/// Panics if the payload is not wire-admissible NBT.
pub fn component_item_registry_payload(
    identifier: &str,
    network_id: i16,
    bytes: Vec<u8>,
) -> crate::ItemRegistryEvent {
    let registry = ItemRegistry {
        item_data: vec![ItemData {
            item_name: identifier.into(),
            item_id: wire::I16LE(network_id),
            is_component_based: true,
            item_version: ItemVersion::Datadriven,
            item_component_data: wire::NetworkNbt(bytes.into()),
        }],
    };
    let encoded = registry.encode_to_vec().expect("fixture registry encodes");
    let decoded = ItemRegistry::decode_exact(&encoded).expect("fixture registry decodes");
    crate::item::normalize_item_registry(decoded).expect("fixture registry is admitted")
}

/// Writes a named NBT tag header without its payload.
fn nbt_name(writer: &mut wire::Writer<'_>, tag: u8, name: &str) {
    writer.write_u8(tag).unwrap();
    writer.write_len_var_u32(name.len()).unwrap();
    writer.write_all(name.as_bytes()).unwrap();
}

/// Projects ordered `(identifier, network id, icon)` entries into startup facts after a
/// wire round trip. Every entry is component-based; `None` carries canonical empty NBT.
pub fn startup_with_item_icons(entries: &[(&str, i16, Option<&str>)]) -> crate::login::GameData {
    let registry = ItemRegistry {
        item_data: entries
            .iter()
            .map(|&(identifier, network_id, icon)| ItemData {
                item_name: identifier.into(),
                item_id: wire::I16LE(network_id),
                is_component_based: true,
                item_component_data: wire::NetworkNbt(icon_component_nbt(icon).into()),
                ..Default::default()
            })
            .collect(),
    };
    let bytes = registry.encode_to_vec().expect("fixture registry encodes");
    let registry = ItemRegistry::decode_exact(&bytes).expect("fixture registry decodes");
    crate::login::GameData::from_wire(&StartGame::default(), &registry)
}

/// Encodes one network-NBT root with an optional item icon component.
fn icon_component_nbt(icon: Option<&str>) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = wire::Writer::new(&mut bytes);
    writer.write_all(&[10, 0]).unwrap();
    if let Some(icon) = icon {
        writer.write_u8(10).unwrap();
        writer.write_len_var_u32("components".len()).unwrap();
        writer.write_all(b"components").unwrap();
        writer.write_u8(8).unwrap();
        writer.write_len_var_u32("minecraft:icon".len()).unwrap();
        writer.write_all(b"minecraft:icon").unwrap();
        writer.write_len_var_u32(icon.len()).unwrap();
        writer.write_all(icon.as_bytes()).unwrap();
        writer.write_u8(0).unwrap();
    }
    writer.write_u8(0).unwrap();
    bytes
}
