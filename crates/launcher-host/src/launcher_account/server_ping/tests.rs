use super::*;
use raknet::prelude::{RakServer, RakServerConfig, RakServerInput, RakServerOutput, Sans};
use std::{
    future::Future,
    task::{Context, Waker},
};

/// Answers one actual unconnected ping through the public server core and a real UDP socket.
async fn answer(socket: &UdpSocket) {
    let now = Instant::now().into_std();
    let mut server = RakServer::new(
        RakServerConfig {
            guid: 7,
            message: b"MCPE;  Native\\ Fixture  ;unused;future;12;-1;id;sub;Unknown;"
                .to_vec()
                .into_boxed_slice(),
            ..Default::default()
        },
        socket.local_addr().unwrap(),
        RakClock::new(now),
        now,
    )
    .unwrap();
    let mut bytes = vec![0; usize::from(u16::MAX)];
    let (length, peer) = socket.recv_from(&mut bytes).await.unwrap();
    server
        .handle(RakServerInput::Datagram(
            bytes[..length].to_vec().into_boxed_slice(),
            peer,
            Instant::now().into_std(),
        ))
        .unwrap();
    let mut replied = false;
    while let Some(output) = server.poll() {
        if let RakServerOutput::SocketDatagram(bytes, peer) = output {
            assert_eq!(socket.send_to(&bytes, peer).await.unwrap(), bytes.len());
            replied = true;
        }
    }
    assert!(replied, "public server did not produce its pong");
}

/// A silent first DNS answer cannot hide another usable peer in the same family.
#[tokio::test]
async fn native_row_ping_uses_actual_public_core_and_all_resolved_peers() {
    for host in ["127.0.0.1:0", "[::1]:0"] {
        let live = match UdpSocket::bind(host).await {
            Ok(socket) => socket,
            Err(error) if host.starts_with('[') => {
                eprintln!(
                    "missing-fixture: IPv6 loopback UDP unavailable ({})",
                    error.kind()
                );
                continue;
            }
            Err(error) => panic!("IPv4 loopback UDP fixture: {error}"),
        };
        let silent = UdpSocket::bind(host).await.unwrap();
        let deadline = Instant::now() + TIMEOUT;
        let attempt = resolved(
            [silent.local_addr().unwrap(), live.local_addr().unwrap()].into_iter(),
            deadline,
        );
        let result = tokio::time::timeout_at(deadline + TIMEOUT, async {
            let (result, ()) = tokio::join!(attempt, answer(&live));
            result
        })
        .await
        .expect("owning ping/peer finish under their original finite deadlines")
        .unwrap();
        assert_eq!(result.name.trim(), "Native Fixture");
        assert_eq!(result.players, 12);
        assert_eq!(result.max_players, -1);
    }
}

/// Cancellation drops the actual bound client socket rather than leaving a background ping actor.
#[tokio::test]
async fn canceling_native_row_ping_releases_its_actual_udp_socket() {
    let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let deadline = Instant::now() + TIMEOUT;
    let mut attempt = Box::pin(family(vec![silent.local_addr().unwrap()], deadline));
    let mut bytes = [0; 128];
    let peer = tokio::select! {
        result = &mut attempt => panic!("silent peer unexpectedly completed: {result:?}"),
        result = silent.recv_from(&mut bytes) => result.unwrap().1,
    };
    assert!(
        attempt
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(attempt);
    let rebound = UdpSocket::bind(peer)
        .await
        .expect("canceled ping retained its UDP socket");
    assert_eq!(rebound.local_addr().unwrap(), peer);
}

/// A canceled borrowed wait never creates a fresh deadline for the retained actual attempt.
#[tokio::test(start_paused = true)]
async fn native_row_ping_observes_the_original_attempt_cutoff() {
    let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let deadline = Instant::now() + TIMEOUT;
    let mut attempt = Box::pin(family(vec![silent.local_addr().unwrap()], deadline));
    assert!(
        attempt
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    tokio::time::advance(TIMEOUT).await;
    let result = tokio::time::timeout_at(deadline + TIMEOUT, &mut attempt)
        .await
        .expect("original pending ping completes after its timer is driven");
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
}
