//! Generated negotiation fixtures for real external transport tests.

use bedrock_protocol_1_26_51::{
    enums::{PlayStatusType, TextPacketType},
    packets,
    types::{ActorRuntimeID, TextData},
    wire,
};
use bytes::Bytes;

use crate::{Packet, PacketKind};

/// A completely decoded client packet observed during server-side negotiation fixtures.
#[derive(Debug, PartialEq, Eq)]
pub enum ClientPacket {
    RequestNetworkSettings,
    Login,
    ClientCacheStatus,
    ResourcePackClientResponse,
    Other(PacketKind),
}

/// Decodes the complete generated body and rejects trailing bytes before classifying it.
pub fn client_packet(frame: &Bytes) -> ClientPacket {
    let mut reader = wire::Reader::from_shared(frame);
    let header = wire::PacketHeader::from_raw(reader.read_var_u32().unwrap());
    let id = packets::PacketId::from_raw(header.id).unwrap();
    let packet = packets::Packet::decode_body(id, &mut reader).unwrap();
    reader.expect_consumed().unwrap();
    match packet {
        packets::Packet::RequestNetworkSettings(_) => ClientPacket::RequestNetworkSettings,
        packets::Packet::Login(_) => ClientPacket::Login,
        packets::Packet::ClientCacheStatus(_) => ClientPacket::ClientCacheStatus,
        packets::Packet::ResourcePackClientResponse(_) => ClientPacket::ResourcePackClientResponse,
        _ => ClientPacket::Other(PacketKind(id)),
    }
}

/// Builds actual NetworkSettings with the requested compression threshold.
pub fn network_settings(threshold: u16) -> Packet {
    Packet::new(packets::NetworkSettings {
        compression_threshold: wire::U16LE(threshold),
        ..Default::default()
    })
}

/// Builds the original Login-success status followed by an empty pack offer.
pub fn login_success_and_pack_info() -> [Packet; 2] {
    [
        Packet::new(packets::PlayStatus {
            status: PlayStatusType::Loginsuccess,
        }),
        Packet::new(packets::ResourcePacksInfo::default()),
    ]
}

/// Builds the empty stack followed by StartGame for the selected runtime ID.
pub fn empty_stack_and_start_game(runtime_id: u64) -> [Packet; 2] {
    [
        Packet::new(packets::ResourcePackStack::default()),
        Packet::new(packets::StartGame {
            runtime_id: ActorRuntimeID(runtime_id),
            ..Default::default()
        }),
    ]
}

/// Builds the default generated server Disconnect packet.
pub fn disconnect() -> Packet {
    Packet::new(packets::Disconnect::default())
}

/// Builds an actual Transfer control without resolving or connecting its destination.
pub fn transfer(address: &str, port: u16) -> Packet {
    Packet::new(packets::Transfer {
        server_address: address.into(),
        server_port: wire::U16LE(port),
        reload_world: true,
        ..Default::default()
    })
}

/// Encodes one generated server packet with both actual subclient header fields.
pub fn server_entry(packet: &Packet, sender: u8, target: u8) -> Bytes {
    let mut body = Vec::new();
    packet
        .data
        .encode_frame(wire::Peer::Server, sender, target, &mut body)
        .unwrap();
    let mut reader = wire::Reader::new(&body);
    reader.read_var_u32().unwrap();
    Bytes::copy_from_slice(&body[reader.position()..])
}

/// Encodes a well-formed unused ID with an opaque body, without a replacement packet decoder.
pub fn unknown_server_entry() -> Bytes {
    let id = (0..=wire::PacketHeader::ID_MASK)
        .rev()
        .find(|id| packets::PacketId::from_raw(*id).is_none())
        .expect("the selected protocol has an unused packet ID");
    let mut entry = header_entry(id);
    entry.extend_from_slice(b"unused extension body");
    entry.into()
}

/// Deliberately omits a known generated packet's required body for rejection controls.
pub fn header_only_server_entry(packet: &Packet) -> Bytes {
    header_entry(packet.kind().id()).into()
}

/// Uses the generated header writer for each deliberately raw fixture entry.
fn header_entry(id: u32) -> Vec<u8> {
    let mut entry = Vec::new();
    wire::Writer::new(&mut entry)
        .write_var_u32(wire::PacketHeader::new(id).to_raw())
        .unwrap();
    entry
}

/// Builds a raw text packet with the fixture's original message bytes.
pub fn raw_text(message: String) -> Packet {
    Packet::new(packets::Text {
        body: TextData::MessageOnly {
            message_type: TextPacketType::Raw,
            message: message.into(),
        },
        ..Default::default()
    })
}

/// Encodes the original primary-subclient server frame, including its entry length.
pub fn server_frame(packet: &Packet) -> Vec<u8> {
    let mut encoded = Vec::new();
    append_server_frame(packet, &mut encoded);
    encoded
}

/// Frames bounded server packets and returns the original length-free batch entries.
pub fn server_entries(
    packets: &[Packet],
    max_packets: usize,
    max_decoded_bytes: usize,
) -> Vec<Bytes> {
    assert!(packets.len() <= max_packets);
    let mut body = Vec::new();
    for packet in packets {
        append_server_frame(packet, &mut body);
    }
    assert!(body.len() <= max_decoded_bytes);
    wire::Frames::new(Bytes::from(body), max_packets)
        .map(|entry| {
            let entry = entry.unwrap();
            let mut reader = wire::Reader::from_shared(entry.frame());
            reader.read_var_u32().unwrap();
            entry.frame().slice(reader.position()..)
        })
        .collect()
}

/// Uses the same generated encoder and primary subclients for every fixture frame.
fn append_server_frame(packet: &Packet, body: &mut Vec<u8>) {
    packet
        .data
        .encode_frame(wire::Peer::Server, 0, 0, body)
        .unwrap();
}
