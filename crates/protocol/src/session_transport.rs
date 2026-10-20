use std::collections::VecDeque;
use std::future::{Future, poll_fn};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};

use crate::codec::SessionFailure;
#[cfg(not(target_arch = "wasm32"))]
use crate::packet_relay::PacketRelay;
use crate::packet_relay::{BatchWriter, RelayLink, put_entry};
use crate::packet_tap::{PacketBody, PacketDirection, PacketOutcome, PacketRecordId, PacketTap};
use crate::raw_packet::{MAX_RAW_BATCH_PACKETS, RawPacket, decode_batch_body};
use crate::session_wire::{CoreMessage, SessionDisconnect, SessionTransfer};
use crate::{Packet, ProtocolError, SessionError};
use bridge::{BridgeError, FrameQueue, FramedReader};
use bytes::{BufMut, Bytes, BytesMut};
mod io;
pub(crate) use io::{PacketSender, PacketWritten};
use io::{ReadItem, Reader, ServerBatch};
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub(crate) mod native;

/// A server may batch a whole pack transfer, or a large item registry, into one batch,
/// so the local leg accepts batches far larger than a typical client's 16 MiB default.
/// The core already bounds pack transfers before they reach this hop.
pub(crate) const MAX_SESSION_BATCH_BYTES: usize =
    bridge::contract::MAX_UPSTREAM_DECOMPRESSED_BATCH_BYTES;
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
    reader: Reader<R>,
    timers: bridge::tasks::Timers,
    writer: BatchWriter,
    relay: Option<RelayLink>,
    prefer_injected: bool,
    joining: bool,
    startup: VecDeque<ServerBatch>, // startup bodies and relay state, read before later input
    received: VecDeque<RawPacket>,  // packets split from a batch but not yet read
    ahead: VecDeque<ReadAhead>,     // original reader items and their immutable admission metadata
    ahead_bytes: usize,             // conservative admission bytes charged to `ahead`
    ended: bool,                    // a terminal message was delivered
    sending: VecDeque<InFlightSend>,
    tap: Option<Arc<dyn PacketTap>>,
    current: Option<CurrentPacket>, // the tapped packet the session is processing
    split_frames: u64,              // batch frames split so far; startup batches count as 0
    delivered: (u64, u32),          // frame and index of the packet `recv` returned last
}

/// One original read-ahead FIFO entry, with native delay metadata on the SAME body/terminal.
struct ReadAhead {
    item: Option<Result<ReadItem, SessionError>>,
    charge: usize,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    delay: Option<native::delay::Deadline>,
}

/// The last delivered packet, kept until the session asks for the next one so an outcome
/// report can name it.
struct CurrentPacket {
    record: PacketRecordId,
    packet_id: u32,
    body: Bytes,
}

type FrameSend = Pin<Box<dyn Future<Output = Result<(), ProtocolError>> + Send>>;

/// A send retained across cancellation until its frame is flushed.
struct InFlightSend {
    // Only reached through `&mut`; the mutex just keeps the transport `Sync`.
    write: Mutex<FrameSend>,
}

impl InFlightSend {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ProtocolError>> {
        self.write
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            .poll(cx)
    }
}

impl<R: bridge::FrameRead> SessionTransport<R> {
    /// Borrows policy for only this original native connection, without extending transport lifetime.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub(crate) fn packet_delay(&self) -> Option<native::delay::NativePacketDelay> {
        match self.writer.sender() {
            PacketSender::Core(_) => None,
            PacketSender::Native(sender) => Some(sender.delay.clone()),
        }
    }

    /// Clones only the native transport's read-only observation, never its lifecycle owner.
    pub(crate) fn observe_rtt(&self) -> Option<crate::RoundTripTime> {
        match &self.reader {
            Reader::Core(_) => None,
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Reader::Native(reader) => reader.rtt.clone(),
        }
    }

    /// Closes the native connection while its runtime is alive; core I/O belongs to its host.
    pub(crate) async fn close(&mut self) -> Result<(), ProtocolError> {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if let Reader::Native(reader) = &mut self.reader {
            reader
                .close()
                .await
                .map_err(|error| ProtocolError::from(SessionFailure::NativeTransport(error)))?;
        }
        Ok(())
    }

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
            reader: Reader::Core(reader),
            timers,
            writer: BatchWriter::direct(PacketSender::Core(frames)),
            relay: None,
            prefer_injected: false,
            joining: true,
            startup: startup_batches(startup)
                .into_iter()
                .map(ServerBatch::new)
                .collect(),
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
        self.writer = self.writer.clone().with_tap(tap.clone());
        self.tap = tap;
        self
    }

    /// The FIFO this transport's own sends share with detached outbound batches.
    #[cfg(test)]
    pub(crate) fn sender(&self) -> PacketSender {
        self.writer.sender().clone()
    }

    /// Exposes only the original Core queue for its capacity-controlled fixtures.
    #[cfg(test)]
    pub(crate) fn frame_queue(&self) -> FrameQueue {
        match self.writer.sender().clone() {
            PacketSender::Core(frames) => frames,
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            PacketSender::Native(_) => panic!("this fixture requires the original Core queue"),
        }
    }

    /// Shares the original packet FIFO with detached outbound writes and relay injections.
    pub(crate) fn batch_writer(&self) -> BatchWriter {
        self.writer.clone()
    }

    /// Runs this session's selected relay on its retained reactor before packet observation.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_relay(&mut self, relay: Arc<dyn PacketRelay>) {
        let (link, writer) =
            RelayLink::start(relay, self.writer.sender().clone(), self.tap.clone());
        self.relay = Some(link);
        self.writer = writer;
    }

    /// Ends the relay's startup budget only after the original spawn observer completes.
    pub(crate) fn finish_startup(&mut self) {
        self.joining = false;
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
                self.receive_retained_batch(batch, 0)?;
                continue;
            }
            if self.ended {
                return Err(SessionFailure::ConnectionClosed.into());
            }
            let next = match self.next_frame().await {
                IncomingFrame::Upstream(next) => next,
                IncomingFrame::Injected(body) => {
                    self.receive_relayed(body, 0);
                    continue;
                }
            };
            let frame = match next {
                Some(Ok(frame)) => frame,
                Some(Err(error)) => return Err(error),
                None => return Err(SessionFailure::ConnectionClosed.into()),
            };
            #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
            let ReadItem::Core(frame) = frame;
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            let frame = match frame {
                ReadItem::Core(frame) => frame,
                #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                ReadItem::Native(batch) => {
                    self.split_frames += 1;
                    self.receive_retained_batch(batch, self.split_frames)?;
                    continue;
                }
            };
            if let Some(body) = crate::session_wire::batch_frame_body(&frame) {
                self.split_frames += 1;
                self.receive_server_batch(body, self.split_frames)?;
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

    /// Alternates injected bodies with the same cancellation-safe delayed reader front.
    async fn next_frame(&mut self) -> IncomingFrame {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        let delay = self.packet_delay();
        let upstream = next_reader_item(
            &mut self.reader,
            &mut self.ahead,
            &mut self.ahead_bytes,
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            delay,
        );
        tokio::pin!(upstream);
        let prefer_injected = &mut self.prefer_injected;
        let relay = &mut self.relay;
        poll_fn(|cx| {
            if *prefer_injected
                && let Some(relay) = relay.as_mut()
                && let Poll::Ready(Some(body)) = relay.injected.poll_recv(cx)
            {
                *prefer_injected = false;
                return Poll::Ready(IncomingFrame::Injected(body));
            }
            if let Poll::Ready(next) = upstream.as_mut().poll(cx) {
                *prefer_injected = true;
                return Poll::Ready(IncomingFrame::Upstream(next));
            }
            if let Some(relay) = relay.as_mut()
                && let Poll::Ready(Some(body)) = relay.injected.poll_recv(cx)
            {
                *prefer_injected = false;
                return Poll::Ready(IncomingFrame::Injected(body));
            }
            Poll::Pending
        })
        .await
    }

    /// Inspects the first retained terminal without consuming accepted input or delay metadata.
    pub(crate) fn buffered_startup_terminal(&self) -> Result<Option<Terminal>, SessionError> {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if matches!(&self.reader, Reader::Native(_)) {
            for packet in &self.received {
                if let Some(terminal) = native_packet_terminal(packet)? {
                    return Ok(Some(terminal));
                }
            }
            for batch in &self.startup {
                if !batch.relay_applied {
                    return Ok(None);
                }
                if let Some(terminal) = native_batch_terminal(&batch.body)? {
                    return Ok(Some(terminal));
                }
            }
            for entry in &self.ahead {
                if let Some(Ok(ReadItem::Native(batch))) = &entry.item {
                    if !batch.relay_applied {
                        return Ok(None);
                    }
                    if let Some(terminal) = native_batch_terminal(&batch.body)? {
                        return Ok(Some(terminal));
                    }
                }
            }
            return Ok(None);
        }
        let Some(ReadAhead {
            item: Some(Ok(ReadItem::Core(frame))),
            ..
        }) = self.ahead.back()
        else {
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

    /// Drops malformed relay output locally while preserving the original session.
    fn receive_relayed(&mut self, body: Bytes, frame: u64) {
        match self.split_batch(body, frame) {
            Ok(packets) => self.received.extend(packets),
            Err(error) => tracing::warn!(%error, "dropping a relayed batch that does not decode"),
        }
    }

    /// Delivers a retained final body without applying the selected relay a second time.
    fn receive_retained_batch(
        &mut self,
        batch: ServerBatch,
        frame: u64,
    ) -> Result<(), SessionError> {
        if batch.relay_applied {
            let packets = self.split_batch(batch.body, frame)?;
            self.received.extend(packets);
            Ok(())
        } else {
            self.receive_server_batch(batch.body, frame)
        }
    }

    /// Both actual upstream owners pass through the relay before decoding and observation.
    fn receive_server_batch(&mut self, body: Bytes, frame: u64) -> Result<(), SessionError> {
        let replaced = self
            .relay
            .as_ref()
            .and_then(|relay| relay.inbound(&body, self.joining));
        match replaced {
            Some(body) => self.receive_relayed(body, frame),
            None => {
                let packets = self.split_batch(body, frame)?;
                self.received.extend(packets);
            }
        }
        Ok(())
    }

    /// Retains original input ahead of [`Self::recv`] until the byte bound or a terminal.
    /// Native bodies pass through their selected relay before terminal metadata is inspected;
    /// delayed callbacks remain behind the original delivery deadline. Cancellation loses nothing.
    pub(crate) async fn read_ahead(&mut self, max_bytes: usize) {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if matches!(&self.reader, Reader::Native(_))
            && !matches!(self.buffered_startup_terminal(), Ok(None))
        {
            return;
        }
        while poll_fn(|cx| self.poll_read_ahead_one(max_bytes, cx)).await {}
    }

    /// Stages at most one reader item, preserving partial I/O and native delay on cancellation.
    /// Returns false at the shared byte bound or a terminal, error or EOF marker. Accepting or
    /// replacing one whole body may cross the bound by that body's size.
    pub(crate) fn poll_read_ahead_one(
        &mut self,
        max_bytes: usize,
        cx: &mut Context<'_>,
    ) -> Poll<bool> {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        let delay = self.packet_delay();
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if matches!(&self.reader, Reader::Native(_)) && !self.apply_native_read_ahead(max_bytes) {
            return Poll::Ready(false);
        }
        let more = poll_reader_ahead(
            &mut self.reader,
            &mut self.ahead,
            &mut self.ahead_bytes,
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            delay,
            max_bytes,
            cx,
        );
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if more.is_ready() && matches!(&self.reader, Reader::Native(_)) {
            return more.map(|more| self.apply_native_read_ahead(max_bytes) && more);
        }
        more
    }

    /// Applies earlier startup and reader callbacks once, before inspecting their final bodies.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    fn apply_native_read_ahead(&mut self, max_bytes: usize) -> bool {
        for packet in &self.received {
            if !matches!(native_packet_terminal(packet), Ok(None)) {
                return false;
            }
        }
        for batch in &mut self.startup {
            batch.apply_relay(self.relay.as_ref(), self.joining);
            if !matches!(native_batch_terminal(&batch.body), Ok(None)) {
                return false;
            }
        }
        // Only FIFO-ordered native bodies are marked: appends are unprocessed, a delayed body
        // leaves every later body untouched, and an error/EOF ends that same pending suffix.
        let pending = self.ahead.partition_point(|entry| {
            matches!(entry.item.as_ref(), Some(Ok(ReadItem::Native(batch))) if batch.relay_applied)
        });
        if pending != 0 && ends_session(&self.ahead[pending - 1].item) {
            return false;
        }
        let delay = self.packet_delay();
        for entry in self.ahead.range_mut(pending..) {
            let mut applied = false;
            if let Some(Ok(ReadItem::Native(batch))) = &mut entry.item {
                // Later callbacks cannot overtake an earlier body still awaiting its delivery time.
                if !batch.relay_applied
                    && entry.delay.is_some_and(|deadline| {
                        delay.as_ref().is_some_and(|delay| {
                            delay.remaining(deadline, native::delay::now()).is_some()
                        })
                    })
                {
                    return self.ahead_bytes < max_bytes;
                }
                applied = !batch.relay_applied;
                batch.apply_relay(self.relay.as_ref(), self.joining);
                // A dropped body still owns its original positive admission and trace position.
                let charge = entry.charge.max(batch.body.len());
                self.ahead_bytes += charge - entry.charge;
                entry.charge = charge;
                if !matches!(native_batch_terminal(&batch.body), Ok(None)) {
                    return false;
                }
            }
            if (applied && self.ahead_bytes >= max_bytes) || ends_session(&entry.item) {
                return false;
            }
        }
        self.ahead_bytes < max_bytes
    }

    /// Adopts previously unobserved startup packets ahead of unread input, preserving their bodies.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub(crate) fn adopt_startup(&mut self, mut packets: Vec<RawPacket>) {
        if self.relay.is_some() {
            let bodies: Vec<_> = packets.iter().map(|packet| packet.body().clone()).collect();
            self.startup
                .extend(startup_batches(&bodies).into_iter().map(ServerBatch::new));
        } else {
            self.record_received(&mut packets, 0);
            self.prepend(packets);
        }
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
        self.record_received(&mut packets, frame);
        Ok(packets)
    }

    /// Tags newly received packets and records their original bodies once before delivery.
    fn record_received(&self, packets: &mut [RawPacket], frame: u64) {
        for (index, packet) in packets.iter_mut().enumerate() {
            packet.trace_position = (frame, index as u32);
        }
        if let Some(tap) = &self.tap {
            for packet in packets {
                packet.record = Some(tap.record(
                    PacketDirection::Clientbound,
                    packet.id as u32,
                    PacketBody::whole(packet.body()),
                ));
            }
        }
    }

    /// Writes `packets` as one Batch frame after every send retained before it, resolving once
    /// it is flushed. An empty list sends nothing.
    pub(crate) async fn send(&mut self, packets: &[Packet]) -> Result<(), ProtocolError> {
        if packets.is_empty() {
            return Ok(());
        }
        let Some(frame) = batch_body(packets) else {
            return Ok(());
        };
        let writer = self.writer.clone();
        self.sending.push_back(InFlightSend {
            write: Mutex::new(Box::pin(async move { writer.send(frame).await })),
        });
        self.drain_send().await
    }

    /// Completes every send retained across cancellation, in order.
    pub(crate) async fn drain_send(&mut self) -> Result<(), ProtocolError> {
        poll_fn(|cx| self.poll_drain_send(cx)).await
    }

    fn poll_drain_send(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ProtocolError>> {
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

/// The next original input or relay injection; upstream metadata stays on its original FIFO.
enum IncomingFrame {
    Upstream(Option<Result<ReadItem, SessionError>>),
    Injected(Bytes),
}

/// Takes only an eligible original front, retaining its immutable delay through cancellation.
async fn next_reader_item<R: bridge::FrameRead>(
    reader: &mut Reader<R>,
    ahead: &mut VecDeque<ReadAhead>,
    ahead_bytes: &mut usize,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))] delay: Option<
        native::delay::NativePacketDelay,
    >,
) -> Option<Result<ReadItem, SessionError>> {
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    if let Some(delay) = delay {
        loop {
            if let Some(front) = ahead.front() {
                let deadline = front.delay;
                if deadline.is_some_and(|deadline| {
                    delay.remaining(deadline, native::delay::now()).is_some()
                }) {
                    let due = delay.wait(deadline);
                    tokio::pin!(due);
                    let mut reading = true;
                    loop {
                        tokio::select! {
                            _ = &mut due => break,
                            more = poll_fn(|cx| poll_reader_ahead(reader, ahead, ahead_bytes,
                                Some(delay.clone()), MAX_READ_AHEAD_BYTES, cx)), if reading => { reading = more; }
                        }
                    }
                }
                return pop_read_ahead(ahead, ahead_bytes)
                    .expect("retained original reader front")
                    .item;
            }
            let next = poll_fn(|cx| reader.poll_next(cx)).await;
            let deadline = delay.capture(native::delay::now());
            if deadline.is_none() {
                return next;
            }
            retain_read_ahead(
                ahead,
                ahead_bytes,
                ReadAhead {
                    item: next,
                    charge: 0,
                    delay: deadline,
                },
            );
        }
    }
    match pop_read_ahead(ahead, ahead_bytes) {
        Some(entry) => entry.item,
        None => poll_fn(|cx| reader.poll_next(cx)).await,
    }
}

/// Removes only the eligible original entry, releasing its conservative admission charge.
fn pop_read_ahead(ahead: &mut VecDeque<ReadAhead>, bytes: &mut usize) -> Option<ReadAhead> {
    let entry = ahead.pop_front()?;
    *bytes -= entry.charge;
    Some(entry)
}

/// Retains one original input or terminal, charging its original body before any relay change.
fn retain_read_ahead(ahead: &mut VecDeque<ReadAhead>, bytes: &mut usize, mut entry: ReadAhead) {
    entry.charge = entry
        .item
        .as_ref()
        .and_then(|item| item.as_ref().ok())
        .map_or(0, ReadItem::len);
    *bytes += entry.charge;
    ahead.push_back(entry);
}

/// Stages one original reader item without losing its partial I/O or native delay on cancellation.
fn poll_reader_ahead<R: bridge::FrameRead>(
    reader: &mut Reader<R>,
    ahead: &mut VecDeque<ReadAhead>,
    ahead_bytes: &mut usize,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))] delay: Option<
        native::delay::NativePacketDelay,
    >,
    max_bytes: usize,
    cx: &mut Context<'_>,
) -> Poll<bool> {
    if *ahead_bytes >= max_bytes || ahead.back().is_some_and(|entry| ends_session(&entry.item)) {
        return Poll::Ready(false);
    }
    let next = ready!(reader.poll_next(cx));
    let ends = ends_session(&next);
    retain_read_ahead(
        ahead,
        ahead_bytes,
        ReadAhead {
            item: next,
            charge: 0,
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            delay: delay.and_then(|delay| delay.capture(native::delay::now())),
        },
    );
    Poll::Ready(*ahead_bytes < max_bytes && !ends)
}

/// Whether a reader item ends the session: a terminal message, a failure or the close.
fn ends_session(next: &Option<Result<ReadItem, SessionError>>) -> bool {
    !matches!(next, Some(Ok(frame)) if frame.is_batch())
}

/// Borrows only terminal metadata; unrelated bodies and unknown IDs remain undecoded.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
fn native_batch_terminal(body: &Bytes) -> Result<Option<Terminal>, SessionError> {
    use bedrock_protocol_1_26_51::{packets::PacketId, wire};

    if body.len() > MAX_SESSION_BATCH_BYTES {
        return Err(SessionFailure::BatchTooLarge {
            actual: body.len(),
            max: MAX_SESSION_BATCH_BYTES,
        }
        .into());
    }
    let mut terminal = None;
    let mut offset = 0;
    for frame in wire::Frames::new(body.clone(), MAX_RAW_BATCH_PACKETS) {
        let frame =
            frame.map_err(|error| crate::raw_packet::framing_failure(&body[offset..], error))?;
        offset += frame.frame().len();
        if terminal.is_none()
            && matches!(
                PacketId::from_raw(frame.header().id),
                Some(PacketId::Disconnect | PacketId::Transfer)
            )
        {
            let packet = crate::raw_packet::raw_packet(frame, body.len())?;
            terminal = native_packet_terminal(&packet)?;
        }
    }
    Ok(terminal)
}

/// Projects one usable server control using its ordinary decoder and target/text policies.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
fn native_packet_terminal(packet: &RawPacket) -> Result<Option<Terminal>, SessionError> {
    use bedrock_protocol_1_26_51::{packets, types::DisconnectMessages};

    if !matches!(
        packet.id,
        packets::PacketId::Disconnect | packets::PacketId::Transfer
    ) {
        return Ok(None);
    }
    let (_, packet) = packet.clone().decode_parts()?;
    Ok(match packet {
        packets::Packet::Disconnect(packet) => {
            let (message, filtered_message) = crate::disconnect::packet_texts(&packet);
            Some(Terminal::Disconnect(SessionDisconnect {
                reason: packet.reason.to_raw(),
                message,
                filtered_message,
                hide_screen: matches!(packet.messages, DisconnectMessages::Empty),
            }))
        }
        packets::Packet::Transfer(packet) => {
            crate::ServerTransferEvent::from_packet_data(&packets::Packet::Transfer(packet))
                .ok()
                .flatten()
                .map(|target| {
                    Terminal::Transfer(SessionTransfer {
                        address: target.host,
                        port: target.port,
                        reload_world: target.reload_world,
                    })
                })
        }
        _ => unreachable!("terminal ID selects its decoded packet"),
    })
}

/// One Batch frame holding `packets`, each length-prefixed as a Bedrock batch carries it. A
/// packet the codec refuses is logged and left out so the rest still go in order; `None` when
/// none encodes.
#[cfg(test)]
pub(crate) fn batch_frame(packets: &[Packet]) -> Option<Bytes> {
    let body = batch_body(packets)?;
    let mut frame = BytesMut::with_capacity(body.len() + 1);
    frame.put_u8(bridge::contract::KIND_BATCH);
    frame.extend_from_slice(&body);
    Some(frame.freeze())
}

/// Encodes a Bedrock batch body independently of either transport's envelope.
pub(crate) fn batch_body(packets: &[Packet]) -> Option<Bytes> {
    let mut frame = Vec::new();
    for packet in packets {
        if let Err(error) = crate::codec::encode_entry(packet, &mut frame) {
            tracing::warn!(packet = ?packet.kind(), %error, "skipping an outbound packet that failed to encode");
        }
    }
    (!frame.is_empty()).then(|| Bytes::from(frame))
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
        put_entry(&mut batch, packet);
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
