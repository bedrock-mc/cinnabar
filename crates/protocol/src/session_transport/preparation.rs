//! Requested protocol controls remain live while presentation preparation holds play packets.

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
use super::Reader;
use super::{
    ReadItem, SessionTransport, Terminal, packet_terminal, poll_reader_ahead, pop_read_ahead,
};
use crate::raw_packet::RawPacket;
use crate::{Packet, ProtocolError, SessionError};
use bedrock_protocol_1_26_51::packets::{self, PacketId};
use std::future::poll_fn;

/// Keeps input failures distinct from writes that permit the existing startup terminal drain.
pub(crate) enum PreparationFailure {
    Input(SessionError),
    Write(ProtocolError),
}

impl From<SessionError> for PreparationFailure {
    fn from(error: SessionError) -> Self {
        Self::Input(error)
    }
}

/// The next eligible control after the already inspected ordinary receive prefix.
enum PreparationControl {
    Reply(Packet),
    Terminal(Terminal),
    Exhausted,
}

impl<R: bridge::FrameRead> SessionTransport<R> {
    /// Retains ordinary packets on the existing FIFO while answering eligible latency requests.
    pub(crate) async fn read_ahead_during_preparation(
        &mut self,
        max_bytes: usize,
    ) -> Result<Option<Terminal>, PreparationFailure> {
        // An already split terminal supersedes a response retained by a canceled wait.
        if let Some(terminal) = self.preparation_terminal()? {
            return Ok(Some(terminal));
        }
        self.drain_send().await.map_err(PreparationFailure::Write)?;
        loop {
            match self.next_preparation_control()? {
                PreparationControl::Reply(reply) => {
                    // The first send poll retains its owner before the next suspension.
                    self.send(&[reply])
                        .await
                        .map_err(PreparationFailure::Write)?;
                    continue;
                }
                PreparationControl::Terminal(terminal) => return Ok(Some(terminal)),
                PreparationControl::Exhausted => {}
            }
            if let Some(batch) = self.startup.pop_front() {
                #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                let batch = if matches!(&self.reader, Reader::Native(_)) {
                    let mut batch = batch;
                    batch.apply_relay(self.relay.as_ref(), self.joining);
                    batch
                } else {
                    batch
                };
                self.receive_retained_batch(batch, 0)?;
                continue;
            }
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            if let Some(front) = self.ahead.front()
                && let Some(Ok(ReadItem::Native(batch))) = &front.item
                && !batch.relay_applied
                && let Some(deadline) = front.delay
                && let Some(delay) = self.packet_delay()
                && delay
                    .remaining(deadline, super::native::delay::now())
                    .is_some()
            {
                // No later callback or control may pass this original delayed front.
                delay.wait(Some(deadline)).await;
                continue;
            }
            if self.split_preparation_batch()? {
                continue;
            }
            // A non-batch front retains its existing core terminal/error/EOF handling.
            if !self.ahead.is_empty() {
                return Ok(self.buffered_startup_terminal()?);
            }
            let available = max_bytes.saturating_sub(self.preparation_bytes);
            if available == 0 {
                return Ok(None);
            }
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            let delay = self.packet_delay();
            poll_fn(|cx| {
                poll_reader_ahead(
                    &mut self.reader,
                    &mut self.ahead,
                    &mut self.ahead_bytes,
                    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                    delay.clone(),
                    available,
                    cx,
                )
            })
            .await;
        }
    }

    /// Releases preparation ownership only after its actual finite job has completed.
    pub(crate) fn complete_preparation(&mut self) {
        self.preparation_bytes = 0;
        self.preparation_scanned = 0;
        self.preparation_terminals_scanned = 0;
    }

    /// Splits one eligible original front through the normal relay and tap owners.
    fn split_preparation_batch(&mut self) -> Result<bool, SessionError> {
        let eligible = match self.ahead.front().and_then(|entry| entry.item.as_ref()) {
            Some(Ok(ReadItem::Framed(frame))) => {
                crate::session_wire::batch_frame_body(frame).is_some()
            }
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Some(Ok(ReadItem::Native(_))) => true,
            _ => false,
        };
        if !eligible {
            return Ok(false);
        }
        let entry = pop_read_ahead(&mut self.ahead, &mut self.ahead_bytes)
            .expect("the eligible original batch is retained");
        let previous = self.received.len();
        self.split_frames += 1;
        let charge = match entry.item.expect("an eligible batch exists")? {
            ReadItem::Framed(frame) => {
                let body = crate::session_wire::batch_frame_body(&frame)
                    .expect("the eligible frame is a batch");
                self.receive_server_batch(body, self.split_frames)?;
                entry.charge
            }
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            ReadItem::Native(mut batch) => {
                batch.apply_relay(self.relay.as_ref(), self.joining);
                let charge = entry.charge.max(batch.body.len());
                self.receive_retained_batch(batch, self.split_frames)?;
                charge
            }
        };
        let retained = self
            .received
            .range(previous..)
            .map(RawPacket::backing_len)
            .max()
            .unwrap_or(0);
        self.preparation_bytes = self.preparation_bytes.saturating_add(charge.max(retained));
        Ok(true)
    }

    /// Inspects only the new tail before any response can suspend terminal handling.
    fn preparation_terminal(&mut self) -> Result<Option<Terminal>, SessionError> {
        for raw in self.received.range(self.preparation_terminals_scanned..) {
            if let Some(terminal) = packet_terminal(raw)? {
                return Ok(Some(terminal));
            }
        }
        self.preparation_terminals_scanned = self.received.len();
        Ok(None)
    }

    /// Classifies new terminals before writing; ordinary probes retain their existing handling.
    fn next_preparation_control(&mut self) -> Result<PreparationControl, SessionError> {
        if let Some(terminal) = self.preparation_terminal()? {
            return Ok(PreparationControl::Terminal(terminal));
        }
        while let Some(raw) = self.received.get(self.preparation_scanned) {
            if raw.id == PacketId::NetworkStackLatency
                && let Ok(packet) = raw.clone().decode()
                && let packets::Packet::NetworkStackLatency(probe) = &*packet.data
                && probe.is_from_server
            {
                let timestamp = probe.creation_time.0;
                self.received.remove(self.preparation_scanned);
                self.preparation_terminals_scanned -= 1;
                return Ok(PreparationControl::Reply(
                    crate::network_stack_latency_reply(timestamp),
                ));
            }
            self.preparation_scanned += 1;
        }
        Ok(PreparationControl::Exhausted)
    }
}
