//! Receives web archive delivery at the same handoff boundary as native sessions.

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use bridge::{BridgeError, FrameRead};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use tokio::io::AsyncRead;

use super::{
    JoinPreparation, LoginSequence, PlaySession, SessionIo, SessionLogin,
    session_join::{bridge_error, join_ended, next_message, send_connect},
};
use crate::{
    GameData, ProtocolError, ResourcePackHandoff,
    session_wire::{
        CoreMessage, HandoffPackReceiver, SessionHandoff, batch_frame_body, decode_core_message,
    },
};

/// Bounds tiny gameplay frames retained while a bulk archive stream is pending.
const MAX_READ_AHEAD_FRAMES: usize = 16;

impl LoginSequence {
    /// Joins injected web play I/O after verifying its archives. The host drives the writer,
    /// timers and startup jobs, and cancels them when the join ends or this future is dropped.
    pub async fn connect_web_prepared<R: FrameRead, P: Send + 'static, A: AsyncRead + Unpin>(
        io: SessionIo<R>,
        login: SessionLogin<'_>,
        join: u64,
        packs: impl Future<Output = Result<Option<A>, ProtocolError>>,
        preparation: &impl JoinPreparation,
        prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
    ) -> Result<(PlaySession<WebSessionReader<R>>, P), ProtocolError> {
        send_connect(&io.frames, &login).await?;
        let (handoff, archives, reader) = receive_handoff(io.reader, join, packs).await?;
        Self::continue_handoff(
            SessionIo {
                reader,
                frames: io.frames,
                timers: io.timers,
            },
            login,
            preparation,
            prepare,
            handoff,
            archives,
        )
        .await
    }

    /// Receives one web join's handoff and verifies every archive before startup. `packs` owns
    /// the independent stream, or returns None for WebSocket. Play remains readable for terminal
    /// messages while bulk data arrives. The returned reader preserves bounded intervening batches
    /// and feeds the existing login transport; dropping this future drops its archive stream.
    pub async fn receive_web_handoff<S, P>(
        reader: S,
        join: u64,
        packs: impl Future<Output = Result<Option<P>, ProtocolError>>,
    ) -> Result<
        (
            SessionHandoff,
            Vec<Vec<u8>>,
            impl Stream<Item = Result<Bytes, BridgeError>> + Unpin,
        ),
        ProtocolError,
    >
    where
        S: Stream<Item = Result<Bytes, BridgeError>> + Unpin,
        P: AsyncRead + Unpin,
    {
        receive_handoff(reader, join, packs).await
    }
}

/// Verifies web archives and retains raw intervening play frames for the shared session reader.
async fn receive_handoff<S, P>(
    mut reader: S,
    join: u64,
    packs: impl Future<Output = Result<Option<P>, ProtocolError>>,
) -> Result<(SessionHandoff, Vec<Vec<u8>>, WebSessionReader<S>), ProtocolError>
where
    S: Stream<Item = Result<Bytes, BridgeError>> + Unpin,
    P: AsyncRead + Unpin,
{
    let handoff = match next_message(&mut reader).await? {
        CoreMessage::Handoff(handoff) => handoff,
        other => return Err(join_ended(other)),
    };
    if join == 0 || handoff.pack_stream.is_some_and(|expected| expected != join) {
        return Err(invalid("handoff pack join mismatch"));
    }
    let mut receiver = HandoffPackReceiver::web(&handoff).map_err(bridge_error)?;
    let mut pending = VecDeque::new();
    if handoff.pack_stream.is_some() {
        let receiving = async {
            let stream = packs.await?.ok_or_else(|| invalid("missing pack stream"))?;
            receiver
                .receive_stream(join, stream)
                .await
                .map_err(bridge_error)
        };
        tokio::pin!(receiving);
        let mut bytes = 0;
        loop {
            tokio::select! {
                biased;
                frame = reader.next(), if pending.len() < MAX_READ_AHEAD_FRAMES && bytes < crate::session_transport::MAX_READ_AHEAD_BYTES => {
                    let frame = frame.ok_or_else(|| invalid("play closed during pack delivery"))?.map_err(bridge_error)?;
                    if batch_frame_body(&frame).is_some() {
                        bytes += frame.len();
                        pending.push_back(frame);
                    } else {
                        return Err(join_ended(decode_core_message(frame).map_err(bridge_error)?));
                    }
                }
                result = &mut receiving => { result?; break; }
            }
        }
    } else {
        if packs.await?.is_some() {
            return Err(invalid("unexpected pack stream"));
        }
        while !receiver.is_complete() {
            match next_message(&mut reader).await? {
                CoreMessage::PackData { index, data } => {
                    receiver.accept(index, &data).map_err(bridge_error)?
                }
                other => return Err(join_ended(other)),
            }
        }
    }
    let archives = receiver.into_archives().map_err(bridge_error)?;
    Ok((handoff, archives, WebSessionReader { reader, pending }))
}

/// Replays bounded play read-ahead without losing the injected reader's trace identity.
pub struct WebSessionReader<S> {
    reader: S,
    pending: VecDeque<Bytes>,
}

impl<S: Stream<Item = Result<Bytes, BridgeError>> + Unpin> Stream for WebSessionReader<S> {
    type Item = Result<Bytes, BridgeError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(frame) = self.pending.pop_front() {
            Poll::Ready(Some(Ok(frame)))
        } else {
            Pin::new(&mut self.reader).poll_next(cx)
        }
    }
}

impl<S: FrameRead> FrameRead for WebSessionReader<S> {
    fn trace_conn(&self) -> u64 {
        self.reader.trace_conn()
    }

    fn set_trace_conn(&self, id: u64) {
        self.reader.set_trace_conn(id);
    }
}

/// Builds a bounded public error for a malformed handoff.
fn invalid(reason: &'static str) -> ProtocolError {
    bridge_error(BridgeError::InvalidSessionMessage { reason })
}
