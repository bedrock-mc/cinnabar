//! Raw bodies of packets ingress rejected, bounded per packet ID and in total for one session.

pub(crate) mod writer;

use std::{fmt::Write as _, fs, io, path::Path};

use protocol::packet_tap::{PacketName, PacketOutcome, carries_secrets};

/// The file describing the samples beside their bodies.
pub(crate) const INDEX_FILE: &str = "index.txt";
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

impl FailureSample {
    /// The body's file name on disk: `<sequence>-<packet name>.bin`.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}-{}.bin", self.sequence, PacketName(self.packet_id))
    }
}

/// One session's samples, and what that session turned away.
///
/// The samples belong to the newest session that rejected a packet: the first sample a newer
/// session offers replaces the older session's samples and counts, so a crash report still
/// shows the last session's failures until a later one has its own.
#[derive(Clone)]
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
    /// its body never stored. Offers from a session older than the kept one are ignored and
    /// return `None`.
    pub(crate) fn offer(
        &mut self,
        session: u64,
        sequence: u64,
        packet_id: u32,
        outcome: PacketOutcome,
        body: &[u8],
    ) -> Option<()> {
        if session < self.session {
            return None;
        }
        let replaced = session > self.session;
        if replaced {
            *self = Self {
                session,
                ..Self::new()
            };
        }
        if carries_secrets(packet_id) {
            self.withheld += 1;
            return Some(());
        }
        let per_packet = self
            .samples
            .iter()
            .filter(|sample| sample.packet_id == packet_id)
            .count();
        let room = MAX_SAMPLE_BYTES - self.bytes;
        if per_packet == MAX_SAMPLES_PER_PACKET
            || self.samples.len() == crash_report::ring::packets::CAPACITY
            || (room == 0 && !body.is_empty())
        {
            self.dropped += 1;
            return Some(());
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
        Some(())
    }

    /// Writes new bodies and coalesced counters after a snapshot, resetting older sessions.
    pub(crate) fn persist_since(
        &self,
        dir: &Path,
        previous: Option<(u64, usize)>,
        missed: u64,
    ) -> io::Result<()> {
        let begin = match previous {
            Some((session, count)) if session == self.session => count,
            _ => {
                match fs::remove_dir_all(dir) {
                    Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                    _ => {}
                }
                fs::create_dir_all(dir)?;
                0
            }
        };
        for sample in &self.samples[begin..] {
            fs::write(dir.join(sample.file_name()), &sample.body)?;
        }
        let mut index = self.index();
        if missed != 0 {
            let _ = writeln!(
                index,
                "{missed} sample offers skipped while the snapshot was busy"
            );
        }
        let temporary = dir.join(".index.tmp");
        fs::write(&temporary, index)?;
        fs::rename(temporary, dir.join(INDEX_FILE))
    }

    /// Replaces the on-disk samples with this snapshot.
    pub(crate) fn persist_all(&self, dir: &Path) -> io::Result<()> {
        self.persist_since(dir, None, 0)
    }

    /// A summary line, then one line per sample naming its file.
    fn index(&self) -> String {
        let mut index = format!(
            "rejected packet samples from session {}: {} kept ({} bytes), {} withheld as credential-bearing, {} over the bounds\n",
            self.session,
            self.samples.len(),
            self.bytes,
            self.withheld,
            self.dropped,
        );
        for sample in &self.samples {
            let _ = writeln!(
                index,
                "{}: session {}, {} ({}) {}, {} of {} bytes",
                sample.file_name(),
                sample.session,
                PacketName(sample.packet_id),
                sample.packet_id,
                sample.outcome.name(),
                sample.body.len(),
                sample.len,
            );
        }
        index
    }

    /// Kept samples, oldest first.
    pub(crate) fn samples(&self) -> &[FailureSample] {
        &self.samples
    }

    /// The session the samples belong to; 0 before any session rejected a packet.
    #[cfg(test)]
    pub(crate) fn session(&self) -> u64 {
        self.session
    }

    /// Body bytes held.
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Rejected packets whose bodies were not stored because they can carry secrets.
    #[cfg(test)]
    pub(crate) fn withheld(&self) -> u64 {
        self.withheld
    }

    /// Rejected packets past the per-ID or byte bound.
    #[cfg(test)]
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }
}
