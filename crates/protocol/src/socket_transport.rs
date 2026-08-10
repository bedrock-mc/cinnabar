use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use bridge::{BridgeError, FramedStream};
use bytes::Bytes;
use futures::{Sink, Stream};
use jolyne::stream::transport::{Transport, TransportMessage, TransportRecvMessage};

/// Returns the local transport endpoint for a logical socket directory.
#[must_use]
pub fn bridge_endpoint_path(socket_dir: &Path) -> std::path::PathBuf {
    bridge::endpoint_path(socket_dir)
}

/// Jolyne transport over the local length-framed bridge.
pub struct SocketTransport {
    stream: FramedStream,
    send_state: SendState,
    peer_addr: SocketAddr,
}

impl SocketTransport {
    pub(crate) async fn connect(socket_dir: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            stream: bridge::connect(socket_dir).await?,
            send_state: SendState::default(),
            peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        })
    }
}

impl Transport for SocketTransport {
    type Error = BridgeError;

    const USES_BATCH_PREFIX: bool = true;

    fn poll_send(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        message: TransportMessage,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.as_mut().get_mut();
        poll_send_frame(
            Pin::new(&mut this.stream),
            &mut this.send_state,
            cx,
            message.buffer,
        )
    }

    fn poll_drain_send(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.as_mut().get_mut();
        poll_drain_send_state(Pin::new(&mut this.stream), &mut this.send_state, cx)
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
        match Pin::new(&mut self.stream).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                Poll::Ready(Some(Ok(TransportRecvMessage::Contiguous(bytes))))
            }
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(error))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }
}

#[derive(Debug)]
struct PendingSend {
    buffer: Bytes,
    started: bool,
}

#[derive(Debug, Default)]
struct SendState {
    active: Option<PendingSend>,
    queued: VecDeque<Bytes>,
}

fn same_buffer(left: &Bytes, right: &Bytes) -> bool {
    left.len() == right.len() && left.as_ptr() == right.as_ptr()
}

fn poll_send_frame<S>(
    mut stream: Pin<&mut S>,
    state: &mut SendState,
    cx: &mut Context<'_>,
    buffer: Bytes,
) -> Poll<Result<(), S::Error>>
where
    S: Sink<Bytes> + Unpin,
{
    let already_retained = state
        .active
        .as_ref()
        .is_some_and(|pending| same_buffer(&pending.buffer, &buffer))
        || state
            .queued
            .iter()
            .any(|queued| same_buffer(queued, &buffer));
    if !already_retained {
        if state.active.is_none() {
            state.active = Some(PendingSend {
                buffer: buffer.clone(),
                started: false,
            });
        } else {
            state.queued.push_back(buffer.clone());
        }
    }

    loop {
        let pending = state
            .active
            .as_mut()
            .expect("the current send is retained before polling");
        if !pending.started {
            match stream.as_mut().poll_ready(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(error)) => {
                    state.active = None;
                    state.queued.clear();
                    return Poll::Ready(Err(error));
                }
                Poll::Pending => return Poll::Pending,
            }
            if let Err(error) = stream.as_mut().start_send(pending.buffer.clone()) {
                state.active = None;
                state.queued.clear();
                return Poll::Ready(Err(error));
            }
            pending.started = true;
        }

        match stream.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                let completed = state.active.take().expect("a flushed send remains active");
                let completed_current = same_buffer(&completed.buffer, &buffer);
                state.active = state.queued.pop_front().map(|buffer| PendingSend {
                    buffer,
                    started: false,
                });
                if completed_current {
                    return Poll::Ready(Ok(()));
                }
            }
            Poll::Ready(Err(error)) => {
                state.active = None;
                state.queued.clear();
                return Poll::Ready(Err(error));
            }
            Poll::Pending => return Poll::Pending,
        }
    }
}

fn poll_drain_send_state<S>(
    mut stream: Pin<&mut S>,
    state: &mut SendState,
    cx: &mut Context<'_>,
) -> Poll<Result<(), S::Error>>
where
    S: Sink<Bytes> + Unpin,
{
    loop {
        let Some(pending) = state.active.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        if !pending.started {
            match stream.as_mut().poll_ready(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(error)) => {
                    state.active = None;
                    state.queued.clear();
                    return Poll::Ready(Err(error));
                }
                Poll::Pending => return Poll::Pending,
            }
            if let Err(error) = stream.as_mut().start_send(pending.buffer.clone()) {
                state.active = None;
                state.queued.clear();
                return Poll::Ready(Err(error));
            }
            pending.started = true;
        }

        match stream.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                state.active = state.queued.pop_front().map(|buffer| PendingSend {
                    buffer,
                    started: false,
                });
            }
            Poll::Ready(Err(error)) => {
                state.active = None;
                state.queued.clear();
                return Poll::Ready(Err(error));
            }
            Poll::Pending => return Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::task::noop_waker;

    #[derive(Default)]
    struct PendingFlushSink {
        starts: usize,
        flushes: usize,
    }

    impl Sink<Bytes> for PendingFlushSink {
        type Error = BridgeError;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(mut self: Pin<&mut Self>, _item: Bytes) -> Result<(), Self::Error> {
            self.starts += 1;
            Ok(())
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            self.flushes += 1;
            if self.flushes == 1 {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            }
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[derive(Default)]
    struct PendingReadySink {
        ready_polls: usize,
        starts: usize,
    }

    impl Sink<Bytes> for PendingReadySink {
        type Error = BridgeError;

        fn poll_ready(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            self.ready_polls += 1;
            if self.ready_polls == 1 {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            }
        }

        fn start_send(mut self: Pin<&mut Self>, _item: Bytes) -> Result<(), Self::Error> {
            self.starts += 1;
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn pending_flush_does_not_start_the_same_frame_twice() {
        let mut sink = PendingFlushSink::default();
        let mut pending = SendState::default();
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let bytes = Bytes::from_static(b"frame");

        assert!(
            poll_send_frame(Pin::new(&mut sink), &mut pending, &mut cx, bytes.clone(),)
                .is_pending()
        );
        assert!(poll_send_frame(Pin::new(&mut sink), &mut pending, &mut cx, bytes).is_ready());
        assert_eq!(sink.starts, 1);
    }

    #[test]
    fn cancelled_pending_send_can_be_drained_without_a_replacement_frame() {
        let mut sink = PendingFlushSink::default();
        let mut pending = SendState::default();
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        assert!(
            poll_send_frame(
                Pin::new(&mut sink),
                &mut pending,
                &mut cx,
                Bytes::from_static(b"cancelled"),
            )
            .is_pending()
        );
        assert!(poll_drain_send_state(Pin::new(&mut sink), &mut pending, &mut cx).is_ready());
        assert_eq!(sink.starts, 1);
        assert!(pending.active.is_none());
        assert!(pending.queued.is_empty());
    }

    #[test]
    fn cancelled_pending_send_flushes_before_starting_the_replacement_frame() {
        let mut sink = PendingFlushSink::default();
        let mut pending = SendState::default();
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        assert!(
            poll_send_frame(
                Pin::new(&mut sink),
                &mut pending,
                &mut cx,
                Bytes::from_static(b"cancelled"),
            )
            .is_pending()
        );
        assert!(
            poll_send_frame(
                Pin::new(&mut sink),
                &mut pending,
                &mut cx,
                Bytes::from_static(b"replacement"),
            )
            .is_ready()
        );
        assert_eq!(sink.starts, 2);
        assert!(pending.active.is_none());
        assert!(pending.queued.is_empty());
    }

    #[test]
    fn cancelled_send_waiting_for_readiness_is_retained_before_its_replacement() {
        let mut sink = PendingReadySink::default();
        let mut pending = SendState::default();
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        assert!(
            poll_send_frame(
                Pin::new(&mut sink),
                &mut pending,
                &mut cx,
                Bytes::from_static(b"cancelled-before-start"),
            )
            .is_pending()
        );
        assert!(
            poll_send_frame(
                Pin::new(&mut sink),
                &mut pending,
                &mut cx,
                Bytes::from_static(b"replacement"),
            )
            .is_ready()
        );
        assert_eq!(sink.starts, 2);
        assert!(pending.active.is_none());
        assert!(pending.queued.is_empty());
    }
}
