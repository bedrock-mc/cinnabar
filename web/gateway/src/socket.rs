//! The Go-compatible bounded binary WebSocket byte stream around the existing socket codec.

use bytes::{Buf, Bytes};
use futures::{Sink, Stream};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_websockets::{Message, WebSocketStream};

pub(super) struct Socket<S> {
    inner: WebSocketStream<S>,
    read: Bytes,
    limit: usize,
    ended: bool,
}

impl<S> Socket<S> {
    /// Retains one selected channel's configured message bound and its actual upgraded stream.
    pub(super) fn new(inner: WebSocketStream<S>, limit: usize) -> Self {
        Self {
            inner,
            read: Bytes::new(),
            limit,
            ended: false,
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for Socket<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            if !this.read.is_empty() {
                let size = output.remaining().min(this.read.len());
                output.put_slice(&this.read[..size]);
                this.read.advance(size);
                return Poll::Ready(Ok(()));
            }
            if this.ended {
                return Poll::Ready(Ok(()));
            }
            let Some(message) = ready!(Pin::new(&mut this.inner).poll_next(cx)) else {
                this.ended = true;
                return Poll::Ready(Ok(()));
            };
            let message = message.map_err(failed)?;
            if message.is_close() {
                this.ended = true;
                return Poll::Ready(Ok(()));
            }
            if message.is_ping() || message.is_pong() {
                continue;
            }
            if !message.is_binary() || message.as_payload().len() > this.limit {
                return Poll::Ready(Err(failed(())));
            }
            this.read = message.into_payload().into();
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for Socket<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if input.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if this.ended {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        ready!(Pin::new(&mut this.inner).poll_ready(cx)).map_err(failed)?;
        let size = input.len().min(this.limit);
        Pin::new(&mut this.inner)
            .start_send(Message::binary(Bytes::copy_from_slice(&input[..size])))
            .map_err(failed)?;
        Poll::Ready(Ok(size))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_flush(cx)
            .map_err(failed)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_close(cx)
            .map_err(failed)
    }
}

/// Avoids including peer frames or transport/certificate details in application diagnostics.
fn failed<T>(_error: T) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "gateway WebSocket stream failed",
    )
}
