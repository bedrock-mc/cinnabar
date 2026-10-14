//! Raw bodies of packets ingress rejected, bounded per packet ID and in total for one session.

use protocol::packet_tap::{PacketOutcome, carries_secrets};

/// Rejected bodies kept for one packet ID in one session.
pub const MAX_SAMPLES_PER_PACKET: usize = 4;
/// Body bytes kept across one session's samples; the sample that reaches the bound keeps a
/// prefix.
pub const MAX_SAMPLE_BYTES: usize = 1 << 20;

/// One rejected packet body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureSample {
    /// The packet log's number for the session that received the packet.
    pub session: u64,
    /// The header record's sequence number.
    pub sequence: u64,
    pub packet_id: u32,
    pub outcome: PacketOutcome,
    /// Body length on the wire.
    pub len: usize,
    /// The body, or its prefix when the byte bound cut it short.
    pub body: Vec<u8>,
}

/// One session's samples, and what that session turned away.
///
/// The samples belong to the newest session that rejected a packet: the first sample a newer
/// session offers replaces the older session's samples and counts, so a crash report still
/// shows the last session's failures until a later one has its own.
pub(crate) struct FailureSamples {
    session: u64,
    samples: Vec<FailureSample>,
    bytes: usize,
    withheld: u64,
    dropped: u64,
}

impl FailureSamples {
    pub(crate) const fn new() -> Self {
        Self {
            session: 0,
            samples: Vec::new(),
            bytes: 0,
            withheld: 0,
            dropped: 0,
        }
    }

    /// Keeps `body`, rejected in `session`, unless its packet ID already has
    /// [`MAX_SAMPLES_PER_PACKET`] samples from that session or the session's byte bound is
    /// spent. A packet that can carry credentials or content keys is counted as withheld and
    /// its body never stored. Offers from a session older than the kept one are ignored.
    pub(crate) fn offer(
        &mut self,
        session: u64,
        sequence: u64,
        packet_id: u32,
        outcome: PacketOutcome,
        body: &[u8],
    ) {
        if session < self.session {
            return;
        }
        if session > self.session {
            *self = Self {
                session,
                ..Self::new()
            };
        }
        if carries_secrets(packet_id) {
            self.withheld += 1;
            return;
        }
        let per_packet = self
            .samples
            .iter()
            .filter(|sample| sample.packet_id == packet_id)
            .count();
        let room = MAX_SAMPLE_BYTES - self.bytes;
        if per_packet == MAX_SAMPLES_PER_PACKET || (room == 0 && !body.is_empty()) {
            self.dropped += 1;
            return;
        }
        let kept = body.len().min(room);
        self.bytes += kept;
        self.samples.push(FailureSample {
            session,
            sequence,
            packet_id,
            outcome,
            len: body.len(),
            body: body[..kept].to_vec(),
        });
    }

    /// The session the samples belong to; 0 before any session rejected a packet.
    pub(crate) fn session(&self) -> u64 {
        self.session
    }

    /// Kept samples, oldest first.
    pub(crate) fn samples(&self) -> &[FailureSample] {
        &self.samples
    }

    /// Body bytes held.
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Rejected packets whose bodies were not stored because they can carry secrets.
    pub(crate) fn withheld(&self) -> u64 {
        self.withheld
    }

    /// Rejected packets past the per-ID or byte bound.
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }
}
