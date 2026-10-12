//! Joins through the core's session endpoint: the core makes the only login and acquires the packs,
//! and this client continues from StartGame with its own spawn sequence.

use std::path::Path;

use bridge::{ConnectTarget, CoreEndpoint};

use crate::session_wire::{ConnectRequest, CoreMessage, HandoffPackReceiver, SessionHandoff};
use bridge::{BridgeError, FramedReader};
use futures::StreamExt;

use super::client_data::{ClientRandomId, LoginSettings};
use super::{LoginSequence, PlaySession};
use crate::codec::SessionFailure;
use crate::disconnect::{disconnect_reason, join_disconnect};
use crate::session_transport::SessionTransport;
use crate::{
    ClientBlobCache, ClientSkin, GameData, PROTOCOL_VERSION, ProtocolError, ResourcePackArchive,
    ResourcePackHandoff, ServerTransferEvent,
};

impl LoginSequence {
    /// Joins `target`, or without one whatever the core has selected: a pending server transfer,
    /// the chosen server or the open local world, using the caller's persisted login ID. The owner
    /// calls `finish_loading` after presenting the world.
    pub async fn connect_session(
        core: &CoreEndpoint,
        target: Option<ConnectTarget>,
        display_name: &str,
        client_random_id: ClientRandomId,
        cache: Option<ClientBlobCache>,
        skin: Option<ClientSkin>,
        settings: &LoginSettings,
    ) -> Result<(PlaySession, GameData), ProtocolError> {
        let (mut reader, frames) = bridge::connect_session(core)
            .await
            .map_err(ProtocolError::Bridge)?;
        let request = ConnectRequest {
            protocol: PROTOCOL_VERSION,
            target,
            client_cache: cache.is_some(),
            client_data: super::client_data::login_client_data(
                display_name,
                client_random_id,
                skin.as_ref(),
                settings,
            ),
        };
        frames
            .send(crate::session_wire::encode_connect(&request).map_err(bridge_error)?)
            .await
            .map_err(bridge_error)?;
        let (handoff, archives) =
            receive_handoff(&mut reader, settings.resource_pack_cache_dir.as_deref()).await?;
        let packs = resource_pack_handoff(&handoff, archives)?;
        // A blob cache serves only a session whose upstream login advertised one.
        let cache = cache.filter(|_| handoff.client_cache);
        let mut transport = SessionTransport::new(reader, frames, &handoff.startup);
        let spawned = super::startup::await_spawn(&mut transport).await?;
        let runtime_id = spawned.runtime_id;
        let terrain_before_spawn = spawned.terrain_before_spawn;
        let game_data = async_platform::run_blocking(move || {
            GameData::from_wire(&spawned.start_game, &spawned.item_registry)
        })
        .await
        .and_then(Result::ok)
        .ok_or_else(|| unexpected("startup facts reader failed"))?;
        let session = PlaySession::new(transport, cache, packs, runtime_id, terrain_before_spawn);
        Ok((session, game_data))
    }
}

/// Reads the handoff and every archive it announces; a terminal message ends the join instead.
async fn receive_handoff(
    reader: &mut FramedReader,
    cache_root: Option<&Path>,
) -> Result<(SessionHandoff, Vec<Vec<u8>>), ProtocolError> {
    let handoff = match next_message(reader).await? {
        CoreMessage::Handoff(handoff) => handoff,
        message => return Err(join_ended(message)),
    };
    let cache_root = cache_root.map(Path::to_path_buf);
    let (handoff, receiver) = async_platform::run_blocking(move || {
        let receiver = HandoffPackReceiver::new(&handoff, cache_root.as_deref());
        (handoff, receiver)
    })
    .await
    .and_then(Result::ok)
    .ok_or_else(|| unexpected("cache archive reader failed"))?;
    let mut receiver = receiver.map_err(bridge_error)?;
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
        Some(Ok(frame)) => crate::session_wire::decode_core_message(frame).map_err(bridge_error),
        Some(Err(error)) => Err(bridge_error(error)),
        None => Err(SessionFailure::ConnectionClosed.into()),
    }
}

/// The error a message other than the next setup step ends the join with, as a login ended before.
fn join_ended(message: CoreMessage) -> ProtocolError {
    match message {
        CoreMessage::Disconnect(disconnect) => join_disconnect(
            "login",
            &disconnect_reason(disconnect.reason),
            &disconnect.message,
            &disconnect.filtered_message,
        )
        .into(),
        CoreMessage::Transfer(transfer) => {
            match ServerTransferEvent::from_target(
                &transfer.address,
                transfer.port,
                transfer.reload_world,
            ) {
                Ok(target) => SessionFailure::ServerTransfer(target).into(),
                Err(_) => unexpected("unusable transfer before the handoff"),
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

#[cfg(all(test, unix))]
mod tests;
