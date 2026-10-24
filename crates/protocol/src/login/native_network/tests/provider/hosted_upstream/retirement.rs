//! Physical completion of the original upstream after its game input has been consumed.

use super::*;
use std::{io, net::UdpSocket, task::Poll};

pub(super) enum Mode {
    None,
    Upstream,
    Relay(tokio::sync::oneshot::Sender<RelayObservation>),
}

#[derive(Debug)]
pub(super) struct RelayObservation {
    initially_pending: bool,
    sockets_held_on_first_poll: [bool; 2],
    finish: Option<io::ErrorKind>,
    repeated_finish: Option<io::ErrorKind>,
    sockets_released_at_completion: [bool; 2],
    cleanup: [Option<io::ErrorKind>; 2],
    sockets_released_after_cleanup: [bool; 2],
}

/// Observes the same two-leg closing owner before rescuing the request-only baseline for cleanup.
pub(super) async fn observe_relay<A>(
    relay: crate::login::native_network::NativeHostedRelay<A>,
    deadline: Instant,
    addresses: [std::net::SocketAddr; 2],
) -> (
    bedrock_client_tokio::server::AcceptingConnection,
    RelayObservation,
) {
    let mut closing = relay.begin_close(deadline);
    let (initially_pending, sockets_held_on_first_poll, finish) = {
        let mut finish = std::pin::pin!(closing.finish_close());
        let first = futures::poll!(finish.as_mut());
        // Neither original actor can run between this first poll and these socket probes.
        let held = addresses.map(|address| {
            UdpSocket::bind(address).is_err_and(|error| error.kind() == io::ErrorKind::AddrInUse)
        });
        let (pending, result) = match first {
            Poll::Pending => (true, finish.await),
            Poll::Ready(result) => (false, result),
        };
        (pending, held, result.err().map(|error| error.kind()))
    };
    let sockets_released_at_completion = addresses.map(|address| UdpSocket::bind(address).is_ok());
    let repeated_finish = closing.finish_close().await.err().map(|error| error.kind());
    let (wire, cleanup) =
        crate::login::native_network::hosted::drain_original_relay_inputs(closing).await;
    let sockets_released_after_cleanup = addresses.map(|address| UdpSocket::bind(address).is_ok());
    (
        wire,
        RelayObservation {
            initially_pending,
            sockets_held_on_first_poll,
            finish,
            repeated_finish,
            sockets_released_at_completion,
            cleanup,
            sockets_released_after_cleanup,
        },
    )
}

/// Both real startup legs must physically retire before the same public finish returns.
#[tokio::test]
async fn hosted_relay_finish_close_waits_for_both_original_socket_retirements() {
    let (send, receive) = tokio::sync::oneshot::channel();
    let observed = tokio::time::timeout(TIMEOUT, async {
        let result = original_upstream_with_retirement(
            Route::LocalHttp,
            None,
            archive_bytes(PACK_ID, &[]),
            bridge::contract::RESOURCE_PACK_TYPE_RESOURCES,
            Mode::Relay(send),
        )
        .await;
        assert!(result.is_none());
        receive.await.unwrap()
    })
    .await
    .expect("finite original two-leg retirement fixture stalled");
    assert_eq!(observed.finish, None, "{observed:?}");
    assert_eq!(observed.repeated_finish, observed.finish, "{observed:?}");
    assert_eq!(observed.cleanup, [None; 2], "{observed:?}");
    assert_eq!(
        observed.sockets_held_on_first_poll, [true; 2],
        "{observed:?}"
    );
    assert_eq!(
        observed.sockets_released_after_cleanup, [true; 2],
        "{observed:?}"
    );
    assert!(observed.initially_pending, "{observed:?}");
    assert_eq!(
        observed.sockets_released_at_completion, [true; 2],
        "finish_close returned before both original sockets retired: {observed:?}"
    );
}

#[derive(Debug)]
pub(super) struct Observation {
    initially_pending: bool,
    socket_held_on_first_poll: bool,
    finish: Option<io::ErrorKind>,
    socket_released_at_completion: bool,
    trailing_packets: usize,
    terminal: io::ErrorKind,
    socket_released_after_drop: bool,
}

/// Keeps the same closing owner alive while observing its actual socket's lifetime.
pub(super) async fn observe(
    upstream: NativeHostedUpstream,
    deadline: Instant,
    address: std::net::SocketAddr,
) -> Observation {
    let mut closing = upstream.begin_close(deadline);
    let (initially_pending, socket_held_on_first_poll, finish) = {
        let mut finish = std::pin::pin!(closing.finish_close());
        let first = futures::poll!(finish.as_mut());
        // Current-thread execution has not yielded since the first close poll.
        let socket_held =
            UdpSocket::bind(address).is_err_and(|error| error.kind() == io::ErrorKind::AddrInUse);
        let (pending, result) = match first {
            Poll::Pending => (true, finish.await),
            Poll::Ready(result) => (false, result),
        };
        (pending, socket_held, result.err().map(|error| error.kind()))
    };
    let socket_released_at_completion = UdpSocket::bind(address).is_ok();
    // The request-only baseline still owns the socket here. Finish its original input
    // before checking the oracle, without treating this locally requested EOF as peer EOF.
    let mut trailing_packets = 0;
    let terminal = loop {
        match closing.receive().await {
            Ok(_) => trailing_packets += 1,
            Err(error) => break error.kind(),
        }
    };
    drop(closing);
    let socket_released_after_drop = UdpSocket::bind(address).is_ok();
    Observation {
        initially_pending,
        socket_held_on_first_poll,
        finish,
        socket_released_at_completion,
        trailing_packets,
        terminal,
        socket_released_after_drop,
    }
}

/// The real initialized upstream must retire physically before final close returns.
#[tokio::test]
async fn hosted_upstream_finish_close_waits_for_its_original_socket_retirement() {
    let observed = tokio::time::timeout(
        TIMEOUT,
        original_upstream_with_retirement(
            Route::LocalHttp,
            None,
            archive_bytes(PACK_ID, &[]),
            bridge::contract::RESOURCE_PACK_TYPE_RESOURCES,
            Mode::Upstream,
        ),
    )
    .await
    .expect("finite original upstream retirement fixture stalled")
    .unwrap();
    assert_eq!(observed.finish, None, "{observed:?}");
    assert!(observed.socket_held_on_first_poll, "{observed:?}");
    assert_eq!(observed.trailing_packets, 0, "{observed:?}");
    assert_eq!(
        observed.terminal,
        io::ErrorKind::UnexpectedEof,
        "{observed:?}"
    );
    assert!(observed.socket_released_after_drop, "{observed:?}");
    assert!(
        observed.initially_pending && observed.socket_released_at_completion,
        "finish_close returned before its original upstream socket retired: {observed:?}"
    );
}
