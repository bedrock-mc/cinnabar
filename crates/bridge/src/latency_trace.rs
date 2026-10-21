//! Timestamped packet-latency events, written beside the core's own trace for offline joining.
//!
//! Off unless [`crate::contract::LATENCY_TRACE_ENV`] names a directory. Both processes stamp
//! events with Unix nanoseconds read once at startup plus monotonic time since, so intervals
//! within a process are monotonic and the two files share one epoch.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;
use web_time::{Instant, SystemTime, UNIX_EPOCH};

/// Event kinds and their fields; times are trace-clock nanoseconds.
pub mod kind {
    /// Batch frame read from the core: conn, frame, t, bytes.
    pub const FRAME_READ: u8 = b'b';
    /// Batch frame flushed to the core: conn, frame, t its bytes were fed to the socket, bytes.
    pub const FRAME_WRITTEN: u8 = b'w';
    /// Packet decoded into world ingress: conn, frame, index, session, fifo, t.
    pub const DECODED: u8 = b'd';
    /// World ingress handed to the app's channel: session, fifo, t.
    pub const ENQUEUED: u8 = b'e';
    /// World ingress drained by the app: session, fifo, t.
    pub const DRAINED: u8 = b'a';
    /// World event committed to client state: fifo, t.
    pub const COMMITTED: u8 = b'c';
    /// Render world extracted the main frame: frame count, t.
    pub const EXTRACTED: u8 = b'x';
    /// Render world finished presenting that frame: frame count, t.
    pub const PRESENTED: u8 = b'p';
    /// PlayerAuthInput admitted to the outbound queue: tick, original sampling frame time, t.
    pub const TICK_SENT: u8 = b'k';
    /// Outbound frame batch handed to the bridge: t begin, t flushed, packets.
    pub const OUTBOUND_FLUSH: u8 = b'f';
    /// Chunk mesh upload acknowledged: dirty since, uploaded at.
    pub const MESH_UPLOADED: u8 = b'g';
}

const QUEUE_CAPACITY: usize = 1 << 16;
const FLUSH_INTERVAL: Duration = Duration::from_millis(200);

/// One process's open trace.
pub struct Trace {
    anchor: Instant,
    anchor_unix: i64,
    records: Mutex<Option<SyncSender<Record>>>,
    dropped: AtomicU64,
    writer: Mutex<Option<JoinHandle<()>>>,
}

struct Record {
    kind: u8,
    values: [i64; 6],
    len: u8,
}

static ACTIVE: OnceLock<Option<Trace>> = OnceLock::new();

/// The process trace, or `None` when tracing is off.
#[must_use]
pub fn active() -> Option<&'static Trace> {
    ACTIVE
        .get_or_init(|| {
            if cfg!(target_arch = "wasm32") {
                return None;
            }
            let dir = std::env::var_os(crate::contract::LATENCY_TRACE_ENV)?;
            Trace::open(Path::new(&dir), "client")
                .inspect_err(|error| tracing::warn!(%error, "latency trace disabled"))
                .ok()
        })
        .as_ref()
}

/// Writes and closes the process trace if one was opened; later events are dropped.
pub fn finish() {
    if let Some(Some(trace)) = ACTIVE.get() {
        trace.finish();
    }
}

impl Trace {
    /// Starts `<role>-<pid>.trace` in `dir` with its own writer thread.
    pub fn open(dir: &Path, role: &str) -> std::io::Result<Self> {
        let file = File::create(dir.join(format!("{role}-{}.trace", std::process::id())))?;
        let anchor = Instant::now();
        let anchor_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos() as i64);
        let (records, queue) = sync_channel(QUEUE_CAPACITY);
        let header = format!(
            "# {role} pid={} anchor_unix_ns={anchor_unix}\n",
            std::process::id()
        );
        let writer = std::thread::Builder::new()
            .name("latency-trace".into())
            .spawn(move || write_records(BufWriter::new(file), &header, &queue))?;
        Ok(Self {
            anchor,
            anchor_unix,
            records: Mutex::new(Some(records)),
            dropped: AtomicU64::new(0),
            writer: Mutex::new(Some(writer)),
        })
    }

    /// The trace clock now.
    #[must_use]
    pub fn now(&self) -> i64 {
        self.at(Instant::now())
    }

    /// `instant` on the trace clock; instants before the anchor saturate to it.
    #[must_use]
    pub fn at(&self, instant: Instant) -> i64 {
        self.anchor_unix + instant.saturating_duration_since(self.anchor).as_nanos() as i64
    }

    /// Queues an event without blocking; a full queue drops it and counts the drop.
    pub fn record(&self, kind: u8, values: &[i64]) {
        let mut record = Record {
            kind,
            values: [0; 6],
            len: values.len().min(6) as u8,
        };
        record.values[..record.len as usize].copy_from_slice(&values[..record.len as usize]);
        let sender = self
            .records
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(sender) = sender.as_ref() else {
            return;
        };
        if let Err(TrySendError::Full(_)) = sender.try_send(record) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Runs `admit` and records `kind` with `values` and the time it began only if it succeeded,
    /// so a rejected attempt that is retried later leaves no record.
    pub fn record_admitted<T, E>(
        &self,
        kind: u8,
        values: &[i64],
        admit: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let began = self.now();
        let result = admit();
        if result.is_ok() {
            let mut fields = values.to_vec();
            fields.push(began);
            self.record(kind, &fields);
        }
        result
    }

    /// Writes every queued event, closes the file and stops recording.
    pub fn finish(&self) {
        drop(
            self.records
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take(),
        );
        let writer = self
            .writer
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        if let Some(writer) = writer {
            let _ = writer.join();
        }
        if self.dropped.load(Ordering::Relaxed) != 0 {
            tracing::warn!(
                dropped = self.dropped.load(Ordering::Relaxed),
                "latency trace dropped events"
            );
        }
    }
}

/// Formats each record as `kind v0 v1 ...`, flushing every [`FLUSH_INTERVAL`] however busy the
/// queue is, and once more when every sender is gone.
fn write_records(mut out: impl Write, header: &str, queue: &Receiver<Record>) {
    let _ = out.write_all(header.as_bytes());
    let mut flush_at = Instant::now() + FLUSH_INTERVAL;
    loop {
        let now = Instant::now();
        if now >= flush_at {
            let _ = out.flush();
            flush_at = now + FLUSH_INTERVAL;
        }
        match queue.recv_timeout(flush_at.saturating_duration_since(now)) {
            Ok(record) => {
                let _ = write!(out, "{}", record.kind as char);
                for value in &record.values[..record.len as usize] {
                    let _ = write!(out, " {value}");
                }
                let _ = out.write_all(b"\n");
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts flushes, so a test can see them while records keep arriving.
    struct CountingWriter(std::sync::Arc<AtomicU64>);

    impl Write for CountingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    /// A queue that never idles for a whole interval still flushes on the interval's deadline.
    #[test]
    fn busy_queue_still_flushes_on_its_deadline() {
        let flushes = std::sync::Arc::new(AtomicU64::new(0));
        let (records, queue) = sync_channel(QUEUE_CAPACITY);
        let writer = {
            let flushes = std::sync::Arc::clone(&flushes);
            std::thread::spawn(move || write_records(CountingWriter(flushes), "", &queue))
        };
        let record = || Record {
            kind: kind::COMMITTED,
            values: [0; 6],
            len: 0,
        };
        test_time::eventually("a flush while records keep arriving", || {
            records.send(record()).unwrap();
            flushes.load(Ordering::Relaxed) > 0
        });
        drop(records);
        writer.join().unwrap();
    }

    /// An admission that fails leaves no record; one that succeeds records the time it began,
    /// before anything the admission publishes can be observed.
    #[test]
    fn only_admitted_attempts_are_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let trace = Trace::open(dir.path(), "admit").unwrap();
        assert!(
            trace
                .record_admitted(kind::TICK_SENT, &[4, 1], || Err::<(), _>("full"))
                .is_err()
        );
        trace
            .record_admitted(kind::TICK_SENT, &[4, 1], || Ok::<_, ()>(()))
            .unwrap();
        let mut admitted_at = 0;
        trace
            .record_admitted(kind::ENQUEUED, &[1, 2], || {
                admitted_at = trace.now();
                Ok::<_, ()>(())
            })
            .unwrap();
        trace.finish();
        let text = std::fs::read_to_string(
            dir.path()
                .join(format!("admit-{}.trace", std::process::id())),
        )
        .unwrap();
        let ticks: Vec<_> = text.lines().filter(|line| line.starts_with('k')).collect();
        assert_eq!(ticks.len(), 1);
        assert!(ticks[0].starts_with("k 4 1 "));
        let enqueued: i64 = text
            .lines()
            .find_map(|line| line.strip_prefix("e 1 2 "))
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            enqueued <= admitted_at,
            "a consumer acting inside the admission must not precede the recorded time"
        );
    }

    /// Records reach the file in order with their fields, on a clock past the anchor.
    #[test]
    fn finished_trace_holds_every_record_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let trace = Trace::open(dir.path(), "test").unwrap();
        let start = trace.now();
        trace.record(kind::FRAME_READ, &[1, 2, start, 40]);
        trace.record(kind::COMMITTED, &[7, trace.now()]);
        trace.finish();
        trace.record(kind::COMMITTED, &[8, 0]);

        let path = dir
            .path()
            .join(format!("test-{}.trace", std::process::id()));
        let text = std::fs::read_to_string(path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("# test pid="));
        assert_eq!(lines[1], format!("b 1 2 {start} 40"));
        let committed: Vec<i64> = lines[2][2..]
            .split(' ')
            .map(|field| field.parse().unwrap())
            .collect();
        assert_eq!(committed[0], 7);
        assert!(committed[1] >= start);
    }
}
