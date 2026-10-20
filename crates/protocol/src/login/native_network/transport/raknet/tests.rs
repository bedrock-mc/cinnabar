//! Valid aggregate pressure must retain the original negotiated RakNet owner.

use crate::login::native_network::{Connection, transport::Transport};
use crate::session_transport::{batch_body, native};
use ::raknet::prelude::{
    Established, RakClock, RakPriority, RakReliability, RakServer, RakServerConfig, RakServerInput,
    RakServerOutput, RakSession, RakSessionInput, RakSessionOutput, Sans,
};
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits,
    raknet::{RakNetConnection, RakNetOptions},
};
use bedrock_protocol_1_26_51::{packets, wire};
use bytes::Bytes;
use tokio::{
    net::UdpSocket,
    sync::oneshot,
    time::{Instant, MissedTickBehavior},
};

/// Uses the same generated packet in its actual transport batch and pump body.
fn message(index: i32) -> (Bytes, Bytes) {
    let packet = crate::Packet::new(packets::SetTime {
        time: wire::ZigZag32(index),
    });
    let entry = crate::session_transport::tests::packet_bytes(&packet);
    let batches = BatchCodec::new(BatchHeader::RakNet, BatchLimits::default())
        .encode(&[entry])
        .unwrap();
    assert_eq!(batches.len(), 1);
    (
        batches.into_iter().next().unwrap(),
        batch_body(&[packet]).unwrap(),
    )
}

/// Negotiates on the real socket, then observes original reliable ACK and marker input.
async fn peer(
    socket: &UdpSocket,
    mut pressured: oneshot::Receiver<()>,
    mut released: oneshot::Receiver<()>,
    acknowledged: oneshot::Sender<()>,
    marker_seen: oneshot::Sender<()>,
    total: usize,
) -> Result<(), String> {
    let address = socket.local_addr().map_err(|error| error.to_string())?;
    let config = RakServerConfig::default();
    let mut updates = tokio::time::interval(config.session.autoflush_interval_ms);
    updates.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut buffer = vec![0; usize::from(config.max_mtu_size)];
    let now = Instant::now().into_std();
    let mut server = RakServer::new(config, address, RakClock::new(now), now)
        .map_err(|error| format!("original server setup: {error}"))?;
    let mut session: Option<RakSession<Established>> = None;
    let mut acknowledged = Some(acknowledged);
    let mut marker_seen = Some(marker_seen);
    let mut pressure = false;
    let mut sent = 0;
    let outcome = async {
        loop {
            tokio::select! {
                biased;
                _ = &mut released => break,
                ready = &mut pressured, if !pressure => {
                    ready.map_err(|_| "original credited body was not retained".to_owned())?;
                    pressure = true;
                }
                received = socket.recv_from(&mut buffer) => {
                    let (length, remote) = received.map_err(|error| format!("original peer receive: {error}"))?;
                    let now = Instant::now().into_std();
                    if let Some(session) = &mut session {
                        if session.addr() != remote {
                            return Err("original peer route changed".to_owned());
                        }
                        session.handle(RakSessionInput::Datagram(buffer[..length].into(), now))
                            .map_err(|error| format!("original established peer input: {error}"))?;
                    } else {
                        server.handle(RakServerInput::Datagram(buffer[..length].into(), remote, now))
                            .map_err(|error| format!("original handshake input: {error}"))?;
                    }
                }
                _ = updates.tick() => {
                    let now = Instant::now().into_std();
                    server.handle(RakServerInput::Update(now))
                        .map_err(|error| format!("original handshake maintenance: {error}"))?;
                    if let Some(session) = &mut session {
                        session.handle(RakSessionInput::Update(now))
                            .map_err(|error| format!("original peer maintenance: {error}"))?;
                    }
                }
            }
            while let Some(output) = server.poll() {
                match output {
                    RakServerOutput::SocketDatagram(bytes, remote) => {
                        socket.send_to(&bytes, remote).await
                            .map_err(|error| format!("original handshake send: {error}"))?;
                    }
                    RakServerOutput::SessionConnected(ready) => {
                        if session.replace(ready).is_some() {
                            return Err("another session replaced the original peer".to_owned());
                        }
                    }
                    RakServerOutput::SessionDatagram(bytes, _) => {
                        session.as_mut().ok_or_else(|| "input preceded original handshake".to_owned())?
                            .handle(RakSessionInput::Datagram(bytes, Instant::now().into_std()))
                            .map_err(|error| format!("original negotiated peer input: {error}"))?;
                    }
                    RakServerOutput::SessionClosing(_) => {
                        return Err("fresh handshake returned a closing owner".to_owned());
                    }
                }
            }
            let Some(session) = &mut session else { continue };
            let target = if pressure { total } else { 1 };
            let previous = sent;
            while sent < target {
                let (batch, _) = message(sent.try_into().map_err(|_| "fixture sequence exceeds i32")?);
                session.send(
                    batch.as_ref().into(), RakReliability::ReliableOrdered,
                    RakPriority::Immediate, Instant::now().into_std(),
                ).map_err(|error| format!("original reliable batch admission: {error}"))?;
                sent += 1;
            }
            if sent != previous && session.outbound_bytes() == 0 {
                return Err("original reliable batches retained no ACK obligation".to_owned());
            }
            session.tick(Instant::now().into_std())
                .map_err(|error| format!("original peer flush: {error}"))?;
            while let Some(output) = session.poll() {
                match output {
                    RakSessionOutput::Datagram(bytes, remote) => {
                        socket.send_to(&bytes, remote).await
                            .map_err(|error| format!("original peer wire output: {error}"))?;
                    }
                    RakSessionOutput::Packet(bytes) => {
                        if bytes.as_ref() != message(-1).0.as_ref() {
                            return Err("original peer received a changed marker".to_owned());
                        }
                        if let Some(seen) = marker_seen.take() { let _ = seen.send(()); }
                    }
                    RakSessionOutput::Disconnected(..) | RakSessionOutput::Wait(_) => {}
                }
            }
            if sent == total && session.outbound_bytes() == 0
                && let Some(acknowledged) = acknowledged.take()
            {
                let _ = acknowledged.send(());
            }
        }
        Ok(())
    }.await;
    if let Some(session) = session {
        let mut closing = match session.abort(Instant::now().into_std()) {
            Ok(closing) => closing,
            Err(error) => error.into_parts().0,
        };
        while let Some(output) = closing.poll() {
            if let RakSessionOutput::Datagram(bytes, remote) = output {
                socket
                    .send_to(&bytes, remote)
                    .await
                    .map_err(|error| format!("original peer cleanup output: {error}"))?;
            }
        }
        if closing.finish().is_err() {
            return Err("original peer did not finish after explicit retirement".to_owned());
        }
    }
    outcome
}

#[tokio::test(flavor = "current_thread")]
async fn stalled_native_consumer_keeps_raknet_control_and_accepted_prefix() {
    let limits = BatchLimits::default();
    let total = limits.max_packets.checked_add(2).unwrap();
    let (first_batch, first_body) = message(0);
    let budget = first_body.len().checked_sub(1).unwrap();
    assert!(budget != 0 && first_batch.len() < limits.max_encoded_bytes);
    let offered_bytes = (0..total)
        .try_fold(0_usize, |bytes, index| {
            bytes.checked_add(message(index.try_into().unwrap()).0.len())
        })
        .unwrap();
    assert!(offered_bytes < limits.max_encoded_bytes);
    let options = RakNetOptions::default();
    let cutoff =
        Instant::now() + options.connect_timeout + options.close_timeout + options.close_timeout;
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let mut local = None;
    let (pressure, pressured) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let (acknowledged, ack) = oneshot::channel();
    let (marker_seen, seen) = oneshot::channel();
    let outcome = tokio::time::timeout_at(cutoff, async {
        let serving = peer(
            &socket,
            pressured,
            released,
            acknowledged,
            marker_seen,
            total,
        );
        let application = async {
            let actual = RakNetConnection::connect(address, options)
                .await
                .map_err(|error| format!("actual client negotiation: {error}"))?;
            local = Some(actual.local_addr());
            let connection = Connection::new(Transport::from_raknet(actual));
            let native::PreparedPump {
                sender,
                mut incoming,
                stop,
                driver,
            } = native::prepare_with_byte_budget(connection, Some(budget));
            let consuming = async {
                let observed = async {
                    let first = incoming
                        .recv()
                        .await
                        .ok_or_else(|| "original first body was not delivered".to_owned())?
                        .into_body()
                        .map_err(|error| format!("original first body: {error}"))?;
                    let first_matches = first == first_body;
                    pressure
                        .send(())
                        .map_err(|_| "original peer pressure gate closed".to_owned())?;
                    ack.await
                        .map_err(|_| "original reliable ACK observation was lost".to_owned())?;
                    let written = match sender.accept(message(-1).1).await {
                        Ok(receipt) => receipt
                            .await
                            .map_err(|_| "original write receipt was lost".to_owned())?,
                        Err(error) => Err(error),
                    };
                    let marker_received = if written.is_ok() {
                        seen.await.is_ok()
                    } else {
                        false
                    };
                    // The same credited first body gates payload reads through both observations.
                    drop(first);
                    let mut received = 1;
                    let mut prefix_matches = first_matches;
                    let mut terminal = None;
                    while received < total {
                        let Some(item) = incoming.recv().await else {
                            break;
                        };
                        match item.into_body() {
                            Ok(body) => {
                                prefix_matches &= body == message(received.try_into().unwrap()).1;
                                received += 1;
                            }
                            Err(error) => {
                                terminal = Some(error);
                                break;
                            }
                        }
                    }
                    Ok::<_, String>((written, marker_received, received, prefix_matches, terminal))
                }
                .await;
                let _ = stop.send(());
                drop(sender);
                drop(incoming);
                let _ = release.send(());
                observed
            };
            let (end, observed) = tokio::join!(driver, consuming);
            drop(end.pending);
            Ok::<_, String>((end.close, observed))
        };
        tokio::join!(serving, application)
    })
    .await;
    drop(socket);
    let peer_socket_released = UdpSocket::bind(address).await.is_ok();
    let client_socket_released = match local {
        Some(local) => UdpSocket::bind(local).await.is_ok(),
        None => false,
    };
    assert!(
        peer_socket_released && client_socket_released,
        "original sockets remained owned"
    );
    let (served, application) = outcome.expect("finite original maintenance fixture stalled");
    served.expect("original negotiated peer and cleanup must finish");
    let (closed, observed) =
        application.expect("original client negotiation and driver must finish");
    closed.expect("original native pump close must complete");
    let (written, marker_received, received, prefix_matches, terminal) =
        observed.expect("original credited pressure and actual ACK stages must complete");
    assert!(prefix_matches, "the original accepted FIFO changed");
    assert!(
        written.is_ok() && marker_received && received == total && terminal.is_none(),
        "valid aggregate pressure retired the original RakNet connection: write={written:?}, marker_received={marker_received}, received={received}/{total}, terminal={terminal:?}"
    );
}
