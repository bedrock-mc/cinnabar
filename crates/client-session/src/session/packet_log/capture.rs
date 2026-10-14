//! Opt-in full capture of one session's packets, written on a background thread.
//!
//! The network tasks hand each packet to a bounded queue as a shared view of its batch; they
//! never copy a body or wait for the disk. When the queue is full the packet is dropped and
//! counted, and the next packet that fits is preceded by a gap record carrying that count.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use protocol::capture::{CaptureHeader, CaptureRecord, CaptureWriter};
use protocol::packet_tap::{PacketBody, PacketDirection};

/// Records waiting for the writer thread; past this, packets are dropped and counted.
pub const CAPTURE_QUEUE_RECORDS: usize = 8192;
/// Extension of capture v2 files.
pub const CAPTURE_EXTENSION: &str = "cnbrcap";

/// Where one session's capture goes, and the wall-clock time its header records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PacketCaptureRequest {
    pub path: PathBuf,
    pub started_unix_ms: u64,
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
        let path = if value == "1" {
            log_dir.join(format!("packets-{started_unix_ms}.{CAPTURE_EXTENSION}"))
        } else {
            PathBuf::from(value)
        };
        Some(Self {
            path,
            started_unix_ms,
        })
    }
}

/// The network side of a running capture.
pub(crate) struct CaptureFeed {
    sender: Sender<CaptureRecord>,
    started: Instant,
    dropped: AtomicU64, // packets dropped since the last gap record
}

impl CaptureFeed {
    /// Creates the capture file and starts its writer thread. An existing file is never
    /// replaced: the start time is added to the name instead. Returns the feed, the path
    /// written and the writer thread, which ends once the feed is dropped and every queued
    /// record is written, or when the size limit ends the capture.
    pub(crate) fn start(
        request: &PacketCaptureRequest,
        started: Instant,
    ) -> io::Result<(Self, PathBuf, JoinHandle<()>)> {
        let (path, file) = create_capture_file(&request.path, request.started_unix_ms)?;
        let writer = CaptureWriter::new(
            BufWriter::new(file),
            CaptureHeader {
                protocol_version: protocol::PROTOCOL_VERSION,
                started_unix_ms: request.started_unix_ms,
            },
        )?;
        let (sender, receiver) = crossbeam_channel::bounded(CAPTURE_QUEUE_RECORDS);
        let display = path.display().to_string();
        let thread = thread::Builder::new()
            .name("packet-capture".to_owned())
            .spawn(move || {
                if let Err(error) = drain(&receiver, writer) {
                    tracing::warn!(path = %display, %error, "packet capture stopped");
                }
            })?;
        Ok((Self::new(sender, started), path, thread))
    }

    /// A feed into `sender` whose timestamps count from `started`.
    pub(crate) fn new(sender: Sender<CaptureRecord>, started: Instant) -> Self {
        Self {
            sender,
            started,
            dropped: AtomicU64::new(0),
        }
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

impl Drop for CaptureFeed {
    /// Records the packets dropped since the last gap, waiting for room if it must: the
    /// session is over, and the count is the only trace of them.
    fn drop(&mut self) {
        let dropped = *self.dropped.get_mut();
        if dropped != 0 {
            let _ = self.sender.send(CaptureRecord::Gap {
                elapsed_us: micros(self.started.elapsed()),
                dropped,
            });
        }
    }
}

/// Writes queued records until every feed is gone or the size limit ends the capture,
/// flushing whenever the queue runs empty. Returns the underlying writer.
pub(crate) fn drain<W: Write>(
    receiver: &Receiver<CaptureRecord>,
    mut writer: CaptureWriter<W>,
) -> io::Result<W> {
    while let Ok(record) = receiver.recv() {
        if !writer.write(&record)? {
            break;
        }
        if receiver.is_empty() {
            writer.flush()?;
        }
    }
    writer.flush()?;
    Ok(writer.into_inner())
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
