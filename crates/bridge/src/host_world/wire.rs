//! One existing frame primitive, with early per-kind bounds and retained partial output.

use super::{contract::Kind, invalid};
use crate::{BridgeCodec, BridgeError};
use bytes::{Bytes, BytesMut};
use futures::{Sink, StreamExt, future::poll_fn};
use std::{io, pin::Pin, task::Poll};
use tokio::net::{TcpStream, tcp};
use tokio_util::codec::{Decoder, FramedRead, FramedWrite};

/// A fully bounded process frame; its bytes are never formatted in errors.
pub struct Frame {
    /// Actual generated kind, already checked against its body ceiling.
    pub kind: Kind,
    /// Original bytes without the kind byte.
    pub body: Bytes,
}

struct Codec {
    inner: BridgeCodec,
    header_checked: bool,
}

impl Decoder for Codec {
    type Item = Frame;
    type Error = BridgeError;

    fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Frame>, BridgeError> {
        // Inspect the original prefix before the shared codec consumes it.
        if !self.header_checked {
            if source.len() < 5 {
                return Ok(None);
            }
            let length = u32::from_be_bytes(source[..4].try_into().expect("prefix")) as usize;
            let kind = Kind::parse(source[4]).ok_or_else(invalid)?;
            if length == 0 || length - 1 > kind.maximum() {
                return Err(invalid().into());
            }
            self.header_checked = true;
        }
        Ok(self.inner.decode(source)?.map(|bytes| {
            self.header_checked = false;
            Frame {
                kind: Kind::parse(bytes[0]).expect("validated frame kind"),
                body: bytes.slice(1..),
            }
        }))
    }

    fn decode_eof(&mut self, source: &mut BytesMut) -> Result<Option<Frame>, BridgeError> {
        if let Some(frame) = self.decode(source)? {
            return Ok(Some(frame));
        }
        self.inner.decode_eof(source).map(|_| None)
    }
}

/// Independent framed input; canceling recv retains the same partial decoder.
pub struct Reader(FramedRead<tcp::OwnedReadHalf, Codec>);

impl Reader {
    /// Reads the next original bounded frame, retaining partial input across cancellation.
    pub async fn recv(&mut self) -> io::Result<Option<Frame>> {
        self.0.next().await.transpose().map_err(io::Error::other)
    }
}

/// A unique bounded write slot. Admission and flushing are separate so controls can keep driving.
pub struct Writer {
    framed: FramedWrite<tcp::OwnedWriteHalf, BridgeCodec>,
    pending: Option<Bytes>,
    active: bool,
}

impl Writer {
    /// Reserves the sole output frame synchronously; a second admission never replaces it.
    pub fn admit(&mut self, kind: Kind, body: &[u8]) -> io::Result<()> {
        if self.active || body.len() > kind.maximum() {
            return Err(invalid());
        }
        let mut frame = BytesMut::with_capacity(body.len() + 1);
        frame.extend_from_slice(&[kind as u8]);
        frame.extend_from_slice(body);
        self.pending = Some(frame.freeze());
        self.active = true;
        Ok(())
    }

    /// Finishes the one actual frame. Canceling this wait retains both admission and partial writes.
    pub async fn flush(&mut self) -> io::Result<()> {
        poll_fn(|cx| {
            if self.pending.is_some() {
                match Pin::new(&mut self.framed).poll_ready(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(io::Error::other(error))),
                    Poll::Ready(Ok(())) => {}
                }
                let frame = self.pending.take().expect("one admitted frame");
                if let Err(error) = Pin::new(&mut self.framed).start_send(frame) {
                    return Poll::Ready(Err(io::Error::other(error)));
                }
            }
            match Pin::new(&mut self.framed).poll_flush(cx) {
                Poll::Ready(Ok(())) => {
                    self.active = false;
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(error)) => Poll::Ready(Err(io::Error::other(error))),
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }
}

/// Splits the actual IPC socket, without a task, queue, runtime or transport-auth assertion.
pub fn split(stream: TcpStream) -> (Reader, Writer) {
    let maximum = super::contract::MAX_PACKET_BYTES as usize + 1;
    let (read, write) = stream.into_split();
    (
        Reader(FramedRead::new(
            read,
            Codec {
                inner: BridgeCodec::with_max(maximum),
                header_checked: false,
            },
        )),
        Writer {
            framed: FramedWrite::new(write, BridgeCodec::with_max(maximum)),
            pending: None,
            active: false,
        },
    )
}
