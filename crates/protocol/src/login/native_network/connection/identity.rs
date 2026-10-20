//! Canonical Login identity applied during the connection's existing outbound framing pass.

use crate::{Packet, packet::GameHeader, session_wire::SessionIdentity};
use bedrock_protocol_1_26_51::{
    enums::TextPacketType,
    packets::{self, PacketId},
    types::TextData,
    wire,
};
use bytes::Bytes;
use std::io;

/// Rewrites only identity-bearing entries; all other bytes keep their original shared allocation.
pub(super) fn entry(
    frame: wire::RawFrame,
    identity: Option<&SessionIdentity>,
) -> io::Result<Bytes> {
    if let Some(identity) = identity {
        let header = frame.header();
        if matches!(header.id, packets::Text::ID | packets::PlayerSkin::ID) {
            let id = PacketId::from_raw(header.id).expect("known identity packet");
            let mut packet = Packet::from_parts(
                GameHeader {
                    id,
                    from_subclient: u32::from(header.sender_subclient),
                    to_subclient: u32::from(header.target_subclient),
                },
                crate::codec::decode_client_body(id, &frame.body()).map_err(io::Error::other)?,
            );
            if normalize(&mut packet, identity) {
                let mut encoded = Vec::new();
                crate::codec::encode_entry(&packet, &mut encoded).map_err(io::Error::other)?;
                return without_length(Bytes::from(encoded));
            }
        }
    }
    without_length(frame.frame().clone())
}

/// Keeps the same subclient header and payload while removing only the batch entry's length.
fn without_length(entry: Bytes) -> io::Result<Bytes> {
    let mut reader = wire::Reader::from_shared(&entry);
    reader.read_var_u32().map_err(io::Error::other)?;
    Ok(entry.slice(reader.position()..))
}

/// Matches the existing upstream identity policy without changing messages, skins, or nonchat text.
fn normalize(packet: &mut Packet, identity: &SessionIdentity) -> bool {
    match &mut *packet.data {
        packets::Packet::Text(text) if !identity.display_name.is_empty() => {
            if let TextData::AuthorAndMessage {
                message_type: TextPacketType::Chat,
                player_name,
                ..
            } = &mut text.body
                && (*player_name != identity.display_name.as_str()
                    || text.sender_xuid != identity.xuid.as_str())
            {
                *player_name = identity.display_name.as_str().into();
                text.sender_xuid = identity.xuid.as_str().into();
                return true;
            }
        }
        packets::Packet::PlayerSkin(skin) => {
            if let Ok(uuid) = uuid::Uuid::parse_str(&identity.uuid)
                && skin.uuid != uuid
            {
                skin.uuid = uuid;
                return true;
            }
        }
        _ => {}
    }
    false
}
