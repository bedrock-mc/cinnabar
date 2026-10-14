//! The always-on packet flight recorder and opt-in full capture at session ingress and egress.
//!
//! Every session reports its packets to [`global`]: a header ring of the last
//! [`RING_CAPACITY`] packets in both directions, and the raw bodies of packets ingress rejected
//! in the latest session that rejected any. Recording a header neither allocates nor formats;
//! only a rejected packet's body is copied. Packets that can carry credentials or pack content
//! keys keep their header but never their body, in samples or in captures.

mod capture;
mod ring;
mod samples;

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use protocol::packet_tap::{
    PacketBody, PacketDirection, PacketName, PacketOutcome, PacketRecordId, PacketTap,
    UNFRAMED_BATCH_ID, carries_secrets,
};

pub use capture::{
    CAPTURE_BUFFER_BYTES, CAPTURE_EXTENSION, CAPTURE_QUEUE_RECORDS, PacketCaptureRequest,
};
pub(crate) use capture::{CAPTURE_FINISH_TIMEOUT, CaptureFeed, CaptureFinisher};
use ring::HeaderRing;
pub use ring::{HeaderRecord, RING_CAPACITY};
use samples::FailureSamples;
pub use samples::{FailureSample, MAX_SAMPLE_BYTES, MAX_SAMPLES_PER_PACKET};

/// Packet headers and rejected bodies recorded by this process's sessions.
pub struct PacketLog {
    ring: HeaderRing,
    samples: Mutex<FailureSamples>,
    sessions: AtomicU64, // sessions begun so far
}

static PACKET_LOG: PacketLog = PacketLog::new();

/// The process-wide packet log every session records to.
#[must_use]
pub fn global() -> &'static PacketLog {
    &PACKET_LOG
}

impl PacketLog {
    pub(crate) const fn new() -> Self {
        Self {
            ring: HeaderRing::new(),
            samples: Mutex::new(FailureSamples::new()),
            sessions: AtomicU64::new(0),
        }
    }

    /// Numbers a new session, from 1; its rejected bodies replace older sessions' samples.
    pub(crate) fn begin_session(&self) -> u64 {
        self.sessions.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Appends a header, returning its sequence number. `elapsed_ms` is the caller's monotonic
    /// time since its epoch.
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
            .record(direction, packet_id, len, elapsed_ms, outcome)
    }

    /// Records that ingress in `session` rejected record `sequence` with `outcome`, keeping its
    /// body as a sample within that session's sample bounds.
    pub(crate) fn reject(
        &self,
        session: u64,
        sequence: u64,
        packet_id: u32,
        outcome: PacketOutcome,
        body: &[u8],
    ) {
        self.ring.settle(sequence, outcome);
        self.lock_samples()
            .offer(session, sequence, packet_id, outcome, body);
    }

    /// The retained headers, oldest first. A header overwritten while it is read is left out.
    #[must_use]
    pub fn headers(&self) -> Vec<HeaderRecord> {
        let mut headers = Vec::with_capacity(RING_CAPACITY);
        self.ring.for_each(|record| headers.push(record));
        headers
    }

    /// The kept rejected-packet samples of the latest session that rejected a packet, oldest
    /// first.
    #[must_use]
    pub fn samples(&self) -> Vec<FailureSample> {
        self.lock_samples().samples().to_vec()
    }

    /// Writes the retained headers as a text table, oldest first, followed by a summary of the
    /// latest session's rejected-packet samples. Never waits: samples another thread is
    /// updating, as when the crash interrupted it, are reported as unavailable.
    pub fn dump(&self, out: &mut impl Write) -> io::Result<()> {
        let recorded = self.ring.recorded();
        writeln!(
            out,
            "packet headers: {recorded} recorded, last {} kept",
            recorded.min(RING_CAPACITY as u64)
        )?;
        writeln!(
            out,
            "{:>10}  {:>10}  {:<11}  {:<8}  {:>9}  packet",
            "sequence", "elapsed_ms", "direction", "outcome", "bytes"
        )?;
        let mut result = Ok(());
        self.ring.for_each(|record| {
            if result.is_ok() {
                result = writeln!(
                    out,
                    "{:>10}  {:>10}  {:<11}  {:<8}  {:>9}  {} ({})",
                    record.sequence,
                    record.elapsed_ms,
                    Direction(record.direction),
                    Outcome(record.outcome),
                    record.len,
                    PacketName(record.packet_id),
                    record.packet_id,
                );
            }
        });
        result?;
        let Some(samples) = self.try_lock_samples() else {
            return writeln!(
                out,
                "rejected packet samples: unavailable while being updated"
            );
        };
        writeln!(
            out,
            "rejected packet samples from session {}: {} kept ({} bytes), {} withheld as credential-bearing, {} over the bounds",
            samples.session(),
            samples.samples().len(),
            samples.bytes(),
            samples.withheld(),
            samples.dropped(),
        )
    }

    /// Writes each kept sample's body to `dir` as `<sequence>-<packet name>.bin`, with an
    /// `index.txt` describing them, and returns how many bodies were written. Like
    /// [`Self::dump`], never waits for a thread updating the samples; it then writes none.
    pub fn dump_samples(&self, dir: &Path) -> io::Result<usize> {
        let Some(samples) = self
            .try_lock_samples()
            .map(|samples| samples.samples().to_vec())
        else {
            return Ok(0);
        };
        fs::create_dir_all(dir)?;
        let mut index = Vec::new();
        for sample in &samples {
            let name = format!("{}-{}.bin", sample.sequence, PacketName(sample.packet_id));
            fs::write(dir.join(&name), &sample.body)?;
            writeln!(
                index,
                "{name}: session {}, {} ({}) {}, {} of {} bytes",
                sample.session,
                PacketName(sample.packet_id),
                sample.packet_id,
                Outcome(sample.outcome),
                sample.body.len(),
                sample.len,
            )?;
        }
        fs::write(dir.join("index.txt"), index)?;
        Ok(samples.len())
    }

    fn lock_samples(&self) -> MutexGuard<'_, FailureSamples> {
        self.samples.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn try_lock_samples(&self) -> Option<MutexGuard<'_, FailureSamples>> {
        match self.samples.try_lock() {
            Ok(samples) => Some(samples),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
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

fn millis(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

struct Direction(PacketDirection);

impl fmt::Display for Direction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.pad(match self.0 {
            PacketDirection::Clientbound => "clientbound",
            PacketDirection::Serverbound => "serverbound",
        })
    }
}

struct Outcome(PacketOutcome);

impl fmt::Display for Outcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.pad(match self.0 {
            PacketOutcome::Accepted => "accepted",
            PacketOutcome::Skipped => "skipped",
            PacketOutcome::Failed => "failed",
        })
    }
}

#[cfg(test)]
mod tests;
