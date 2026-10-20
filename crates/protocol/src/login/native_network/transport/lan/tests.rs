use super::*;
use futures::FutureExt;
use nethernet::{
    identity::SdpIdentity,
    protocol::{
        codec::NetherCodec,
        packet::discovery::{Packets, ResponsePacket, decode, encode},
    },
};
use nethernet_tokio::{NetherServer, ServerData};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::net::UdpSocket;

/// Owns the same real LAN discovery and accepting peer used by selected local joins.
async fn listener(
    offline: bool,
    identity: Option<Arc<dyn SdpIdentity>>,
) -> (NetherServer, SocketAddr) {
    let reservation = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    (listener_at(address, 31, offline, identity).await, address)
}

/// Binds only the original numeric fixture endpoint, retaining the actual library listener.
async fn listener_at(
    address: SocketAddr,
    network_id: u64,
    offline: bool,
    identity: Option<Arc<dyn SdpIdentity>>,
) -> NetherServer {
    let signaling = LanSignaling::with_config(
        network_id,
        address,
        LanConfig {
            discovery_port: address.port(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut data = ServerData::new("Fixture".into(), "World".into());
    data.flag_b = offline;
    data.session_id = "synthetic-nonce".into();
    data.connection_type = LAN_CONNECTION_TYPE;
    signaling.set_server_data(data);
    NetherServer::bind_with(
        signaling,
        ConnectionConfig {
            identity,
            allow_unsigned_answers: true,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn selected_local_lan_skips_another_level_and_keeps_the_actual_selected_ready_owner() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let neighbor = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = neighbor.local_addr().unwrap();
        let mut connecting = Box::pin(LanTransport::connect_local(
            address,
            "World",
            crate::NATIVE_JOIN_TIMEOUT,
            tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
        ));
        let mut datagram = [0; 2048];
        let (len, source) = tokio::select! {
            request = neighbor.recv_from(&mut datagram) => request.unwrap(),
            _ = &mut connecting => panic!("local discovery completed before a response"),
        };
        assert!(matches!(
            decode(&datagram[..len]).unwrap().0,
            Packets::Request(_)
        ));
        let data = ServerData::new("Neighbor".into(), "AnotherWorld".into());
        let mut application = Vec::new();
        data.serialize(&mut application).unwrap();
        let response = encode(&Packets::Response(ResponsePacket::new(application)), 31).unwrap();
        neighbor.send_to(&response, source).await.unwrap();
        drop(neighbor);
        let mut server = listener_at(address, 32, true, None).await;
        let (connected, peer) = tokio::join!(&mut connecting, server.accept());
        let (mut connected, id) = connected.unwrap();
        let mut peer = peer.unwrap();
        assert_eq!(id, 32);
        assert_eq!(connected.nonce(), "synthetic-nonce");
        connected
            .send_until(Bytes::from_static(b"selected process"), None)
            .await
            .unwrap();
        assert_eq!(
            peer.reliable.recv().await.unwrap().unwrap(),
            "selected process"
        );
        connected.close();
        peer.close().session.closed().await;
        server.close().await.unwrap();
    })
    .await
    .expect("finite selected-level local LAN fixture stalled");
}

#[tokio::test]
async fn selected_local_lan_refuses_an_online_only_advertisement() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let (mut server, address) = listener(false, None).await;
        let result = LanTransport::connect_local(
            address,
            "World",
            crate::NATIVE_JOIN_TIMEOUT,
            tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
        )
        .await;
        assert_eq!(
            result.err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(server.accept().now_or_never().is_none());
        server.close().await.unwrap();
    })
    .await
    .expect("finite online-only local LAN fixture stalled");
}

struct MalformedIdentity;

impl SdpIdentity for MalformedIdentity {
    fn augment(&self, sdp: &str, _: SystemTime) -> nethernet::identity::error::Result<String> {
        Ok(format!("{sdp}a=identity:malformed-present-assertion\r\n"))
    }
}

#[tokio::test]
async fn selected_local_lan_does_not_treat_a_malformed_present_identity_as_anonymous() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let (mut server, address) = listener(true, Some(Arc::new(MalformedIdentity))).await;
        let result = LanTransport::connect_local(
            address,
            "World",
            crate::NATIVE_JOIN_TIMEOUT,
            tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
        )
        .await;
        let error = result
            .err()
            .expect("a malformed signed answer became a ready local owner");
        assert!(
            matches!(
                error
                    .get_ref()
                    .and_then(|source| source.downcast_ref::<nethernet_tokio::NetherError>()),
                Some(nethernet_tokio::NetherError::Identity(_))
            ),
            "the actual present identity must fail verification: {error}"
        );
        assert!(server.accept().now_or_never().is_none());
        server.close().await.unwrap();
    })
    .await
    .expect("finite malformed local LAN answer fixture stalled");
}

#[tokio::test]
async fn selected_local_lan_cancellation_and_original_cutoff_retire_the_actual_discovery_socket() {
    for expires in [false, true] {
        let blackhole = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = blackhole.local_addr().unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let mut connecting = Box::pin(LanTransport::connect_local(
            address,
            "World",
            crate::NATIVE_JOIN_TIMEOUT,
            deadline,
        ));
        let mut datagram = [0; 2048];
        let (_, source) = tokio::select! {
            result = blackhole.recv_from(&mut datagram) => result.unwrap(),
            _ = &mut connecting => panic!("a silent loopback peer completed local discovery"),
        };
        if expires {
            tokio::time::pause();
            tokio::time::advance(deadline.saturating_duration_since(tokio::time::Instant::now()))
                .await;
            assert_eq!(
                connecting.await.err().unwrap().kind(),
                io::ErrorKind::TimedOut
            );
        } else {
            drop(connecting);
        }
        tokio::task::spawn_blocking(move || {
            test_time::eventually("original local discovery socket retired", || {
                std::net::UdpSocket::bind(source).is_ok()
            });
        })
        .await
        .unwrap();
    }
}
