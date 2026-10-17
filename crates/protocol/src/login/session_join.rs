//! Joins through the core's session endpoint: the core makes the only login and acquires the packs,
//! and this client continues from StartGame with its own spawn sequence.

use std::path::Path;
use std::sync::Arc;

use crate::session_wire::{ConnectRequest, CoreMessage, HandoffPackReceiver, SessionHandoff};
use bridge::{BridgeError, FrameQueue, FrameRead};
use futures::StreamExt;
use std::future::Future;

use super::client_data::{ClientRandomId, LoginSettings};
use super::{LoginSequence, PlaySession};
use crate::codec::SessionFailure;
use crate::disconnect::{disconnect_reason, join_disconnect};
use crate::packet_tap::{PacketOutcome, PacketTap};
use crate::session_transport::{MAX_READ_AHEAD_BYTES, SessionTransport};
use crate::{
    ClientBlobCache, ClientSkin, GameData, PROTOCOL_VERSION, ProtocolError, ResourcePackArchive,
    ResourcePackHandoff, ServerTransferEvent,
};

/// Prepared ordered play I/O and the host's timer registration function.
pub struct SessionIo<R> {
    pub reader: R,
    pub frames: FrameQueue,
    pub timers: bridge::tasks::Timers,
}

/// Login choices shared by native endpoints and relay play streams.
pub struct SessionLogin<'a> {
    pub display_name: &'a str,
    pub client_random_id: ClientRandomId,
    pub cache: Option<ClientBlobCache>,
    pub skin: Option<ClientSkin>,
    pub settings: &'a LoginSettings,
    pub tap: Option<Arc<dyn PacketTap>>,
    pub target: Option<bridge::ConnectTarget>,
}

/// Schedules the two startup jobs while login continues reading the ordered play stream.
/// Native hosts offload blocking work; a local host owns yielding and cancellation.
pub trait JoinPreparation {
    type Task<T: Send + 'static>: Future<Output = T>;

    /// Submits one finite startup job without running it inside the receive loop.
    fn submit<T: Send + 'static>(&self, work: impl FnOnce() -> T + Send + 'static)
    -> Self::Task<T>;
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeJoinPreparation;

impl LoginSequence {
    /// Joins over prepared play I/O using the existing handoff and spawn sequence. Dropping
    /// this future ends this join; the host must also cancel its writer and startup jobs.
    pub async fn connect_prepared<R: FrameRead, P: Send + 'static>(
        io: SessionIo<R>,
        login: SessionLogin<'_>,
        preparation: &impl JoinPreparation,
        prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
    ) -> Result<(PlaySession<R>, P), ProtocolError> {
        let SessionIo {
            mut reader,
            frames,
            timers,
        } = io;
        let SessionLogin {
            display_name,
            client_random_id,
            cache,
            skin,
            settings,
            tap,
            target,
        } = login;
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
        let (handoff, archives) = receive_handoff(
            &mut reader,
            settings.resource_pack_cache_dir.as_deref(),
            preparation,
        )
        .await?;
        let packs = resource_pack_handoff(&handoff, archives)?;
        // A blob cache serves only a session whose upstream login advertised one.
        let cache = cache.filter(|_| handoff.client_cache);
        reader.set_trace_conn(handoff.trace_conn);
        let mut transport =
            SessionTransport::with_timers(reader, frames, &handoff.startup, timers).with_tap(tap);
        let mut prepared_job = None;
        let spawned =
            match super::startup::await_spawn(&mut transport, |start_game, item_registry| {
                prepared_job = Some(preparation.submit(move || {
                    prepare(packs, GameData::from_wire(&start_game, &item_registry))
                }));
            })
            .await
            {
                Ok(spawned) => spawned,
                Err(error) => {
                    if let ProtocolError::Session(session) = &error
                        && session.0.is_packet_failure()
                    {
                        transport.reject_current(PacketOutcome::Failed);
                    }
                    return Err(error);
                }
            };
        let preparation = prepared_job.expect("spawn waits for StartGame and the item registry");
        tokio::pin!(preparation);
        // Keep reading while the packs prepare, so the server's backlog never stalls the core.
        let prepared = tokio::select! {
            prepared = &mut preparation => prepared,
            () = transport.read_ahead(MAX_READ_AHEAD_BYTES) => preparation.await,
        };
        let session = PlaySession::new(
            transport,
            cache,
            spawned.runtime_id,
            spawned.terrain_before_spawn,
        );
        Ok((session, prepared))
    }
}

/// Reads the handoff and every archive it announces; a terminal message ends the join instead.
async fn receive_handoff<R: FrameRead>(
    reader: &mut R,
    cache_root: Option<&Path>,
    preparation: &impl JoinPreparation,
) -> Result<(SessionHandoff, Vec<Vec<u8>>), ProtocolError> {
    let handoff = match next_message(reader).await? {
        CoreMessage::Handoff(handoff) => handoff,
        message => return Err(join_ended(message)),
    };
    let cache_root = cache_root.map(Path::to_path_buf);
    let (handoff, receiver) = preparation
        .submit(move || {
            let receiver = HandoffPackReceiver::new(&handoff, cache_root.as_deref());
            (handoff, receiver)
        })
        .await;
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

async fn next_message<R: FrameRead>(reader: &mut R) -> Result<CoreMessage, ProtocolError> {
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
