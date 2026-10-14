//! The always-on packet flight recorder and opt-in full capture at session ingress and egress.
//!
//! Every session reports its packets to [`global`]: a header ring of the last
//! [`RING_CAPACITY`] packets in both directions, and the raw bodies of packets ingress rejected.
//! Recording a header neither allocates nor formats; only a rejected packet's body is copied.
//! Packets that can carry credentials or pack content keys keep their header but never their
//! body, in samples or in captures.

mod capture;
mod ring;
mod samples;

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use protocol::packet_tap::{
    PacketBody, PacketDirection, PacketName, PacketOutcome, PacketRecordId, PacketTap,
    UNFRAMED_BATCH_ID, carries_secrets,
};

pub(crate) use capture::CaptureFeed;
pub use capture::{CAPTURE_EXTENSION, CAPTURE_QUEUE_RECORDS, PacketCaptureRequest};
use ring::HeaderRing;
pub use ring::{HeaderRecord, RING_CAPACITY};
use samples::FailureSamples;
pub use samples::{FailureSample, MAX_SAMPLE_BYTES, MAX_SAMPLES_PER_PACKET};

/// Packet headers and rejected bodies recorded by this process's sessions.
pub struct PacketLog {
    ring: HeaderRing,
    samples: Mutex<FailureSamples>,
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
        }
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

    /// Records that ingress rejected record `sequence` with `outcome`, keeping its body as a
    /// sample within the sample bounds.
    pub(crate) fn reject(
        &self,
        sequence: u64,
        packet_id: u32,
        outcome: PacketOutcome,
        body: &[u8],
    ) {
        self.ring.settle(sequence, outcome);
        self.lock_samples()
            .offer(sequence, packet_id, outcome, body);
    }

    /// The retained headers, oldest first. A header overwritten while it is read is left out.
    #[must_use]
    pub fn headers(&self) -> Vec<HeaderRecord> {
        let mut headers = Vec::with_capacity(RING_CAPACITY);
        self.ring.for_each(|record| headers.push(record));
        headers
    }

    /// The kept rejected-packet samples, oldest first.
    #[must_use]
    pub fn samples(&self) -> Vec<FailureSample> {
        self.lock_samples().samples().to_vec()
    }

    /// Writes the retained headers as a text table, oldest first, followed by a summary of the
    /// rejected-packet samples.
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
        let samples = self.lock_samples();
        writeln!(
            out,
            "rejected packet samples: {} kept ({} bytes), {} withheld as credential-bearing, {} over the bounds",
            samples.samples().len(),
            samples.bytes(),
            samples.withheld(),
            samples.dropped(),
        )
    }

    /// Writes each kept sample's body to `dir` as `<sequence>-<packet name>.bin`, with an
    /// `index.txt` describing them, and returns how many bodies were written.
    pub fn dump_samples(&self, dir: &Path) -> io::Result<usize> {
        let samples = self.samples();
        fs::create_dir_all(dir)?;
        let mut index = Vec::new();
        for sample in &samples {
            let name = format!("{}-{}.bin", sample.sequence, PacketName(sample.packet_id));
            fs::write(dir.join(&name), &sample.body)?;
            writeln!(
                index,
                "{name}: {} ({}) {}, {} of {} bytes",
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
}

/// The instant [`global`] header times count from: the first session's start.
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Starts a session's tap into [`global`], with a full capture when `capture` asks for one.
/// A capture that cannot start is logged and the session continues without it.
pub(crate) fn session_tap(capture: Option<&PacketCaptureRequest>) -> Arc<dyn PacketTap> {
    let epoch = *EPOCH;
    let capture = capture.and_then(|request| {
        match CaptureFeed::start(request, Instant::now()) {
            Ok((feed, path, _writer)) => {
                tracing::info!(path = %path.display(), "recording a full packet capture");
                Some(feed)
            }
            Err(error) => {
                tracing::warn!(path = %request.path.display(), %error, "packet capture unavailable");
                None
            }
        }
    });
    Arc::new(SessionTap::new(global(), epoch, capture))
}

/// One session's [`PacketTap`]: headers go to a [`PacketLog`], and packets to its capture.
pub(crate) struct SessionTap<'log> {
    log: &'log PacketLog,
    epoch: Instant,
    capture: Option<CaptureFeed>,
}

impl<'log> SessionTap<'log> {
    pub(crate) fn new(log: &'log PacketLog, epoch: Instant, capture: Option<CaptureFeed>) -> Self {
        Self {
            log,
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
            .reject(record.0, packet_id, outcome, body.as_slice());
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
