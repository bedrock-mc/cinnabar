use bytes::{Bytes, BytesMut};
use jolyne::raw::decode_packet_raw;
use valentine::bedrock::{
    codec::Nbt, context::BedrockSession, version::v1_26_51::AvailableActorIdentifiersPacket,
};

/// Network NBT for an `idlist` holding one custom actor built on a native one.
fn custom_actor_list() -> Vec<u8> {
    let mut bytes = vec![10, 0, 9, 6];
    bytes.extend(b"idlist");
    bytes.extend([10, 2]);
    for (key, value) in [("id", "custom:archer"), ("bid", "minecraft:skeleton")] {
        bytes.extend([8, key.len() as u8]);
        bytes.extend(key.as_bytes());
        bytes.push(value.len() as u8);
        bytes.extend(value.as_bytes());
    }
    bytes.extend([0, 0]);
    bytes
}

#[test]
fn actor_identifier_registry_reaches_raw_world_ingress() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: crate::Packet = AvailableActorIdentifiersPacket {
        identifier_list: Nbt(Bytes::from(custom_actor_list())),
    }
    .into();
    let mut frame = BytesMut::new();
    packet
        .data
        .encode_inner_bytes_mut(&mut frame, 0, 0)
        .unwrap();
    let raw = decode_packet_raw(&mut frame.freeze()).unwrap();
    let event = super::decode_world_raw_with(raw, 0, |raw| raw.decode(&session)).unwrap();
    let Some(crate::WorldEvent::Actor(crate::ActorEvent::Identifiers(registry))) = event else {
        panic!("expected the identifier registry, got {event:?}");
    };
    assert_eq!(registry.skipped, 0);
    assert_eq!(&*registry.entries[0].identifier, "custom:archer");
    assert_eq!(&*registry.entries[0].base_identifier, "minecraft:skeleton");
}
