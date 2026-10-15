//! Opt-in full capture of one session's packets, written on a background thread.
//!
//! The network tasks hand each packet to a bounded queue as a shared view of its batch; they
//! never copy a body, wait for the disk or block on a full queue. A packet that finds the queue
//! full is dropped and counted, and the next packet that fits is preceded by a gap record
//! carrying that count; packets dropped after the last gap are counted in a gap the writer
//! appends when the capture finishes.
//!
//! The writer passes at most [`CAPTURE_BUFFER_BYTES`] at a time to the file and flushes
//! whenever its queue runs empty, so a crash loses at most that much plus the queue, and the
//! file then ends inside a record, which readers drop. At session end, [`CaptureFinisher`]
//! asks it to write what is queued, append the last gap and flush, waiting a bounded time.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError};
use protocol::capture::{CaptureHeader, CaptureRecord, CaptureWriter};
use protocol::packet_tap::{PacketBody, PacketDirection};

/// Records waiting for the writer thread; past this, packets are dropped and counted.
pub const CAPTURE_QUEUE_RECORDS: usize = 8192;
/// Encoded bytes the writer holds before passing them to the file.
pub const CAPTURE_BUFFER_BYTES: usize = 64 << 10;
/// Extension of capture v2 files.
pub const CAPTURE_EXTENSION: &str = "cnbrcap";
/// Captures the log directory keeps, the one being started included.
pub const LOG_DIR_CAPTURES_KEPT: usize = 10;
/// Bytes the log directory's captures may take, the one being started counted at its size cap.
pub const LOG_DIR_CAPTURE_BYTES: u64 = 2 << 30;
/// How long session teardown waits for the writer to finish the file.
pub(crate) const CAPTURE_FINISH_TIMEOUT: Duration = Duration::from_secs(2);
/// The name every capture in the log directory starts with.
const LOG_DIR_CAPTURE_PREFIX: &str = "packets-";

/// Where one session's capture goes, and the wall-clock time its header records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PacketCaptureRequest {
    pub path: PathBuf,
    pub started_unix_ms: u64,
    /// The capture is one of the log directory's `packets-<started_unix_ms>.cnbrcap` files, so
    /// the oldest of those are deleted before it starts to keep within
    /// [`LOG_DIR_CAPTURES_KEPT`] and [`LOG_DIR_CAPTURE_BYTES`]. A path the user named is never
    /// pruned.
    pub in_log_dir: bool,
}

impl PacketCaptureRequest {
    /// Resolves the value of [`crate::markers::PACKET_CAPTURE`]. Unset, empty and `0` disable
    /// capture; `1` names `packets-<started_unix_ms>.cnbrcap` in `log_dir`; any other value is
    /// the capture file's path.
    #[must_use]
    pub fn from_setting(
        value: Option<&OsStr>,
        log_dir: &Path,
        started_unix_ms: u64,
    ) -> Option<Self> {
        let value = value.filter(|value| !value.is_empty() && *value != "0")?;
        let in_log_dir = value == "1";
        let path = if in_log_dir {
            log_dir.join(format!(
                "{LOG_DIR_CAPTURE_PREFIX}{started_unix_ms}.{CAPTURE_EXTENSION}"
            ))
        } else {
            PathBuf::from(value)
        };
        Some(Self {
            path,
            started_unix_ms,
            in_log_dir,
        })
    }
}

/// The network side of a running capture.
pub(crate) struct CaptureFeed {
    sender: Sender<CaptureRecord>,
    started: Instant,
    dropped: Arc<AtomicU64>, // packets dropped since the last gap record
}

/// The writer side of a capture: queued records, the stop request and the drop count.
pub(crate) struct CaptureQueue {
    records: Receiver<CaptureRecord>,
    stop: Receiver<()>,
    started: Instant,
    dropped: Arc<AtomicU64>,
}

/// Ends a capture from session teardown.
pub(crate) struct CaptureFinisher {
    stop: Sender<()>,
    done: Receiver<()>,
}

/// A feed into a queue of `capacity` records whose timestamps count from `started`, and the
/// sender that stops the queue's writer, which dropping also does.
pub(crate) fn channel(
    capacity: usize,
    started: Instant,
) -> (CaptureFeed, CaptureQueue, Sender<()>) {
    let (sender, records) = crossbeam_channel::bounded(capacity);
    let (stop_sender, stop) = crossbeam_channel::bounded(1);
    let dropped = Arc::new(AtomicU64::new(0));
    let feed = CaptureFeed {
        sender,
        started,
        dropped: Arc::clone(&dropped),
    };
    let queue = CaptureQueue {
        records,
        stop,
        started,
        dropped,
    };
    (feed, queue, stop_sender)
}

impl CaptureFeed {
    /// Creates the capture file and starts its writer thread. An existing file is never
    /// replaced: the start time is added to the name instead. A capture in the log directory
    /// first deletes that directory's oldest captures past its retention. Returns the feed, the
    /// finisher that ends the capture and the path written.
    pub(crate) fn start(
        request: &PacketCaptureRequest,
        started: Instant,
    ) -> io::Result<(Self, CaptureFinisher, PathBuf)> {
        if request.in_log_dir
            && let Some(dir) = request.path.parent()
            && let Err(error) = prune_log_dir_captures(dir)
        {
            tracing::warn!(dir = %dir.display(), %error, "could not delete old packet captures");
        }
        let (path, file) = create_capture_file(&request.path, request.started_unix_ms)?;
        let writer = CaptureWriter::new(
            BufWriter::with_capacity(CAPTURE_BUFFER_BYTES, file),
            CaptureHeader {
                protocol_version: protocol::PROTOCOL_VERSION,
                started_unix_ms: request.started_unix_ms,
            },
        )?;
        let (feed, queue, stop) = channel(CAPTURE_QUEUE_RECORDS, started);
        let (done_sender, done) = crossbeam_channel::bounded::<()>(0);
        let shown_path = path.display().to_string();
        thread::Builder::new()
            .name("packet-capture".to_owned())
            .spawn(move || {
                if let Err(error) = queue.drain(writer) {
                    tracing::warn!(path = %shown_path, %error, "packet capture stopped");
                }
                drop(done_sender);
            })?;
        Ok((feed, CaptureFinisher { stop, done }, path))
    }

    /// Queues one packet seen at `now` without waiting; the body stays a view of its batch.
    pub(crate) fn packet(
        &self,
        direction: PacketDirection,
        packet_id: u32,
        now: Instant,
        body: PacketBody<'_>,
    ) {
        let elapsed_us = micros(now.saturating_duration_since(self.started));
        let dropped = self.dropped.swap(0, Ordering::Relaxed);
        if dropped != 0 {
            match self.sender.try_send(CaptureRecord::Gap {
                elapsed_us,
                dropped,
            }) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    self.dropped.fetch_add(dropped + 1, Ordering::Relaxed);
                    return;
                }
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
        let record = CaptureRecord::Packet {
            direction,
            packet_id,
            elapsed_us,
            body: body.to_shared(),
        };
        if let Err(TrySendError::Full(_)) = self.sender.try_send(record) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl CaptureQueue {
    /// Writes queued records until every feed is gone, a stop is requested or the size limit
    /// ends the capture. On a stop it writes the records already queued. It then appends a gap
    /// for packets dropped since the last one, flushes and returns the underlying writer.
    pub(crate) fn drain<W: Write>(self, mut writer: CaptureWriter<W>) -> io::Result<W> {
        loop {
            crossbeam_channel::select! {
                recv(self.records) -> record => {
                    let Ok(record) = record else { break };
                    if !writer.write(&record)? {
                        break;
                    }
                    if self.records.is_empty() {
                        writer.flush()?;
                    }
                }
                recv(self.stop) -> _ => {
                    for record in self.records.try_iter() {
                        if !writer.write(&record)? {
                            break;
                        }
                    }
                    break;
                }
            }
        }
        let dropped = self.dropped.swap(0, Ordering::Relaxed);
        if dropped != 0 {
            writer.write(&CaptureRecord::Gap {
                elapsed_us: micros(self.started.elapsed()),
                dropped,
            })?;
        }
        writer.flush()?;
        Ok(writer.into_inner())
    }
}

impl CaptureFinisher {
    /// Asks the writer to finish the file and waits at most `within` for it; returns whether
    /// it finished. Feeds still held elsewhere may keep queuing, but nothing more is written.
    pub(crate) fn finish(self, within: Duration) -> bool {
        let _ = self.stop.try_send(());
        matches!(
            self.done.recv_timeout(within),
            Err(RecvTimeoutError::Disconnected)
        )
    }
}

#[cfg(test)]
impl CaptureQueue {
    /// Takes the queued records without writing them.
    pub(crate) fn take_queued(&self) -> Vec<CaptureRecord> {
        self.records.try_iter().collect()
    }
}

/// Creates `path` exclusively, readable only by its owner on Unix; when it exists, creates
/// `<stem>-<started_unix_ms>.<extension>` beside it instead.
fn create_capture_file(path: &Path, started_unix_ms: u64) -> io::Result<(PathBuf, File)> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    match create_new(path) {
        Ok(file) => Ok((path.to_owned(), file)),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            let extension = path
                .extension()
                .map_or(CAPTURE_EXTENSION.into(), OsStr::to_string_lossy);
            let path = path.with_file_name(format!("{stem}-{started_unix_ms}.{extension}"));
            create_new(&path).map(|file| (path, file))
        }
        Err(error) => Err(error),
    }
}

/// Deletes the oldest `packets-<unix ms>[-<unix ms>].cnbrcap` files in `dir` so that a new
/// capture leaves at most [`LOG_DIR_CAPTURES_KEPT`] and, with the new one at its size cap,
/// [`LOG_DIR_CAPTURE_BYTES`].
fn prune_log_dir_captures(dir: &Path) -> io::Result<()> {
    prune_captures(
        dir,
        LOG_DIR_CAPTURES_KEPT - 1,
        LOG_DIR_CAPTURE_BYTES - protocol::capture::MAX_CAPTURE_BYTES,
    )
}

/// Keeps the newest of the log directory's captures in `dir` while both `count_budget` and
/// `byte_budget` hold, then deletes the first that would break either and every older one, as
/// session logs are pruned. Tries every deletion and reports the first failure.
pub(crate) fn prune_captures(dir: &Path, count_budget: usize, byte_budget: u64) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut captures = Vec::new();
    for entry in entries {
        let entry = entry?;
        if let Some(key) = log_dir_capture_key(&entry.file_name().to_string_lossy()) {
            captures.push((key, entry.path()));
        }
    }
    captures.sort_unstable();
    let (mut kept, mut bytes, mut full) = (0usize, 0u64, false);
    let mut failure = None;
    for (_, path) in captures.into_iter().rev() {
        let size = fs::metadata(&path).map_or(0, |metadata| metadata.len());
        full = full || kept >= count_budget || bytes + size > byte_budget;
        if !full {
            kept += 1;
            bytes += size;
            continue;
        }
        match fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                failure.get_or_insert(error);
            }
            _ => {}
        }
    }
    failure.map_or(Ok(()), Err)
}

/// Orders the log directory's capture names by start time, then by the collision suffix
/// [`create_capture_file`] adds; `None` for any other name.
fn log_dir_capture_key(name: &str) -> Option<(u64, u64)> {
    let stamps = name
        .strip_prefix(LOG_DIR_CAPTURE_PREFIX)?
        .strip_suffix(CAPTURE_EXTENSION)?
        .strip_suffix('.')?;
    let number = |digits: &str| {
        (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| digits.parse().ok())
            .flatten()
    };
    match stamps.split_once('-') {
        Some((started, suffix)) => Some((number(started)?, number(suffix)?)),
        None => Some((number(stamps)?, 0)),
    }
}

fn create_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn micros(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
}
