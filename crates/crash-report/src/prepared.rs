//! Crash folders written from a Unix signal handler: everything that allocates is prepared
//! beforehand, and [`Prepared::write`] uses only async-signal-safe calls.

use std::{ffi::CString, os::unix::ffi::OsStrExt, path::Path};

use crate::{
    folder::{self, LogSources},
    report::Report,
    section,
};

/// Report fields known only when the signal arrives; [`Prepared::write`] appends them in this
/// order.
const DEFERRED: [&str; 3] = ["uptime_ms", "thread", "native"];
/// Fits the deferred fields' fixed text plus five 20-digit numbers and a sign.
const BUFFER_BYTES: usize = 256;
/// The scratch buffer [`Prepared::write`] needs to copy one log section.
pub const SCRATCH_BYTES: usize = section::MAX_SECTION_BYTES as usize;

/// What a fatal signal tells the handler.
#[derive(Clone, Copy, Debug)]
pub struct SignalFault {
    pub signal: i32,
    /// `si_code`; positive for hardware faults.
    pub code: i32,
    pub address: u64,
}

/// One crash folder, prepared outside the handler.
pub struct Prepared {
    folder: CString,
    report: CString,
    /// `report.json` without its deferred fields or closing brace.
    prefix: Vec<u8>,
    started_at_ms: u64,
    client_log: LogCopy,
    core_log: LogCopy,
}

/// A log whose latest session the handler copies into the crash folder.
struct LogCopy {
    source: CString,
    target: CString,
}

impl Prepared {
    /// Prepares `report` for a folder in `crash_dir` named by `now_ms` and the report's session
    /// id with a `-signal` suffix, copying the session's sections of `logs`.
    pub fn new(
        crash_dir: &Path,
        report: &Report,
        logs: &LogSources<'_>,
        now_ms: u64,
    ) -> Option<Self> {
        let prefix = report.open_json(&DEFERRED).ok()?;
        let crash = crash_dir.join(folder::name(
            now_ms,
            &format!("{}-signal", report.session_id),
        ));
        let c_path = |path: &Path| CString::new(path.as_os_str().as_bytes()).ok();
        Some(Self {
            folder: c_path(&crash)?,
            report: c_path(&crash.join(folder::REPORT_FILE))?,
            prefix,
            started_at_ms: logs.started_at_ms,
            client_log: LogCopy {
                source: c_path(logs.client)?,
                target: c_path(&crash.join(folder::CLIENT_LOG_FILE))?,
            },
            core_log: LogCopy {
                source: c_path(logs.core)?,
                target: c_path(&crash.join(folder::CORE_LOG_FILE))?,
            },
        })
    }

    /// Writes the folder for a fault at `now_ms` (Unix milliseconds): `report.json` first, so a
    /// fault while copying logs still leaves the report, then the client and core log sections.
    ///
    /// # Safety
    ///
    /// Async-signal-safe: no allocation or locks. The caller must give exclusive use of
    /// `scratch`, which should hold [`SCRATCH_BYTES`]; a shorter one copies less of each log.
    pub unsafe fn write(&self, fault: &SignalFault, now_ms: u64, scratch: &mut [u8]) {
        // SAFETY: The paths are prebuilt; mkdir, open, write and close are async-signal-safe.
        let fd = unsafe {
            libc::mkdir(self.folder.as_ptr(), 0o700);
            libc::open(
                self.report.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY,
                0o600,
            )
        };
        if fd >= 0 {
            let uptime_ms = now_ms.saturating_sub(self.started_at_ms);
            let buffer = deferred_fields(fault, uptime_ms, thread_id());
            // SAFETY: `fd` is the report this call opened.
            unsafe {
                write_all(fd, &self.prefix);
                write_all(fd, &buffer.bytes[..buffer.len]);
                libc::fsync(fd);
                libc::close(fd);
            }
        }
        // SAFETY: The copies use the prebuilt paths and the caller's exclusive scratch buffer.
        unsafe {
            copy_section(
                &self.client_log,
                section::CLIENT_SESSION_START,
                None,
                scratch,
            );
            copy_section(
                &self.core_log,
                section::CORE_SESSION_START,
                Some(self.started_at_ms),
                scratch,
            );
        }
    }
}

/// `,"uptime_ms":…,"thread":{…},"native":{…}}`, closing the prepared report.
fn deferred_fields(fault: &SignalFault, uptime_ms: u64, thread_id: u64) -> Buffer {
    let mut buffer = Buffer {
        bytes: [0; BUFFER_BYTES],
        len: 0,
    };
    buffer.text(b",\"uptime_ms\":");
    buffer.number(uptime_ms);
    buffer.text(b",\"thread\":{\"name\":null,\"id\":\"");
    buffer.number(thread_id);
    buffer.text(b"\"},\"native\":{\"signal\":");
    buffer.number(fault.signal.unsigned_abs().into());
    buffer.text(b",\"signal_code\":");
    if fault.code < 0 {
        buffer.text(b"-");
    }
    buffer.number(fault.code.unsigned_abs().into());
    buffer.text(b",\"fault_address\":");
    buffer.number(fault.address);
    buffer.text(b"}}");
    buffer
}

/// Copies the latest session of a log, read from at most its final `scratch.len()` bytes, into a
/// new file with async-signal-safe calls only; a missing log leaves the copy empty. With
/// `since_ms`, a section whose header predates it belongs to an earlier session and is left out.
unsafe fn copy_section(copy: &LogCopy, marker: &str, since_ms: Option<u64>, scratch: &mut [u8]) {
    // SAFETY: The paths are prebuilt C strings; open, lseek, read, write, fsync and close are
    // async-signal-safe, and the section helpers neither allocate nor lock.
    unsafe {
        let target = libc::open(
            copy.target.as_ptr(),
            libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY,
            0o600,
        );
        if target < 0 {
            return;
        }
        let source = libc::open(copy.source.as_ptr(), libc::O_RDONLY);
        if source >= 0 {
            let length = libc::lseek(source, 0, libc::SEEK_END);
            let offset = (length.max(0) as u64).saturating_sub(scratch.len() as u64);
            if length >= 0 && libc::lseek(source, offset as libc::off_t, libc::SEEK_SET) >= 0 {
                let filled = read_full(source, scratch);
                let tail = &scratch[..filled];
                let part = &tail[section::start(tail, marker, offset > 0)..];
                if !since_ms.is_some_and(|since| section::is_stale(part, marker, since)) {
                    write_all(target, part);
                }
            }
            libc::close(source);
        }
        libc::fsync(target);
        libc::close(target);
    }
}

/// Reads until `buffer` is full, end of file or an OS error; returns the bytes read.
unsafe fn read_full(fd: i32, buffer: &mut [u8]) -> usize {
    let mut filled = 0;
    while filled < buffer.len() {
        // SAFETY: The destination is the unfilled, writable rest of `buffer`.
        let read = unsafe {
            libc::read(
                fd,
                buffer[filled..].as_mut_ptr().cast(),
                buffer.len() - filled,
            )
        };
        if read <= 0 {
            break;
        }
        filled += read as usize;
    }
    filled
}

/// Writes until complete or an OS error without calling Rust's I/O runtime.
unsafe fn write_all(fd: i32, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        // SAFETY: `bytes` is readable for its length.
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written <= 0 {
            break;
        }
        bytes = &bytes[written as usize..];
    }
}

/// The kernel id of the faulting thread, read without locks.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn thread_id() -> u64 {
    // SAFETY: gettid is a raw system call without side effects.
    (unsafe { libc::gettid() }) as u64
}

/// The kernel id of the faulting thread, read without locks.
#[cfg(target_vendor = "apple")]
fn thread_id() -> u64 {
    let mut id = 0_u64;
    // SAFETY: A zero thread selects the caller; the id is written to owned storage.
    unsafe {
        libc::pthread_threadid_np(0, &mut id);
    }
    id
}

/// The faulting thread's pthread handle, read without locks.
#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn thread_id() -> u64 {
    // SAFETY: pthread_self reads the caller's thread pointer.
    (unsafe { libc::pthread_self() }) as usize as u64
}

struct Buffer {
    bytes: [u8; BUFFER_BYTES],
    len: usize,
}

impl Buffer {
    /// Appends a fixed literal whose total size is bounded by the report format.
    fn text(&mut self, text: &[u8]) {
        for byte in text {
            self.bytes[self.len] = *byte;
            self.len += 1;
        }
    }

    /// Encodes an unsigned integer on the stack without formatting or allocation.
    fn number(&mut self, mut number: u64) {
        let mut digits = [0_u8; 20];
        let mut len = 0;
        loop {
            digits[len] = b'0' + (number % 10) as u8;
            len += 1;
            number /= 10;
            if number == 0 {
                break;
            }
        }
        for digit in digits[..len].iter().rev() {
            self.text(&[*digit]);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::report::{Identity, Kind};

    const NOW_MS: u64 = 1_760_443_200_000;

    #[test]
    fn fault_numbers_fit_without_allocating_or_truncating() {
        let mut buffer = Buffer {
            bytes: [0; BUFFER_BYTES],
            len: 0,
        };
        buffer.text(b"{");
        buffer.number(0);
        buffer.text(b",");
        buffer.number(u64::MAX);
        buffer.text(b"}");
        assert_eq!(&buffer.bytes[..buffer.len], b"{0,18446744073709551615}");
        let widest = deferred_fields(
            &SignalFault {
                signal: i32::MAX,
                code: i32::MIN,
                address: u64::MAX,
            },
            u64::MAX,
            u64::MAX,
        );
        assert!(widest.len < BUFFER_BYTES);
    }

    #[test]
    fn prepared_folder_reads_back_with_its_fault_and_this_sessions_logs() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            logs.join("client.log"),
            "CLIENT_SESSION_START session=old\nold\nCLIENT_SESSION_START session=a\nclient\n",
        )
        .unwrap();
        fs::write(
            logs.join("core.log"),
            "CORE_SESSION_START timestamp_ms=1\nstale core\n",
        )
        .unwrap();
        let identity = Identity {
            session_id: "0123456789abcdef".into(),
            started_at_ms: NOW_MS - 1_000,
            release: "1.2.3".into(),
            commit: "abc123".into(),
            build_profile: "debug".into(),
        };
        let report = Report::new(Kind::Native, "Fatal signal".into(), &identity, NOW_MS);
        let crash_dir = dir.path().join("crashes");
        fs::create_dir_all(&crash_dir).unwrap();
        let (client, core) = (logs.join("client.log"), logs.join("core.log"));
        let sources = LogSources {
            client: &client,
            core: &core,
            started_at_ms: identity.started_at_ms,
        };
        let prepared = Prepared::new(&crash_dir, &report, &sources, NOW_MS).unwrap();
        let mut scratch = vec![0; SCRATCH_BYTES];
        // SAFETY: The test owns the scratch buffer.
        unsafe {
            prepared.write(
                &SignalFault {
                    signal: libc::SIGSEGV,
                    code: -6,
                    address: 24,
                },
                NOW_MS,
                &mut scratch,
            );
        }
        let folders = folder::list(&crash_dir);
        assert_eq!(folders.len(), 1);
        assert!(
            folders[0]
                .to_str()
                .unwrap()
                .ends_with("-0123456789abcdef-signal")
        );
        let written = folder::read(&folders[0]).unwrap();
        assert_eq!(written.kind, Kind::Native);
        let native = written.native.unwrap();
        assert_eq!(native["signal"], libc::SIGSEGV);
        assert_eq!(native["signal_code"], -6);
        assert_eq!(native["fault_address"], 24);
        assert!(written.thread.id.parse::<u64>().is_ok());
        assert_eq!(
            fs::read_to_string(folders[0].join(folder::CLIENT_LOG_FILE)).unwrap(),
            "CLIENT_SESSION_START session=a\nclient\n"
        );
        assert!(
            fs::read(folders[0].join(folder::CORE_LOG_FILE))
                .unwrap()
                .is_empty()
        );
    }
}
