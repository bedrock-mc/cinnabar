use super::*;
use crate::login::native_network::transport::LAN_CONNECTION_TYPE;
use bedrock_net::BatchHeader;
use bytes::Bytes;
use nethernet_tokio::{ConnectionConfig, LanConfig, LanSignaling, NetherServer, ServerData};
use std::{net::Ipv6Addr, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::UdpSocket,
    process::Command,
};

/// A real localhost answer remains usable when an unsupported IPv6 candidate precedes it.
#[tokio::test]
async fn native_lan_resolution_skips_ipv6_and_keeps_the_ready_ipv4_owner() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let reservation = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let signaling = LanSignaling::with_config(
            31,
            address,
            LanConfig {
                discovery_port: address.port(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut data = ServerData::new("Fixture".into(), "World".into());
        data.flag_b = true;
        data.session_id = "original-fixture-nonce".into();
        data.connection_type = LAN_CONNECTION_TYPE;
        data.protocol_version = crate::PROTOCOL_VERSION as u32;
        signaling.set_server_data(data);
        let mut listener = NetherServer::bind_with(
            signaling,
            ConnectionConfig {
                allow_unsigned_answers: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let deadline = Instant::now() + crate::NATIVE_JOIN_TIMEOUT;
        let resolved = tokio::net::lookup_host(("localhost", address.port()))
            .await
            .unwrap();
        let addresses =
            std::iter::once(SocketAddr::new(Ipv6Addr::LOCALHOST.into(), address.port()))
                .chain(resolved);
        let (connected, peer) = tokio::join!(
            connect_resolved(NativeTransport::NetherNetLan, addresses, None, deadline),
            listener.accept(),
        );
        let mut connected = connected.unwrap();
        let mut peer = peer.unwrap();
        assert_eq!(connected.header(), BatchHeader::NetherNet);
        assert_eq!(connected.nonce(), "original-fixture-nonce");
        peer.session
            .send(Bytes::from_static(b"same peer"))
            .await
            .unwrap();
        assert_eq!(connected.recv().await.unwrap(), "same peer");
        connected
            .send_until(Bytes::from_static(b"same client"), None)
            .await
            .unwrap();
        assert_eq!(peer.reliable.recv().await.unwrap().unwrap(), "same client");
        connected.close().await.unwrap();
        let retired = peer.close();
        retired.session.closed().await;
        listener.close().await.unwrap();
    })
    .await
    .expect("finite IPv4 LAN resolution fixture stalled");
}

/// The actual pending RakNet socket is dropped on either cancellation or the original cutoff.
#[tokio::test]
async fn native_candidate_cancellation_and_original_deadline_release_actual_sockets() {
    for expires in [false, true] {
        let blackhole = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = blackhole.local_addr().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut connecting = Box::pin(connect_resolved(
            NativeTransport::RakNet,
            std::iter::once(address),
            None,
            deadline,
        ));
        let mut datagram = [0; 2048];
        let (_, source) = tokio::select! {
            result = blackhole.recv_from(&mut datagram) => result.unwrap(),
            _ = &mut connecting => panic!("a silent loopback peer completed a handshake"),
        };
        if expires {
            tokio::time::pause();
            tokio::time::advance(deadline.saturating_duration_since(Instant::now())).await;
            assert_eq!(
                connecting.await.err().unwrap().kind(),
                io::ErrorKind::TimedOut
            );
        } else {
            drop(connecting);
        }
        let _released = UdpSocket::bind(source)
            .await
            .expect("original candidate socket retired");
    }
}

/// Reuses the library's real bounded Go peer, while a first live UDP endpoint stays silent.
#[tokio::test]
async fn native_raknet_alternative_progresses_past_a_silent_first_endpoint() {
    let Some(binary) = std::env::var_os("BEDROCK_NET_RAKNET_PEER") else {
        eprintln!("missing fixture: BEDROCK_NET_RAKNET_PEER; build bedrock-net tools/raknet-peer");
        return;
    };
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut child = Command::new(binary)
            .args(["-close-after=0", "-ipv6=false"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("start existing loopback RakNet fixture");
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let peer: SocketAddr = lines.next_line().await.unwrap().unwrap().parse().unwrap();
        assert!(peer.ip().is_loopback());
        let blackhole = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let deadline = Instant::now() + crate::NATIVE_JOIN_TIMEOUT;
        let resolved = tokio::net::lookup_host(("localhost", peer.port()))
            .await
            .unwrap();
        let addresses = std::iter::once(blackhole.local_addr().unwrap()).chain(resolved);
        let mut connected = connect_resolved(NativeTransport::RakNet, addresses, None, deadline)
            .await
            .unwrap();
        assert_eq!(connected.header(), BatchHeader::RakNet);
        let message = Bytes::from_static(b"\xfefallback");
        connected.send_until(message.clone(), None).await.unwrap();
        assert_eq!(connected.recv().await.unwrap(), message);
        connected.close().await.unwrap();
        assert!(child.wait().await.unwrap().success());
    })
    .await
    .expect("finite alternative RakNet fixture stalled");
}

/// An empty compatible set cannot start LAN discovery or manufacture a transport owner.
#[tokio::test]
async fn native_lan_resolution_rejects_an_incompatible_candidate_set() {
    let addresses = [SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 19132)];
    let error = connect_resolved(
        NativeTransport::NetherNetLan,
        addresses.into_iter(),
        None,
        Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::AddrNotAvailable);
}
