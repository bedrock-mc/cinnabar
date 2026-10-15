//! The always-on packet flight recorder and opt-in full capture at session ingress and egress.
//!
//! Every session reports its packets to [`global`]: a header ring of the last
//! [`crash_report::ring::packets::CAPACITY`] packets in both directions, and the raw bodies of
//! packets ingress rejected in the latest session that rejected any. Recording a header neither
//! allocates, formats nor locks; only a rejected packet's body is copied. Packets that can carry
//! credentials or pack content keys keep their header but never their body, in samples or in
//! captures.
//!
//! Once [`PacketLog::attach`]ed to the session's crash staging directory, the ring lives in a
//! mapped file there. A bounded background writer coalesces sample changes beside it, so
//! crash reports can read persisted samples after native faults. Pending sample changes may
//! be lost if the process exits before the writer runs.

mod capture;
mod samples;

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use crash_report::{
    ring::packets::{self, PacketRing},
    staging,
};
use protocol::packet_tap::{
    PacketBody, PacketDirection, PacketOutcome, PacketRecordId, PacketTap, UNFRAMED_BATCH_ID,
    carries_secrets,
};

pub use capture::{
    CAPTURE_BUFFER_BYTES, CAPTURE_EXTENSION, CAPTURE_QUEUE_BYTES, CAPTURE_QUEUE_RECORDS,
    LOG_DIR_CAPTURE_BYTES, LOG_DIR_CAPTURES_KEPT, PacketCaptureRequest,
};
pub(crate) use capture::{CAPTURE_FINISH_TIMEOUT, CaptureFeed, CaptureFinisher};
use samples::FailureSamples;
pub use samples::{FailureSample, MAX_SAMPLE_BYTES, MAX_SAMPLES_PER_PACKET};

/// One packet header as the ring stores it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeaderRecord {
    /// Position in the stream of every record this ring has accepted, starting at 1.
    pub sequence: u64,
    pub direction: PacketDirection,
    pub packet_id: u32,
    /// Body length in bytes.
    pub len: u32,
    /// Monotonic milliseconds since the packet log's epoch, as the recorder supplied them.
    pub elapsed_ms: u64,
    pub outcome: PacketOutcome,
}

impl HeaderRecord {
    /// The typed record of a raw ring header; `None` for wire bytes no type has.
    #[must_use]
    pub fn from_ring(header: packets::PacketHeader) -> Option<Self> {
        Some(Self {
            sequence: header.sequence,
            direction: PacketDirection::from_wire(header.direction)?,
            packet_id: header.packet_id,
            len: header.len,
            elapsed_ms: header.elapsed_ms,
            outcome: PacketOutcome::from_wire(header.outcome)?,
        })
    }
}

/// Packet headers and rejected bodies recorded by this process's sessions.
pub struct PacketLog {
    ring: PacketRing,
    samples: Arc<Mutex<FailureSamples>>,
    /// Where sample changes are written once attached.
    samples_writer: OnceLock<samples::writer::SampleWriter>,
    missed_samples: Arc<AtomicU64>,
    sessions: AtomicU64, // sessions begun so far
}

static PACKET_LOG: LazyLock<PacketLog> = LazyLock::new(PacketLog::new);

/// The process-wide packet log every session records to.
#[must_use]
pub fn global() -> &'static PacketLog {
    &PACKET_LOG
}

impl PacketLog {
    pub(crate) fn new() -> Self {
        Self {
            ring: PacketRing::new(),
            samples: Arc::new(Mutex::new(FailureSamples::new())),
            samples_writer: OnceLock::new(),
            missed_samples: Arc::new(AtomicU64::new(0)),
            sessions: AtomicU64::new(0),
        }
    }

    /// Moves the header ring into [`staging::PACKET_RING_FILE`] in the crash staging directory
    /// `staging_dir` and from then on writes the samples to [`staging::DECODE_SAMPLES_DIR`]
    /// there, starting with those kept so far. Headers recorded during the move may be lost.
    /// Fails, changing nothing for the samples, when the file cannot be mapped or the log is
    /// already attached.
    pub fn attach(&self, staging_dir: &Path) -> io::Result<()> {
        self.ring
            .attach(&staging_dir.join(staging::PACKET_RING_FILE))?;
        let dir = staging_dir.join(staging::DECODE_SAMPLES_DIR);
        self.lock_samples().clone().persist_all(&dir)?;
        let writer = samples::writer::SampleWriter::start(
            dir,
            Arc::clone(&self.samples),
            Arc::clone(&self.missed_samples),
        )?;
        let _ = self.samples_writer.set(writer);
        self.samples_writer.get().unwrap().changed();
        Ok(())
    }

    /// Numbers a new session, from 1; its rejected bodies replace older sessions' samples.
    pub(crate) fn begin_session(&self) -> u64 {
        self.sessions.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Appends a header, returning its sequence number. `elapsed_ms` is the caller's monotonic
    /// time since its epoch.
    #[inline]
    pub(crate) fn record(
        &self,
        direction: PacketDirection,
        packet_id: u32,
        len: usize,
        elapsed_ms: u64,
        outcome: PacketOutcome,
    ) -> u64 {
        let len = u32::try_from(len).unwrap_or(u32::MAX);
        self.ring
            .record(direction as u8, packet_id, len, elapsed_ms, outcome as u8)
    }

    /// Records that ingress in `session` rejected record `sequence` with `outcome`, keeping its
    /// body as a sample within that session's sample bounds, and on disk once attached.
    pub(crate) fn reject(
        &self,
        session: u64,
        sequence: u64,
        packet_id: u32,
        outcome: PacketOutcome,
        body: &[u8],
    ) {
        self.ring.settle(sequence, outcome as u8);
        match self.samples.try_lock() {
            Ok(mut samples) => {
                samples.offer(session, sequence, packet_id, outcome, body);
            }
            Err(TryLockError::Poisoned(error)) => {
                error
                    .into_inner()
                    .offer(session, sequence, packet_id, outcome, body);
            }
            Err(TryLockError::WouldBlock) => {
                self.missed_samples.fetch_add(1, Ordering::Relaxed);
            }
        }
        if let Some(writer) = self.samples_writer.get() {
            writer.changed();
        }
    }

    /// The retained headers, oldest first. A header overwritten while it is read is left out.
    #[must_use]
    pub fn headers(&self) -> Vec<HeaderRecord> {
        let mut headers = Vec::with_capacity(packets::CAPACITY);
        self.ring
            .for_each(|header| headers.extend(HeaderRecord::from_ring(header)));
        headers
    }

    /// The kept rejected-packet samples of the latest session that rejected a packet, oldest
    /// first.
    #[must_use]
    pub fn samples(&self) -> Vec<FailureSample> {
        self.lock_samples().samples().to_vec()
    }

    /// Locks the in-memory snapshot for readers outside ingress.
    fn lock_samples(&self) -> MutexGuard<'_, FailureSamples> {
        self.samples.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The instant [`global`] header times count from: the first session's start.
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Starts a session's tap into [`global`] under a new session number, with a full capture when
/// `capture` asks for one. A capture that cannot start is logged and the session continues
/// without it; one that starts comes with the finisher session teardown ends it with.
pub(crate) fn session_tap(
    capture: Option<&PacketCaptureRequest>,
) -> (Arc<dyn PacketTap>, Option<CaptureFinisher>) {
    let epoch = *EPOCH;
    let (feed, finisher) = match capture
        .map(|request| (request, CaptureFeed::start(request, Instant::now())))
    {
        Some((_, Ok((feed, finisher, path)))) => {
            tracing::info!(path = %path.display(), "recording a full packet capture");
            (Some(feed), Some(finisher))
        }
        Some((request, Err(error))) => {
            tracing::warn!(path = %request.path.display(), %error, "packet capture unavailable");
            (None, None)
        }
        None => (None, None),
    };
    let log = global();
    let tap = SessionTap::new(log, log.begin_session(), epoch, feed);
    (Arc::new(tap), finisher)
}

/// A tap into [`global`] for a new session without capture, as a session records through, for
/// fixtures that stand in for one.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn fixture_tap() -> Arc<dyn PacketTap> {
    session_tap(None).0
}

/// Waits until [`global`]'s rejected-packet samples are on disk, for fixtures that crash right
/// after rejecting a packet; production accepts losing samples the writer had not reached.
#[cfg(any(test, feature = "test-support"))]
pub fn fixture_flush_samples() {
    if let Some(writer) = global().samples_writer.get() {
        writer.flush();
    }
}

/// One session's [`PacketTap`]: headers go to a [`PacketLog`], and packets to its capture.
pub(crate) struct SessionTap<'log> {
    log: &'log PacketLog,
    session: u64,
    epoch: Instant,
    capture: Option<CaptureFeed>,
}

impl<'log> SessionTap<'log> {
    /// A tap for session number `session` of `log`, timing headers from `epoch`.
    pub(crate) fn new(
        log: &'log PacketLog,
        session: u64,
        epoch: Instant,
        capture: Option<CaptureFeed>,
    ) -> Self {
        Self {
            log,
            session,
            epoch,
            capture,
        }
    }

    /// Returns monotonic milliseconds since this tap began.
    fn elapsed_ms(&self, now: Instant) -> u64 {
        millis(now.saturating_duration_since(self.epoch))
    }
}

impl PacketTap for SessionTap<'_> {
    fn record(
        &self,
        direction: PacketDirection,
        packet_id: u32,
        body: PacketBody<'_>,
    ) -> PacketRecordId {
        let now = Instant::now();
        let sequence = self.log.record(
            direction,
            packet_id,
            body.len(),
            self.elapsed_ms(now),
            PacketOutcome::Accepted,
        );
        if let Some(capture) = &self.capture
            && !carries_secrets(packet_id)
        {
            capture.packet(direction, packet_id, now, body);
        }
        PacketRecordId(sequence)
    }

    fn reject(
        &self,
        record: PacketRecordId,
        packet_id: u32,
        outcome: PacketOutcome,
        body: PacketBody<'_>,
    ) {
        self.log
            .reject(self.session, record.0, packet_id, outcome, body.as_slice());
    }

    fn reject_batch(&self, len: usize) {
        self.log.record(
            PacketDirection::Clientbound,
            UNFRAMED_BATCH_ID,
            len,
            self.elapsed_ms(Instant::now()),
            PacketOutcome::Failed,
        );
    }
}

/// Converts elapsed time without overflowing the wire field.
fn millis(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
