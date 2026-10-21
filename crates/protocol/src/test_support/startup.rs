//! Original startup fixtures normalized through the wire codec and login projection.

use bedrock_protocol_1_26_51::{
    packets::{ItemRegistry, StartGame},
    types::ServerBlockProperty,
    wire::{self, Decode, Encode},
};

/// Projects ordered block definitions through a StartGame wire round trip.
/// Panics when a fixture contains invalid network NBT or exceeds codec limits.
pub fn with_block_definitions<'a>(
    definitions: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    hashed: bool,
) -> crate::GameData {
    let packet = StartGame {
        block_network_ids_are_hashes: hashed,
        block_properties: definitions
            .into_iter()
            .map(|(name, nbt)| ServerBlockProperty {
                block_name: name.into(),
                block_definition: wire::NetworkNbt(nbt.to_vec().into()),
            })
            .collect(),
        ..Default::default()
    };
    let bytes = packet.encode_to_vec().expect("fixture StartGame encodes");
    let decoded = StartGame::decode_exact(&bytes).expect("fixture StartGame decodes");
    crate::GameData::from_wire(&decoded, &ItemRegistry::default())
}
