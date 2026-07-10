use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use bridge::{BridgeError, FramedStream};
use bytes::Bytes;
use futures::{Sink, Stream};
use jolyne::stream::transport::{Transport, TransportMessage, TransportRecvMessage};

/// Jolyne transport over the local length-framed bridge.
pub struct SocketTransport {
    stream: FramedStream,
    send_pending: bool,
    peer_addr: SocketAddr,
}

impl SocketTransport {
    pub(crate) async fn connect(socket_dir: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            stream: bridge::connect(socket_dir).await?,
            send_pending: false,
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
            &mut this.send_pending,
            cx,
            message.buffer,
        )
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

fn poll_send_frame<S>(
    mut stream: Pin<&mut S>,
    send_pending: &mut bool,
    cx: &mut Context<'_>,
    buffer: Bytes,
) -> Poll<Result<(), S::Error>>
where
    S: Sink<Bytes> + Unpin,
{
    if !*send_pending {
        match stream.as_mut().poll_ready(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Pending => return Poll::Pending,
        }
        if let Err(error) = stream.as_mut().start_send(buffer) {
            return Poll::Ready(Err(error));
        }
        *send_pending = true;
    }

    match stream.poll_flush(cx) {
        Poll::Ready(result) => {
            *send_pending = false;
            Poll::Ready(result)
        }
        Poll::Pending => Poll::Pending,
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

    #[test]
    fn pending_flush_does_not_start_the_same_frame_twice() {
        let mut sink = PendingFlushSink::default();
        let mut pending = false;
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
}
