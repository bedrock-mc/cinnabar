//! Pending original cache work must not stop the negotiated RakNet I/O owner.

use super::*;
use crate::login::native_network::{Connection, PacketConnection, transport::Transport};
use ::raknet::prelude::{
    Established, RakClock, RakPriority, RakReliability, RakServer, RakServerConfig, RakServerInput,
    RakServerOutput, RakSession, RakSessionInput, RakSessionOutput, RakSessionState, Sans,
};
use bedrock_client::packs::{PackOffer, PackTransferLimits};
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits,
    raknet::{DisconnectReason, RakNetConnection, RakNetError, RakNetOptions},
};
use bedrock_protocol_1_26_51::{packets, wire};
use bytes::Bytes;
use resource_pack::test_support::archive_bytes;
use std::net::SocketAddr;
use tokio::{
    net::UdpSocket,
    sync::{OwnedSemaphorePermit, oneshot},
    time::{Instant, MissedTickBehavior},
};

/// Drives the original server/socket and releases cache admission after the ACK observation.
async fn peer(
    socket: UdpSocket,
    mut waiting: oneshot::Receiver<()>,
    held: OwnedSemaphorePermit,
    message: Bytes,
) -> Result<bool, String> {
    let address = socket.local_addr().map_err(|error| error.to_string())?;
    let config = RakServerConfig::default();
    let allowance = config.session.close_timeout;
    let mut updates = tokio::time::interval(config.session.autoflush_interval_ms);
    updates.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut buffer = vec![0; usize::from(config.max_mtu_size)];
    let now = Instant::now().into_std();
    let mut server = RakServer::new(config, address, RakClock::new(now), now)
        .map_err(|error| format!("original server configuration: {error}"))?;
    let mut session: Option<RakSession<Established>> = None;
    let mut cache_held = Some(held);
    let mut cache_waiting = false;
    let mut observation = None;
    let mut acknowledged = None;
    loop {
        tokio::select! {
            ready = &mut waiting, if !cache_waiting => {
                ready.map_err(|_| "original pending cache wait was not entered".to_owned())?;
                cache_waiting = true;
            }
            received = socket.recv_from(&mut buffer) => {
                let (length, remote) = received.map_err(|error| format!("original peer socket receive: {error}"))?;
                server.handle(RakServerInput::Datagram(
                    buffer[..length].into(), remote, Instant::now().into_std(),
                )).map_err(|error| format!("original server input: {error}"))?;
            }
            _ = updates.tick() => {
                let now = Instant::now().into_std();
                server.handle(RakServerInput::Update(now))
                    .map_err(|error| format!("original server timer: {error}"))?;
                if let Some(session) = &mut session {
                    session.handle(RakSessionInput::Update(now))
                        .map_err(|error| format!("original peer timer: {error}"))?;
                }
            }
        }
        while let Some(output) = server.poll() {
            match output {
                RakServerOutput::SocketDatagram(bytes, remote) => {
                    socket
                        .send_to(&bytes, remote)
                        .await
                        .map_err(|error| format!("original handshake output: {error}"))?;
                }
                RakServerOutput::SessionConnected(ready) => {
                    if session.replace(ready).is_some() {
                        return Err(
                            "original server unexpectedly negotiated another owner".to_owned()
                        );
                    }
                }
                RakServerOutput::SessionDatagram(bytes, _) => {
                    session
                        .as_mut()
                        .ok_or_else(|| "session input preceded its genuine handshake".to_owned())?
                        .handle(RakSessionInput::Datagram(bytes, Instant::now().into_std()))
                        .map_err(|error| format!("original peer input: {error}"))?;
                }
                RakServerOutput::SessionClosing(_) => {
                    return Err("fresh negotiation produced a closing owner".to_owned());
                }
            }
        }
        let Some(session) = &mut session else {
            continue;
        };
        if cache_waiting && observation.is_none() {
            session
                .send(
                    message.as_ref().into(),
                    RakReliability::ReliableOrdered,
                    RakPriority::Immediate,
                    Instant::now().into_std(),
                )
                .map_err(|error| format!("original reliable batch admission: {error}"))?;
            if session.outbound_bytes() == 0 {
                return Err("original reliable batch has no retained ACK obligation".to_owned());
            }
            observation = Some(Instant::now() + allowance);
        }
        session
            .tick(Instant::now().into_std())
            .map_err(|error| format!("original peer flush: {error}"))?;
        while let Some(output) = session.poll() {
            match output {
                RakSessionOutput::Datagram(bytes, remote) => {
                    socket
                        .send_to(&bytes, remote)
                        .await
                        .map_err(|error| format!("original reliable output: {error}"))?;
                }
                RakSessionOutput::Packet(_) => {
                    return Err("cache wait unexpectedly sent application input".to_owned());
                }
                RakSessionOutput::Disconnected(..) => {
                    return Err("original peer retired before its controlled close".to_owned());
                }
                RakSessionOutput::Wait(_) => {}
            }
        }
        if let Some(deadline) = observation {
            if acknowledged.is_none()
                && (session.outbound_bytes() == 0 || Instant::now() >= deadline)
            {
                acknowledged = Some(session.outbound_bytes() == 0);
                drop(cache_held.take());
            }
            if acknowledged.is_some() && session.outbound_bytes() == 0 {
                break;
            }
        }
    }
    let mut closing = session
        .take()
        .ok_or_else(|| "original negotiated peer was lost".to_owned())?
        .close(Instant::now().into_std())
        .map_err(|error| format!("original peer close: {error}"))?;
    while let Some(output) = closing.poll() {
        if let RakSessionOutput::Datagram(bytes, remote) = output {
            socket
                .send_to(&bytes, remote)
                .await
                .map_err(|error| format!("original peer terminal output: {error}"))?;
        }
    }
    if closing.state() != RakSessionState::Disconnected || closing.finish().is_err() {
        return Err("original peer did not finish after draining its terminal output".to_owned());
    }
    drop(socket);
    acknowledged.ok_or_else(|| "original ACK observation was never reached".to_owned())
}

#[tokio::test]
async fn pending_pack_cache_io_keeps_original_raknet_ack_and_packet_prefix() {
    let root = tempfile::tempdir().unwrap();
    let cache = open_cache(Some(root.path())).await.unwrap();
    let cancelled = Cancellation::new();
    let id = uuid::Uuid::from_u128(6);
    let archive = archive_bytes(id, &[]);
    let mut offered = PackInfoData::default();
    offered.pack_id_version.pack_uuid = id;
    offered.pack_id_version.pack_version.version = "1.0.0".into();
    offered.pack_size = wire::U64LE(archive.len() as u64);
    let mut offer = PackOffer::new(
        packets::ResourcePacksInfo {
            resource_packs: vec![offered.clone()],
            ..Default::default()
        },
        PackTransferLimits::default(),
    )
    .unwrap();
    offer.supply_preloaded(id, archive.clone()).unwrap();
    let mut stores = Stores::new();
    let held = Arc::clone(&stores.slot).acquire_owned().await.unwrap();
    stores.register(&offered);
    let (mut offer, _) = offer.start().unwrap();
    stores.queue_completed(
        &Some(Arc::clone(&cache)),
        |id| offer.completed_archive(id),
        &cancelled.0,
    );
    assert_eq!(
        stores.jobs.len(),
        1,
        "the original cache write must be pending"
    );

    let expected = [1, 2].map(|time| {
        crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(time),
        })
    });
    let entries = expected
        .iter()
        .map(crate::session_transport::tests::packet_bytes)
        .collect::<Vec<_>>();
    let mut batches = BatchCodec::new(BatchHeader::RakNet, BatchLimits::default())
        .encode(&entries)
        .unwrap();
    assert_eq!(
        batches.len(),
        1,
        "the siblings share one original reliable batch"
    );
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let (ready, waiting) = oneshot::channel();
    let options = RakNetOptions::default();
    let cutoff =
        Instant::now() + options.connect_timeout + options.close_timeout + options.close_timeout;
    let mut client_address: Option<SocketAddr> = None;
    let result = tokio::time::timeout_at(cutoff, async {
        let serving = peer(socket, waiting, held, batches.remove(0));
        let application = async {
            let actual = RakNetConnection::connect(address, options)
                .await
                .map_err(|error| format!("actual public RakNet connection: {error}"))?;
            client_address = Some(actual.local_addr());
            let mut connection = Connection::new(Transport::from_raknet(actual));
            let observed = async {
                let mut waiting = Box::pin(super::super::wait_io(&mut connection, stores.finish()));
                if !futures::poll!(waiting.as_mut()).is_pending() {
                    return Err("the original held cache job completed before release".to_owned());
                }
                ready
                    .send(())
                    .map_err(|_| "original peer did not retain cache observation".to_owned())?;
                waiting.await;
                let first = connection
                    .next_packet()
                    .await
                    .map_err(|error| format!("first original retained packet: {error}"))?;
                let second = connection
                    .next_packet()
                    .await
                    .map_err(|error| format!("second original retained packet: {error}"))?;
                let terminal =
                    connection.next_packet().await.err().ok_or_else(|| {
                        "expected original peer terminal after its prefix".to_owned()
                    })?;
                Ok::<_, String>(([first, second], terminal))
            }
            .await;
            let closed = connection.close().await;
            drop(connection);
            closed.map_err(|error| format!("original client close: {error}"))?;
            observed
        };
        tokio::join!(serving, application)
    })
    .await;
    // The original cache owner is joined even if an earlier transport/setup stage failed.
    stores.finish().await;
    let server_socket_released = UdpSocket::bind(address).await.is_ok();
    let client_socket_released = match client_address {
        Some(address) => UdpSocket::bind(address).await.is_ok(),
        None => false,
    };
    let loaded = preload(&offered, Some(cache), None, Arc::clone(&cancelled.0)).await;
    let (served, received) = result.expect("finite original RakNet/cache fixture stalled");
    let acknowledged = served
        .expect("setup: original negotiation, ACK observation and peer retirement must complete");
    let (received, terminal) =
        received.expect("setup: actual packet prefix and client retirement must complete");
    assert!(
        server_socket_released && client_socket_released,
        "original UDP sockets remained owned"
    );
    assert!(loaded.cache_hit, "the original cache job did not finish");
    assert_eq!(loaded.archive, Some(archive));
    assert_eq!(received, expected);
    assert!(
        matches!(
            terminal
                .get_ref()
                .and_then(|error| error.downcast_ref::<RakNetError>()),
            Some(RakNetError::Disconnected(DisconnectReason::Remote))
        ),
        "the original typed RakNet terminal must follow its retained prefix: {terminal}"
    );
    assert!(
        acknowledged,
        "pending original pack cache I/O stopped RakNet ACK progress"
    );
}
