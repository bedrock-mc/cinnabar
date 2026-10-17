//! Receives web archive delivery at the same handoff boundary as native sessions.

use std::future::Future;

use bridge::BridgeError;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use tokio::io::AsyncRead;

use super::{
    LoginSequence,
    session_join::{bridge_error, join_ended, next_message},
};
use crate::{
    ProtocolError,
    session_wire::{
        CoreMessage, HandoffPackReceiver, SessionHandoff, batch_frame_body, decode_core_message,
    },
};

/// Bounds tiny gameplay frames retained while a bulk archive stream is pending.
const MAX_READ_AHEAD_FRAMES: usize = 16;

impl LoginSequence {
    /// Receives one web join's handoff and verifies every archive before startup. `packs` owns
    /// the independent stream, or returns None for WebSocket. Play remains readable for terminal
    /// messages while bulk data arrives. The returned reader preserves bounded intervening batches
    /// and feeds the existing login transport; dropping this future drops its archive stream.
    pub async fn receive_web_handoff<S, P>(
        mut reader: S,
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
        let handoff = match next_message(&mut reader).await? {
            CoreMessage::Handoff(handoff) => handoff,
            other => return Err(join_ended(other)),
        };
        if join == 0 || handoff.pack_stream.is_some_and(|expected| expected != join) {
            return Err(invalid("handoff pack join mismatch"));
        }
        let mut receiver = HandoffPackReceiver::web(&handoff).map_err(bridge_error)?;
        let mut pending = Vec::new();
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
                            if frame.len() > crate::session_transport::MAX_READ_AHEAD_BYTES - bytes {
                                return Err(invalid("pack handoff play read-ahead limit"));
                            }
                            bytes += frame.len();
                            pending.push(Ok(frame));
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
        Ok((
            handoff,
            archives,
            futures::stream::iter(pending).chain(reader),
        ))
    }
}

/// Builds a bounded public error for a malformed handoff.
fn invalid(reason: &'static str) -> ProtocolError {
    bridge_error(BridgeError::InvalidSessionMessage { reason })
}
