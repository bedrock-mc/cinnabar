//! Receives web archive delivery at the same handoff boundary as native sessions.

use bedrock_packs::ResourcePackHandoff;
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use bridge::{
    BridgeError, FrameRead,
    tasks::{Timer, Timers},
};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use tokio::io::AsyncRead;

use super::{
    JoinPreparation, LoginSequence, PlaySession, SessionIo, SessionLogin,
    session_join::{bridge_error, join_ended, next_message, send_connect},
};
use crate::{
    GameData, ProtocolError,
    session_transport::MAX_READ_AHEAD_BYTES,
    session_wire::{
        CoreMessage, HandoffPackReceiver, SessionHandoff, batch_frame_body, decode_core_message,
    },
};

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
        let (handoff, archives, reader) =
            receive_handoff(io.reader, join, packs, io.timers).await?;
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
    /// Ready archives complete before further play is buffered. A full byte budget still accepts
    /// terminal messages and rejects further gameplay frames. The host's timer bounds the wait
    /// for a terminal message after an archive failure.
    pub async fn receive_web_handoff<S, P>(
        reader: S,
        join: u64,
        packs: impl Future<Output = Result<Option<P>, ProtocolError>>,
        timers: Timers,
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
        receive_handoff(reader, join, packs, timers).await
    }
}

/// Verifies web archives and retains raw intervening play frames for the shared session reader.
async fn receive_handoff<S, P>(
    mut reader: S,
    join: u64,
    packs: impl Future<Output = Result<Option<P>, ProtocolError>>,
    timers: Timers,
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
        let mut archive_error = None;
        let mut expires: Timer = Box::pin(futures::future::pending());
        loop {
            tokio::select! {
                biased;
                () = &mut expires => return Err(archive_error.expect("archive failure started the deadline")),
                result = &mut receiving, if archive_error.is_none() => {
                    match result {
                        Ok(()) => break,
                        Err(error) => {
                            archive_error = Some(error);
                            expires = timers.sleep(super::WRITE_FAILURE_DRAIN);
                        }
                    }
                }
                frame = reader.next() => {
                    let frame = match frame {
                        Some(Ok(frame)) => frame,
                        Some(Err(error)) => return Err(archive_error.unwrap_or_else(|| bridge_error(error))),
                        None => return Err(archive_error.unwrap_or_else(|| invalid("play closed during pack delivery"))),
                    };
                    let full = bytes >= MAX_READ_AHEAD_BYTES;
                    if batch_frame_body(&frame).is_some() {
                        if full {
                            return Err(ProtocolError::HandoffReadAheadOverflow);
                        }
                        bytes += frame.len();
                        pending.push_back(frame);
                    } else {
                        let message = match decode_core_message(frame) {
                            Ok(message) => message,
                            Err(error) => return Err(archive_error.unwrap_or_else(|| bridge_error(error))),
                        };
                        if matches!(message, CoreMessage::Disconnect(_) | CoreMessage::Transfer(_)) {
                            return Err(join_ended(message));
                        }
                        if full {
                            return Err(ProtocolError::HandoffReadAheadOverflow);
                        }
                        return Err(archive_error.unwrap_or_else(|| join_ended(message)));
                    }
                }
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
