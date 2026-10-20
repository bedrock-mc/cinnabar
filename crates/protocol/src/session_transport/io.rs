//! Packet I/O shared by core handoffs and native connections.

use super::*;

pub(super) enum ReadItem {
    Core(Bytes),
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(Bytes),
}

impl ReadItem {
    /// Charges every byte retained by read-ahead.
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Core(bytes) => bytes.len(),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(bytes) => bytes.len(),
        }
    }

    /// Whether the item contains play packets rather than a terminal marker.
    pub(super) fn is_batch(&self) -> bool {
        match self {
            Self::Core(frame) => crate::session_wire::batch_frame_body(frame).is_some(),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(body) => {
                super::native_batch_terminal(body).is_ok_and(|terminal| terminal.is_none())
            }
        }
    }
}

pub(super) enum Reader<R> {
    Core(R),
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(super::native::NativeReader),
}

impl<R: bridge::FrameRead> Reader<R> {
    /// Polls the actual backend; native batches never pass through IPC encoding.
    pub(super) fn poll_next(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<ReadItem, SessionError>>> {
        match self {
            Self::Core(reader) => Pin::new(reader).poll_next(cx).map(|item| {
                item.map(|result| {
                    result
                        .map(ReadItem::Core)
                        .map_err(|error| SessionFailure::Transport(error).into())
                })
            }),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(reader) => reader.poll_next(cx).map(|item| {
                item.map(|result| {
                    result
                        .map(ReadItem::Native)
                        .map_err(|error| SessionFailure::NativeTransport(error).into())
                })
            }),
        }
    }

    /// Returns the core's trace identity when this session has one.
    pub(super) fn trace_conn(&self) -> u64 {
        match self {
            Self::Core(reader) => reader.trace_conn(),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(_) => 0,
        }
    }
}

/// Detached access to the connection's ordered packet FIFO.
#[derive(Clone)]
pub(crate) enum PacketSender {
    Core(FrameQueue),
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(super::native::NativeSender),
}

/// Completion of one accepted batch, retained independently of the caller's wait.
pub(crate) enum PacketWritten {
    Core(bridge::FrameWritten),
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(tokio::sync::oneshot::Receiver<std::io::Result<()>>),
}

impl Future for PacketWritten {
    type Output = Result<(), ProtocolError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut *self {
            Self::Core(written) => Pin::new(written)
                .poll(cx)
                .map(|result| result.map_err(|error| ProtocolError::Bridge(error.into()))),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(written) => {
                let result = ready!(Pin::new(written).poll(cx)).unwrap_or_else(|_| {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "native connection stopped before sending batch",
                    ))
                });
                Poll::Ready(result.map_err(|error| SessionFailure::NativeTransport(error).into()))
            }
        }
    }
}

impl PacketSender {
    /// Reserves capacity before committing one batch; never suspends after commitment.
    pub(crate) async fn accept(&self, body: Bytes) -> Result<PacketWritten, ProtocolError> {
        match self {
            Self::Core(frames) => {
                let mut frame = BytesMut::with_capacity(body.len() + 1);
                frame.put_u8(bridge::contract::KIND_BATCH);
                frame.extend_from_slice(&body);
                frames
                    .accept(frame.freeze())
                    .await
                    .map(PacketWritten::Core)
                    .map_err(|error| ProtocolError::Bridge(error.into()))
            }
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(sender) => sender
                .accept(body)
                .await
                .map(PacketWritten::Native)
                .map_err(|error| SessionFailure::NativeTransport(error).into()),
        }
    }

    /// Waits for admission and the accepted batch's actual write completion.
    #[cfg(test)]
    pub(crate) async fn send(&self, body: Bytes) -> Result<(), ProtocolError> {
        self.accept(body).await?.await
    }
}
