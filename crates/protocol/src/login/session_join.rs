//! Joins through the core's session endpoint: the core makes the only login and acquires the packs,
//! and this client continues from StartGame with Jolyne's spawn sequence.

use std::path::Path;

use bridge::{
    BridgeError, ConnectRequest, CoreMessage, FramedReader, HandoffPackReceiver, SessionDisconnect,
    SessionHandoff, SessionTransfer,
};
use bytes::{BufMut, Bytes, BytesMut};
use futures::StreamExt;
use jolyne::error::JolyneError;
use jolyne::stream::transport::BedrockTransport;
use jolyne::stream::{BedrockStream, Client, StartGame};
use valentine::bedrock::codec::{BedrockCodec, ZigZag32};
use valentine::bedrock::version::v1_26_51::{
    DisconnectPacket, DisconnectPacketMessages, EnumsConnectionDisconnectFailReason,
    McpePacketData, TransferPacket,
};

use super::client_data::LoginSettings;
use super::{LoginSequence, MAX_DECOMPRESSED_BATCH_SIZE, PlaySession};
use crate::session_transport::SessionTransport;
use crate::{
    ClientBlobCache, ClientSkin, GameData, PROTOCOL_VERSION, Packet, ProtocolError,
    ResourcePackArchive, ResourcePackHandoff, ServerTransferEvent,
};

impl LoginSequence {
    /// Joins whatever the core has selected: a pending server transfer, the chosen server or the
    /// open local world. The owner calls `finish_loading` after presenting the world.
    pub async fn connect_session(
        socket_dir: &Path,
        display_name: &str,
        cache: Option<ClientBlobCache>,
        skin: Option<ClientSkin>,
        settings: &LoginSettings,
    ) -> Result<(PlaySession, GameData), ProtocolError> {
        let (mut reader, frames) = bridge::connect_session(socket_dir)
            .await
            .map_err(ProtocolError::Bridge)?;
        let request = ConnectRequest {
            protocol: PROTOCOL_VERSION,
            target: None,
            client_cache: cache.is_some(),
            client_data: super::client_data::login_client_data(
                display_name,
                skin.as_ref(),
                settings,
            ),
        };
        frames
            .send(bridge::encode_connect(&request).map_err(bridge_error)?)
            .await
            .map_err(bridge_error)?;
        let (handoff, archives) = receive_handoff(&mut reader).await?;
        let packs = resource_pack_handoff(&handoff, archives)?;
        // A blob cache serves only a session whose upstream login advertised one.
        let cache = cache.filter(|_| handoff.client_cache);
        let mut transport = BedrockTransport::new(SessionTransport::new(
            reader,
            frames,
            startup_batches(&handoff.startup),
        ));
        transport.set_max_decompressed_batch_size(Some(MAX_DECOMPRESSED_BATCH_SIZE));
        let (stream, game_data) =
            BedrockStream::<StartGame, Client, _>::from_session_handoff(transport, packs)
                .await_start_game()
                .await?;
        Ok((PlaySession::new(stream, cache), game_data))
    }
}

/// Reads the handoff and every archive it announces; a terminal message ends the join instead.
async fn receive_handoff(
    reader: &mut FramedReader,
) -> Result<(SessionHandoff, Vec<Vec<u8>>), ProtocolError> {
    let handoff = match next_message(reader).await? {
        CoreMessage::Handoff(handoff) => handoff,
        message => return Err(join_ended(message)),
    };
    let mut receiver = HandoffPackReceiver::new(&handoff);
    while !receiver.is_complete() {
        match next_message(reader).await? {
            CoreMessage::PackData { index, data } => {
                receiver.accept(index, &data).map_err(bridge_error)?;
            }
            message => return Err(join_ended(message)),
        }
    }
    let archives = receiver.into_archives().map_err(bridge_error)?;
    Ok((handoff, archives))
}

async fn next_message(reader: &mut FramedReader) -> Result<CoreMessage, ProtocolError> {
    match reader.next().await {
        Some(Ok(frame)) => bridge::decode_core_message(frame).map_err(bridge_error),
        Some(Err(error)) => Err(bridge_error(error)),
        None => Err(JolyneError::ConnectionClosed.into()),
    }
}

/// The error a message other than the next setup step ends the join with, as a login ended before.
fn join_ended(message: CoreMessage) -> ProtocolError {
    match message {
        CoreMessage::Disconnect(disconnect) => {
            let McpePacketData::DisconnectPacket(packet) = disconnect_packet(&disconnect).data
            else {
                unreachable!("a disconnect message builds a Disconnect packet")
            };
            JolyneError::from(jolyne::error::ProtocolError::ServerDisconnect {
                stage: "login",
                reason: format!("{:?}", packet.reason),
                message: packet.messages.message,
                filtered_message: packet.messages.filtered_message,
            })
            .into()
        }
        CoreMessage::Transfer(transfer) => {
            match ServerTransferEvent::from_packet_data(&transfer_packet(&transfer).data) {
                Ok(Some(target)) => {
                    JolyneError::from(jolyne::error::ProtocolError::ServerTransfer(target)).into()
                }
                _ => unexpected("unusable transfer before the handoff"),
            }
        }
        CoreMessage::Handoff(_) => unexpected("repeated handoff"),
        CoreMessage::PackData { .. } => unexpected("pack data before the handoff"),
        CoreMessage::Batch(_) => unexpected("batch before the handoff completed"),
    }
}

fn unexpected(reason: &'static str) -> ProtocolError {
    bridge_error(BridgeError::InvalidSessionMessage { reason })
}

fn bridge_error(error: BridgeError) -> ProtocolError {
    ProtocolError::Bridge(error.into())
}

/// Carries the handed-off archives in their stack order, with sub-packs, content keys and the
/// required bit, as the client's own negotiation captured them.
fn resource_pack_handoff(
    handoff: &SessionHandoff,
    archives: Vec<Vec<u8>>,
) -> Result<ResourcePackHandoff, ProtocolError> {
    let archives = handoff
        .packs
        .iter()
        .zip(archives)
        .map(|(pack, archive)| {
            let id = uuid::Uuid::parse_str(&pack.uuid)
                .map_err(|_| unexpected("handoff pack identity is not a UUID"))?;
            Ok(ResourcePackArchive::with_content_key(
                id,
                pack.version.clone(),
                pack.sub_pack.clone(),
                archive,
                pack.content_key.expose().as_bytes().to_vec(),
            ))
        })
        .collect::<Result<Vec<_>, ProtocolError>>()?;
    Ok(ResourcePackHandoff::from_archives(archives).with_required(handoff.packs_required))
}

/// Groups the startup packets, which may span many upstream batches, into uncompressed Bedrock
/// batches that each stay within Jolyne's per-batch packet bound.
fn startup_batches(packets: &[Bytes]) -> Vec<Bytes> {
    packets
        .chunks(jolyne::raw::MAX_RAW_BATCH_PACKETS)
        .map(|chunk| {
            let mut batch = BytesMut::with_capacity(
                1 + chunk.iter().map(|packet| 5 + packet.len()).sum::<usize>(),
            );
            batch.put_u8(0xfe);
            for packet in chunk {
                let mut length = u32::try_from(packet.len()).expect("a frame bounds its packets");
                while length >= 0x80 {
                    batch.put_u8(length as u8 | 0x80);
                    length >>= 7;
                }
                batch.put_u8(length as u8);
                batch.extend_from_slice(packet);
            }
            batch.freeze()
        })
        .collect()
}

/// The Transfer packet a core Transfer message stands for.
pub(crate) fn transfer_packet(transfer: &SessionTransfer) -> Packet {
    TransferPacket {
        server_address: transfer.address.clone(),
        server_port: transfer.port,
        reload_world: transfer.reload_world,
        gatherings_configuration: None,
    }
    .into()
}

/// The Disconnect packet a core Disconnect message stands for; a reason this protocol does not
/// name keeps the message and reads as unknown.
pub(crate) fn disconnect_packet(disconnect: &SessionDisconnect) -> Packet {
    let mut reason = BytesMut::new();
    ZigZag32(disconnect.reason)
        .encode(&mut reason)
        .expect("a buffer accepts a varint");
    let reason = EnumsConnectionDisconnectFailReason::decode(&mut reason.freeze(), ())
        .unwrap_or(EnumsConnectionDisconnectFailReason::Unknown);
    DisconnectPacket {
        reason,
        hide_disconnection_screen: disconnect.hide_screen,
        messages: DisconnectPacketMessages {
            message: disconnect.message.clone(),
            filtered_message: disconnect.filtered_message.clone(),
        },
    }
    .into()
}

#[cfg(all(test, unix))]
mod tests;
