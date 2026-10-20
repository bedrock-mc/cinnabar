//! Shared spawn and finite preparation after the native connection negotiates its packs.

use super::super::{JoinPreparation, NativeJoinPreparation, PlaySession, SessionLogin};
use super::negotiated::NegotiatedConnection;
use crate::session_transport::SessionTransport;
use crate::{GameData, ProtocolError};
use bedrock_packs::ResourcePackHandoff;

/// Consumes the negotiated connection once without replacing its raw FIFO or pack archives.
pub(super) async fn continue_join<P: Send + 'static>(
    negotiated: NegotiatedConnection,
    login: SessionLogin<'_>,
    prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
) -> Result<(PlaySession, P), ProtocolError> {
    let NegotiatedConnection {
        mut connection,
        packs,
    } = negotiated;
    if login.relay.is_some() {
        connection.defer_through_start_game().await?;
    }
    let deferred = connection.take_deferred();
    let packs = if let Some(relay) = &login.relay {
        let last = deferred
            .iter()
            .position(|packet| packet.id == bedrock_protocol_1_26_51::packets::PacketId::StartGame)
            .expect("the original deferred prefix reached first StartGame");
        let startup: Vec<_> = deferred[..=last]
            .iter()
            .map(crate::raw_packet::RawPacket::encoded)
            .collect();
        let added = crate::packet_relay::start(
            std::sync::Arc::clone(relay),
            connection
                .identity
                .as_ref()
                .expect("negotiation retains the actual Login identity")
                .clone(),
            super::super::client_data::login_client_data(
                login.display_name,
                login.client_random_id,
                login.skin.as_ref(),
                login.settings,
            )
            .to_string(),
            &startup,
        )
        .await;
        packs.with_relay_packs(added)
    } else {
        packs
    };
    let mut transport = SessionTransport::from_native(connection).with_tap(login.tap);
    if let Some(relay) = login.relay {
        transport.set_relay(relay);
    }
    transport.adopt_startup(deferred);
    let preparation = NativeJoinPreparation;
    let mut prepared_job = None;
    let initialization = super::super::startup::await_spawn(
        &mut transport,
        login.settings.view_distance_chunks,
        |start, items| {
            prepared_job = Some(
                preparation.submit(move || prepare(packs, GameData::from_wire(&start, &items))),
            );
        },
    )
    .await?;
    let job = prepared_job.expect("spawn requires StartGame and item registry");
    transport.finish_startup();
    let prepared = super::super::session_join::prepare_with_read_ahead(&mut transport, job).await?;
    Ok((
        PlaySession::new(transport, login.cache, initialization),
        prepared,
    ))
}
