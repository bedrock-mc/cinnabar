use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll, ready};

use crate::packet_relay::{BatchWriter, PreparedBatch};
use bedrock_protocol_1_26_51::{packets::PacketId, wire};
use bridge::{BridgeError, FrameWritten};
use bytes::Bytes;
use simulation_clock::Moment;

use super::PlaySession;
use super::cache_status::SharedCacheStatus;
use crate::{Packet, ProtocolError};

/// The write half of a play session: frames whole batches without borrowing the session.
pub struct PlayOutbound {
    writer: BatchWriter,
    finish_loading: Vec<Packet>,
    cache_status: SharedCacheStatus,
    pending: VecDeque<PendingBatch>,
}

#[cfg(test)]
mod tests;

type PrepareFrame = Pin<Box<dyn Future<Output = Option<PreparedBatch>> + Send>>;
type AcceptFrame = Pin<Box<dyn Future<Output = Result<FrameWritten, BridgeError>> + Send>>;

enum BatchState {
    Queued {
        packets: Vec<Packet>,
        opportunity: Option<Moment>,
    },
    Preparing(Mutex<PrepareFrame>),
    Prepared {
        batch: PreparedBatch,
        accepting: Option<Mutex<AcceptFrame>>,
    },
    Written(FrameWritten),
    Done,
}

/// An entire submitted batch, retained until its exact frame flushes across cancelled waits.
struct PendingBatch {
    state: BatchState,
    status_epoch: Option<u64>,
}

impl PendingBatch {
    /// Transfers ordinary packet ownership before the first capacity or flush wait.
    fn new(packets: Vec<Packet>, opportunity: Option<Moment>) -> Self {
        Self {
            state: BatchState::Queued {
                packets,
                opportunity,
            },
            status_epoch: None,
        }
    }

    /// Serializes epoch checks and queue acceptance, then observes the accepted write once.
    fn poll(
        &mut self,
        writer: &BatchWriter,
        status: &SharedCacheStatus,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), BridgeError>> {
        let mut status = status.lock().expect("cache status lock");
        loop {
            match &mut self.state {
                BatchState::Queued {
                    packets,
                    opportunity,
                } => {
                    if let Some(now) = opportunity.take()
                        && let Some(packet) = status.select(now)
                    {
                        self.status_epoch = Some(status.epoch);
                        packets.push(Packet::new(packet));
                    }
                    let Some(frame) = crate::session_transport::batch_frame(packets) else {
                        self.state = BatchState::Done;
                        continue;
                    };
                    let writer = writer.clone();
                    self.state = BatchState::Preparing(Mutex::new(Box::pin(async move {
                        writer.prepare(frame).await
                    })));
                }
                BatchState::Preparing(preparing) => {
                    self.state = match ready!(
                        preparing
                            .get_mut()
                            .expect("relay preparation lock")
                            .as_mut()
                            .poll(cx)
                    ) {
                        Some(batch) => BatchState::Prepared {
                            batch,
                            accepting: None,
                        },
                        None => BatchState::Done,
                    };
                }
                BatchState::Prepared { batch, accepting } => {
                    if self.status_epoch.is_some_and(|epoch| epoch != status.epoch) {
                        batch.frame = without_cache_status(&batch.frame)?;
                        self.status_epoch = None;
                        *accepting = None;
                    }
                    if batch.frame.len() == 1 {
                        self.state = BatchState::Done;
                        continue;
                    }
                    if accepting.is_none() {
                        let frame = batch.frame.clone();
                        let writer = writer.clone();
                        *accepting = Some(Mutex::new(Box::pin(async move {
                            writer.accept_prepared(frame).await
                        })));
                    }
                    // Admission cannot suspend after committing its permit. Keep the epoch
                    // check and ticket under one lock; resets only rebuild the capacity wait.
                    let written = ready!(
                        accepting
                            .as_mut()
                            .expect("prepared admission")
                            .get_mut()
                            .expect("batch admission lock")
                            .as_mut()
                            .poll(cx)
                    )?;
                    self.state = BatchState::Written(written);
                }
                BatchState::Written(written) => return Pin::new(written).poll(cx),
                BatchState::Done => return Poll::Ready(Ok(())),
            }
        }
    }
}

/// Retires stale status entries while preserving the relay's other bytes and packet order.
fn without_cache_status(frame: &Bytes) -> Result<Bytes, BridgeError> {
    let mut retained = vec![bridge::contract::KIND_BATCH];
    for entry in wire::Frames::new(frame.slice(1..), usize::MAX) {
        let entry = entry.map_err(|_| BridgeError::InvalidSessionMessage {
            reason: "malformed relayed outbound batch",
        })?;
        if entry.header().id != PacketId::ClientCacheBlobStatus as u32 {
            retained.extend_from_slice(entry.frame());
        }
    }
    Ok(retained.into())
}

impl<R: bridge::FrameRead> PlaySession<R> {
    /// Detaches the write half; batches it sends share the session's own send FIFO and tap.
    pub fn outbound(&mut self) -> Result<PlayOutbound, ProtocolError> {
        Ok(PlayOutbound {
            writer: self.transport.batch_writer(),
            finish_loading: self.take_finish_loading_packets(),
            cache_status: self.cache_status.clone(),
            pending: VecDeque::new(),
        })
    }
}

impl PlayOutbound {
    /// Writes `packets` as one batch behind every frame already accepted; resolves once the
    /// batch is flushed, waiting for queue capacity first.
    /// An explicit simulation opportunity may append one cache status; ordinary frames do not.
    /// Cancellation retains every submitted batch until the next call resumes its write.
    pub async fn send_batch(
        &mut self,
        packets: Vec<Packet>,
        opportunity: Option<Moment>,
    ) -> Result<(), ProtocolError> {
        for packet in &packets {
            crate::codec::validate_packet(packet)?;
        }
        if !packets.is_empty() || opportunity.is_some() {
            self.pending
                .push_back(PendingBatch::new(packets, opportunity));
        }
        poll_fn(|cx| {
            while let Some(batch) = self.pending.front_mut() {
                let result = ready!(batch.poll(&self.writer, &self.cache_status, cx));
                self.pending.pop_front();
                if let Err(error) = result {
                    return Poll::Ready(Err(ProtocolError::Bridge(error.into())));
                }
            }
            Poll::Ready(Ok(()))
        })
        .await
    }

    /// Takes the one-shot loading-end and initialization packets the session would send.
    pub fn take_finish_loading(&mut self) -> Vec<Packet> {
        std::mem::take(&mut self.finish_loading)
    }
}
