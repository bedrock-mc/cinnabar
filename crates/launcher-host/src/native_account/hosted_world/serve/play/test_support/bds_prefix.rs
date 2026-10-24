//! Exercises only an actual initialized BDS peer's original closing continuation.

use super::super::*;
use bytes::Bytes;
use std::net::{SocketAddr, UdpSocket};
use tokio::sync::oneshot;

#[derive(Debug)]
pub(in crate::native_account::hosted_world) struct BdsPrefixObservation {
    pub received: [Bytes; 2],
    pub forwarding: bool,
    pub first_finish: Option<io::ErrorKind>,
    pub socket_held_before_forwarding: bool,
    pub closed: Option<io::ErrorKind>,
    pub finished: bool,
    pub socket_released_at_completion: bool,
}

/// Retains the actual accepted pair, original output obligation and backend until final finish.
pub(in crate::native_account::hosted_world) async fn close_bds_prefix(
    mut peer: NativeHostedPeer,
    host: &mut NativeHostedWorld,
    ready: oneshot::Receiver<SocketAddr>,
    marker_received: oneshot::Receiver<()>,
    start: oneshot::Sender<()>,
    deadline: Instant,
) -> io::Result<BdsPrefixObservation> {
    enum Progress {
        Ready(SocketAddr),
        Step(Step),
    }
    let ready = async {
        let address = ready.await.map_err(|_| unavailable())?;
        marker_received.await.map_err(|_| unavailable())?;
        Ok::<_, io::Error>(address)
    };
    let mut ready = std::pin::pin!(ready);
    let address = loop {
        host.wait_approval(peer.approval(), Some(deadline)).await?;
        let progress = host
            .peer_io(Some(deadline), async {
                tokio::select! { biased;
                    address = &mut ready => address.map(Progress::Ready),
                    step = peer.next_step() => step.map(Progress::Step),
                }
            })
            .await?;
        host.wait_approval(peer.approval(), Some(deadline)).await?;
        match progress {
            Progress::Ready(address) => break address,
            Progress::Step(step) => {
                if peer.apply(step)? {
                    return Err(unavailable());
                }
            }
        }
    };
    // Finish only already admitted effects before the fixture releases its two actual batches.
    while peer.output.deadline().is_some() || peer.backend_deadline.is_some() {
        advance(&mut peer, host, deadline).await?;
    }
    start.send(()).map_err(|_| unavailable())?;
    let PeerBackend::Bds { upstream, .. } = &mut peer.backend else {
        return Err(unavailable());
    };
    let received = host
        .peer_io(
            Some(deadline),
            protocol::test_support::retain_hosted_received_pair(upstream),
        )
        .await?;
    host.wait_approval(peer.approval(), Some(deadline)).await?;
    // The normal step stages the first original packet; its sibling stays on the same source FIFO.
    advance(&mut peer, host, deadline).await?;
    let forwarding = matches!(peer.output, Output::Forwarding(_));
    let mut closing = peer.begin_close(deadline);
    let first_finish = closing
        .backend
        .as_mut()
        .ok_or_else(unavailable)?
        .finish_close()
        .await
        .err()
        .map(|error| error.kind());
    let socket_held_before_forwarding =
        UdpSocket::bind(address).is_err_and(|error| error.kind() == io::ErrorKind::AddrInUse);
    let closed = closing
        .finish_close(host)
        .await
        .err()
        .map(|error| error.kind());
    let finished = closing.is_finished();
    let socket_released_at_completion = UdpSocket::bind(address).is_ok();
    drop(closing);
    Ok(BdsPrefixObservation {
        received,
        forwarding,
        first_finish,
        socket_held_before_forwarding,
        closed,
        finished,
        socket_released_at_completion,
    })
}

/// Drives one original step with the same pre/post authority and synchronous apply boundary.
async fn advance(
    peer: &mut NativeHostedPeer,
    host: &mut NativeHostedWorld,
    deadline: Instant,
) -> io::Result<()> {
    host.wait_approval(peer.approval(), Some(deadline)).await?;
    let step = host.peer_io(Some(deadline), peer.next_step()).await?;
    host.wait_approval(peer.approval(), Some(deadline)).await?;
    if peer.apply(step)? {
        Err(unavailable())
    } else {
        Ok(())
    }
}

/// Reports only fixture-owner retirement, without copying peer or member identity.
fn unavailable() -> io::Error {
    io::ErrorKind::BrokenPipe.into()
}
