//! Startup item fixtures pass through the same wire and login projection as a real join.

use bedrock_protocol_1_26_51::{
    packets::{ItemRegistry, StartGame},
    types::ItemData,
    wire::{self, Decode, Encode},
};

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
