use std::collections::VecDeque;
use std::future::{Future, poll_fn};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};

use crate::codec::SessionFailure;
use crate::packet_tap::{PacketBody, PacketDirection, PacketOutcome, PacketRecordId, PacketTap};
use crate::raw_packet::{MAX_RAW_BATCH_PACKETS, RawPacket, decode_batch_body};
use crate::session_wire::{CoreMessage, SessionDisconnect, SessionTransfer};
use crate::{Packet, ProtocolError, SessionError};
use bridge::{BridgeError, FrameQueue, FramedReader};
use bytes::{BufMut, Bytes, BytesMut};
use futures::StreamExt;

/// A server may batch a whole pack transfer, or a large item registry, into one batch,
/// so the local leg accepts batches far larger than a typical client's 16 MiB default.
/// The core already bounds pack transfers before they reach this hop.
pub(crate) const MAX_SESSION_BATCH_BYTES: usize = 256 * 1024 * 1024;
/// Bounds frames read while packs prepare or the world consumer is full; past it the core waits.
pub(crate) const MAX_READ_AHEAD_BYTES: usize = 64 * 1024 * 1024;

/// Best-effort: tells the core whether this client applied (or reverted) the
/// newest attempt's handed-off packs; returns whether the core recorded it.
#[cfg(not(target_arch = "wasm32"))]
pub async fn report_pack_application(socket_dir: &Path, applied: bool) -> bool {
    let Ok(status) = bridge::read_status(socket_dir).await else {
        return false;
    };
    bridge::report_pack_application(socket_dir, status.pack_admission.attempt_id, applied)
        .await
        .is_ok()
}

/// What the core delivers next: a server packet, or the terminal message that ends the session.
pub(crate) enum Inbound {
    Packet(RawPacket),
    Terminal(Terminal),
}

/// The core's last message on a session, standing for the server's own Transfer or Disconnect.
pub(crate) enum Terminal {
    Transfer(SessionTransfer),
    Disconnect(SessionDisconnect),
}

/// A handed-off session's packets and its send FIFO.
///
/// The handed-off startup packets come first; each later Batch frame is split in place, without
/// copying. A terminal Transfer or Disconnect ends the stream. A send is retained from its first
/// poll until flushed, strictly in order, so cancellation neither loses nor repeats it. An
/// optional [`PacketTap`] sees each received packet once, as its batch is read and split (see
/// [`crate::packet_tap`] for what that time measures), and each sent packet once.
pub(crate) struct SessionTransport<R = FramedReader> {
    reader: R,
    timers: bridge::tasks::Timers,
    frames: FrameQueue,
    startup: VecDeque<Bytes>, // startup batch bodies, read before any core frame
    received: VecDeque<RawPacket>, // packets split from a batch but not yet read
    ahead: VecDeque<Option<Result<Bytes, BridgeError>>>, // reader items `read_ahead` took
    ahead_bytes: usize,       // frame bytes held in `ahead`
    ended: bool,              // a terminal message was delivered
    sending: VecDeque<InFlightSend>,
    tap: Option<Arc<dyn PacketTap>>,
    current: Option<CurrentPacket>, // the tapped packet the session is processing
    split_frames: u64,              // batch frames split so far; startup batches count as 0
    delivered: (u64, u32),          // frame and index of the packet `recv` returned last
}

/// The last delivered packet, kept until the session asks for the next one so an outcome
/// report can name it.
struct CurrentPacket {
    record: PacketRecordId,
    packet_id: u32,
    body: Bytes,
}

type FrameSend = Pin<Box<dyn Future<Output = Result<(), BridgeError>> + Send>>;

/// A send retained across cancellation until its frame is flushed.
struct InFlightSend {
    // Only reached through `&mut`; the mutex just keeps the transport `Sync`.
    write: Mutex<FrameSend>,
}

impl InFlightSend {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BridgeError>> {
        self.write
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            .poll(cx)
    }
}

impl<R: bridge::FrameRead> SessionTransport<R> {
    /// Continues a handed-off session; the `startup` packets, each with its header, come first.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn new(reader: R, frames: FrameQueue, startup: &[Bytes]) -> Self {
        Self::with_timers(reader, frames, startup, bridge::tasks::Timers::tokio())
    }

    /// Continues prepared I/O with deadlines registered by its host.
    pub(crate) fn with_timers(
        reader: R,
        frames: FrameQueue,
        startup: &[Bytes],
        timers: bridge::tasks::Timers,
    ) -> Self {
        Self {
            reader,
            timers,
            frames,
            startup: startup_batches(startup).into(),
            received: VecDeque::new(),
            ahead: VecDeque::new(),
            ahead_bytes: 0,
            ended: false,
            sending: VecDeque::new(),
            tap: None,
            current: None,
            split_frames: 0,
            delivered: (0, 0),
        }
    }

    /// Registers one host deadline without restarting it on each packet.
    pub(crate) fn sleep(&self, duration: std::time::Duration) -> bridge::tasks::Timer {
        self.timers.sleep(duration)
    }

    /// The latency-trace connection, frame and in-frame index of the packet [`Self::recv`]
    /// returned last; frames count the core's batch frames from 1, startup packets are frame 0.
    pub(crate) fn trace_position(&self) -> (u64, u64, u32) {
        (self.reader.trace_conn(), self.delivered.0, self.delivered.1)
    }

    /// Reports every packet this transport delivers or sends to `tap`.
    pub(crate) fn with_tap(mut self, tap: Option<Arc<dyn PacketTap>>) -> Self {
        self.tap = tap;
        self
    }

    /// The tap shared with detached outbound batches.
    pub(crate) fn tap(&self) -> Option<Arc<dyn PacketTap>> {
        self.tap.clone()
    }

    /// The FIFO this transport's own sends share with detached outbound batches.
    pub(crate) fn frame_queue(&self) -> FrameQueue {
        self.frames.clone()
    }

    /// Returns the next packet or terminal message; cancelling the wait loses nothing.
    ///
    /// Asking for the next packet settles the previous one: an outcome reported later no longer
    /// names it.
    pub(crate) async fn recv(&mut self) -> Result<Inbound, SessionError> {
        self.current = None;
        loop {
            if let Some(packet) = self.received.pop_front() {
                self.delivered = packet.trace_position;
                self.observe(&packet);
                return Ok(Inbound::Packet(packet));
            }
            if let Some(batch) = self.startup.pop_front() {
                let packets = self.split_batch(batch, 0)?;
                self.received.extend(packets);
                continue;
            }
            if self.ended {
                return Err(SessionFailure::ConnectionClosed.into());
            }
            let next = match self.ahead.pop_front() {
                Some(next) => {
                    if let Some(Ok(frame)) = &next {
                        self.ahead_bytes -= frame.len();
                    }
                    next
                }
                None => self.reader.next().await,
            };
            let frame = match next {
                Some(Ok(frame)) => frame,
                Some(Err(error)) => return Err(SessionFailure::Transport(error).into()),
                None => return Err(SessionFailure::ConnectionClosed.into()),
            };
            if let Some(body) = crate::session_wire::batch_frame_body(&frame) {
                self.split_frames += 1;
                let packets = self.split_batch(body, self.split_frames)?;
                self.received.extend(packets);
                continue;
            }
            let terminal = match crate::session_wire::decode_core_message(frame)
                .map_err(SessionFailure::Transport)?
            {
                CoreMessage::Transfer(transfer) => Terminal::Transfer(transfer),
                CoreMessage::Disconnect(disconnect) => Terminal::Disconnect(disconnect),
                CoreMessage::Handoff(_) | CoreMessage::PackData { .. } | CoreMessage::Batch(_) => {
                    return Err(
                        SessionFailure::Transport(BridgeError::InvalidSessionMessage {
                            reason: "setup message after the handoff",
                        })
                        .into(),
                    );
                }
            };
            self.ended = true;
            return Ok(Inbound::Terminal(terminal));
        }
    }

    /// Inspects the core terminal retained by read-ahead without consuming or reordering frames.
    /// Startup may abandon preparation on this boundary; play must retain ordinary FIFO delivery.
    pub(crate) fn buffered_startup_terminal(&self) -> Result<Option<Terminal>, SessionError> {
        let Some(Some(Ok(frame))) = self.ahead.back() else {
            return Ok(None);
        };
        if crate::session_wire::batch_frame_body(frame).is_some() {
            return Ok(None);
        }
        Ok(
            match crate::session_wire::decode_core_message(frame.clone())
                .map_err(SessionFailure::Transport)?
            {
                CoreMessage::Transfer(transfer) => Some(Terminal::Transfer(transfer)),
                CoreMessage::Disconnect(disconnect) => Some(Terminal::Disconnect(disconnect)),
                _ => None,
            },
        )
    }

    /// Reads core frames ahead of [`Self::recv`], unparsed, until `max_bytes` are held or the core
    /// ends the session, so short preparation or world-admission pauses do not block the core.
    /// Cancelling the wait loses nothing.
    pub(crate) async fn read_ahead(&mut self, max_bytes: usize) {
        while poll_fn(|cx| self.poll_read_ahead_one(max_bytes, cx)).await {}
    }

    /// Stages at most one unparsed reader item, retaining partial frames across cancellation.
    /// Returns false at the shared byte bound or a terminal, error or EOF marker. As with
    /// `read_ahead`, accepting a whole frame may cross the bound by that frame's size.
    pub(crate) fn poll_read_ahead_one(
        &mut self,
        max_bytes: usize,
        cx: &mut Context<'_>,
    ) -> Poll<bool> {
        if self.ahead_bytes >= max_bytes || self.ahead.back().is_some_and(ends_session) {
            return Poll::Ready(false);
        }
        let next = ready!(Pin::new(&mut self.reader).poll_next(cx));
        if let Some(Ok(frame)) = &next {
            self.ahead_bytes += frame.len();
        }
        let more = self.ahead_bytes < max_bytes && !ends_session(&next);
        self.ahead.push_back(next);
        Poll::Ready(more)
    }

    /// Puts `packets` back ahead of every unread packet, in their order. The tap has already
    /// seen them and does not see them again.
    pub(crate) fn prepend(&mut self, packets: Vec<RawPacket>) {
        let mut restored = VecDeque::from(packets);
        restored.append(&mut self.received);
        self.received = restored;
    }

    /// Releases the observed body once decoding and validation have succeeded.
    pub(crate) fn accept_current(&mut self) {
        self.current = None;
    }

    /// Keeps a detached packet's observer view on the same bounded allocation.
    pub(crate) fn compact_current(&mut self, packet: &RawPacket) {
        if let Some(current) = &mut self.current
            && packet.record == Some(current.record)
            && (current.body.as_ptr() != packet.body().as_ptr()
                || current.body.len() != packet.body().len())
        {
            current.body = packet.body().clone();
        }
    }

    /// Reports `outcome` for the packet the session is processing: the one [`Self::recv`]
    /// returned last. Only the first report after each delivery counts.
    pub(crate) fn reject_current(&mut self, outcome: PacketOutcome) {
        if let (Some(tap), Some(current)) = (&self.tap, self.current.take()) {
            tap.reject(
                current.record,
                current.packet_id,
                outcome,
                PacketBody::whole(&current.body),
            );
        }
    }

    /// Makes `packet`, which the tap recorded when its batch was split, the current packet.
    fn observe(&mut self, packet: &RawPacket) {
        if self.tap.is_none() {
            return;
        }
        self.current = packet.record.map(|record| CurrentPacket {
            record,
            packet_id: packet.id as u32,
            body: packet.body().clone(),
        });
    }

    /// Splits a batch body, tagging each packet with `frame` and its slot, and records each with
    /// the tap, so every packet of one batch carries the time its batch was read; a batch that
    /// cannot be split is reported whole.
    fn split_batch(&self, body: Bytes, frame: u64) -> Result<Vec<RawPacket>, SessionError> {
        let len = body.len();
        let mut packets = decode_batch_body(body, MAX_SESSION_BATCH_BYTES).inspect_err(|_| {
            if let Some(tap) = &self.tap {
                tap.reject_batch(len);
            }
        })?;
        for (index, packet) in packets.iter_mut().enumerate() {
            packet.trace_position = (frame, index as u32);
        }
        if let Some(tap) = &self.tap {
            for packet in &mut packets {
                packet.record = Some(tap.record(
                    PacketDirection::Clientbound,
                    packet.id as u32,
                    PacketBody::whole(packet.body()),
                ));
            }
        }
        Ok(packets)
    }

    /// Writes `packets` as one Batch frame after every send retained before it, resolving once
    /// it is flushed. An empty list sends nothing.
    pub(crate) async fn send(&mut self, packets: &[Packet]) -> Result<(), ProtocolError> {
        if packets.is_empty() {
            return Ok(());
        }
        let Some(frame) = batch_frame(packets) else {
            return Ok(());
        };
        if let Some(tap) = &self.tap {
            crate::packet_tap::record_serverbound(tap.as_ref(), &frame);
        }
        let frames = self.frames.clone();
        self.sending.push_back(InFlightSend {
            write: Mutex::new(Box::pin(async move { frames.send(frame).await })),
        });
        self.drain_send().await
    }

    /// Completes every send retained across cancellation, in order.
    pub(crate) async fn drain_send(&mut self) -> Result<(), ProtocolError> {
        poll_fn(|cx| self.poll_drain_send(cx))
            .await
            .map_err(|error| SessionFailure::Transport(error).into())
    }

    fn poll_drain_send(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BridgeError>> {
        while let Some(front) = self.sending.front_mut() {
            let result = ready!(front.poll(cx));
            self.sending.pop_front();
            if result.is_err() {
                self.sending.clear();
                return Poll::Ready(result);
            }
        }
        Poll::Ready(Ok(()))
    }
}

/// Whether a reader item ends the session: a terminal message, a failure or the close.
fn ends_session(next: &Option<Result<Bytes, BridgeError>>) -> bool {
    !matches!(next, Some(Ok(frame)) if crate::session_wire::batch_frame_body(frame).is_some())
}

/// One Batch frame holding `packets`, each length-prefixed as a Bedrock batch carries it. A
/// packet the codec refuses is logged and left out so the rest still go in order; `None` when
/// none encodes.
pub(crate) fn batch_frame(packets: &[Packet]) -> Option<Bytes> {
    let mut frame = vec![bridge::contract::KIND_BATCH];
    for packet in packets {
        if let Err(error) = crate::codec::encode_entry(packet, &mut frame) {
            tracing::warn!(packet = ?packet.kind(), %error, "skipping an outbound packet that failed to encode");
        }
    }
    (frame.len() > 1).then(|| Bytes::from(frame))
}

/// Groups the startup packets, which may span many upstream batches, into batch bodies that each
/// stay within the per-batch packet and byte bounds; a packet larger than the byte bound keeps a
/// body of its own, which is refused as it would be from the server.
fn startup_batches(packets: &[Bytes]) -> Vec<Bytes> {
    let mut batches = Vec::new();
    let mut batch = BytesMut::new();
    let mut count = 0;
    for packet in packets {
        let length = u32::try_from(packet.len()).expect("a frame bounds its packets");
        let entry = varuint32_len(length) + packet.len();
        if count != 0
            && (count == MAX_RAW_BATCH_PACKETS || batch.len() + entry > MAX_SESSION_BATCH_BYTES)
        {
            batches.push(std::mem::take(&mut batch).freeze());
            count = 0;
        }
        let mut length = length;
        while length >= 0x80 {
            batch.put_u8(length as u8 | 0x80);
            length >>= 7;
        }
        batch.put_u8(length as u8);
        batch.extend_from_slice(packet);
        count += 1;
    }
    if count != 0 {
        batches.push(batch.freeze());
    }
    batches
}

fn varuint32_len(value: u32) -> usize {
    (32 - value.max(1).leading_zeros() as usize).div_ceil(7)
}

#[cfg(test)]
pub(crate) mod tests;
