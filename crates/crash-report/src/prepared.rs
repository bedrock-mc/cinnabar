//! Crash folders written from a Unix signal handler: everything that allocates is prepared
//! beforehand, and [`Prepared::write`] uses only async-signal-safe calls.

use std::{
    ffi::CString,
    fs::File,
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    path::Path,
};

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
    ready: CString,
    /// `report.json` without its deferred fields or closing brace.
    prefix: Vec<u8>,
    started_at_ms: u64,
    session_id: Option<String>,
    client_log: LogCopy,
    core_log: LogCopy,
}

/// A log whose latest session the handler copies into the crash folder.
struct LogCopy {
    source: Option<File>,
    current: CString,
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
            ready: c_path(&crash.join(folder::FALLBACK_READY_FILE))?,
            prefix,
            started_at_ms: logs.started_at_ms,
            session_id: logs.session_id.map(str::to_owned),
            client_log: LogCopy {
                current: c_path(logs.client)?,
                source: section::session::open(
                    logs.client,
                    section::CLIENT_SESSION_START,
                    logs.session_id,
                ),
                target: c_path(&crash.join(folder::CLIENT_LOG_FILE))?,
            },
            core_log: LogCopy {
                current: c_path(logs.core)?,
                source: section::session::open(
                    logs.core,
                    section::CORE_SESSION_START,
                    logs.session_id,
                ),
                target: c_path(&crash.join(folder::CORE_LOG_FILE))?,
            },
        })
    }

    /// Writes the folder for a fault at `now_ms` (Unix milliseconds): `report.json` first, so a
    /// fault while copying logs still leaves the report, then the client and core log sections.
    /// A final marker lets a healthy sender recover the completed fallback on a later launch.
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
        // SAFETY: The copies and marker use prebuilt paths and async-signal-safe IO only.
        // The caller owns the scratch buffer exclusively.
        unsafe {
            copy_section(
                &self.client_log,
                section::CLIENT_SESSION_START,
                None,
                self.session_id.as_deref(),
                scratch,
            );
            copy_section(
                &self.core_log,
                section::CORE_SESSION_START,
                Some(self.started_at_ms),
                self.session_id.as_deref(),
                scratch,
            );
            if fd >= 0 {
                let ready = libc::open(
                    self.ready.as_ptr(),
                    libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY,
                    0o600,
                );
                if ready >= 0 {
                    libc::close(ready);
                }
            }
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
unsafe fn copy_section(
    copy: &LogCopy,
    marker: &str,
    since_ms: Option<u64>,
    session_id: Option<&str>,
    scratch: &mut [u8],
) {
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
        let current =
            session_id.and_then(|session_id| current_source(copy, marker, session_id, scratch));
        let source = current.or_else(|| copy.source.as_ref().map(AsRawFd::as_raw_fd));
        if let Some(source) = source {
            let length = libc::lseek(source, 0, libc::SEEK_END);
            let current = since_ms.is_none_or(|since| {
                if libc::lseek(source, 0, libc::SEEK_SET) < 0 {
                    return false;
                }
                section::header::latest_timestamp(marker, scratch, |buffer| {
                    let count = libc::read(source, buffer.as_mut_ptr().cast(), buffer.len());
                    if count < 0 {
                        Err(())
                    } else {
                        Ok(count as usize)
                    }
                })
                .is_ok_and(|timestamp| timestamp.is_none_or(|at| at >= since))
            });
            let offset = (length.max(0) as u64).saturating_sub(scratch.len() as u64);
            if current
                && length >= 0
                && libc::lseek(source, offset as libc::off_t, libc::SEEK_SET) >= 0
            {
                let filled = read_full(source, scratch);
                let tail = &scratch[..filled];
                let cut = match since_ms {
                    Some(since) => section::since(tail, marker, since, offset > 0),
                    None => section::start(tail, marker, offset > 0),
                };
                write_all(target, &tail[cut..]);
            }
        }
        if let Some(current) = current {
            libc::close(current);
        }
        libc::fsync(target);
        libc::close(target);
    }
}

/// Opens the current segment only when its header still belongs to this session.
/// The caller owns scratch exclusively and must close any returned descriptor.
unsafe fn current_source(
    copy: &LogCopy,
    marker: &str,
    session_id: &str,
    scratch: &mut [u8],
) -> Option<i32> {
    // SAFETY: The C path is prepared, and all IO uses the exclusive caller-provided buffer.
    unsafe {
        let fd = libc::open(
            copy.current.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        );
        if fd < 0 {
            return None;
        }
        let count = scratch.len().min(section::HEADER_BYTES);
        let filled = read_full(fd, &mut scratch[..count]);
        if section::session::matches_header(&scratch[..filled], marker, session_id) {
            Some(fd)
        } else {
            libc::close(fd);
            None
        }
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
            format!(
                "CORE_SESSION_START timestamp_ms=1\n{}",
                "stale core\n".repeat(SCRATCH_BYTES / 8)
            ),
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
            session_id: None,
            client: &client,
            core: &core,
            started_at_ms: identity.started_at_ms,
        };
        let prepared = Prepared::new(&crash_dir, &report, &sources, NOW_MS).unwrap();
        fs::rename(&client, logs.join("client-20260101-000000.log")).unwrap();
        fs::write(
            &client,
            "CLIENT_SESSION_START session=another\nunrelated client\n",
        )
        .unwrap();
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
        assert!(folders[0].join(folder::FALLBACK_READY_FILE).is_file());
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
        let payload = crate::upload::build(
            &folders[0],
            crate::upload::Ids {
                install: "0123456789abcdef0123456789abcdef",
                report: "fedcba9876543210fedcba9876543210",
            },
            None,
        )
        .unwrap();
        assert_eq!(payload.native.as_ref().unwrap()["address"], "0x18");
        assert_eq!(
            payload.signature,
            format!("native: signal {} / code -6", libc::SIGSEGV)
        );
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

#[cfg(test)]
mod rotation_tests {
    use super::*;
    use crate::report::{Identity, Kind};

    #[test]
    fn native_snapshot_follows_the_same_sessions_new_segment() {
        let dir = tempfile::tempdir().unwrap();
        let client = dir.path().join("client.log");
        let core = dir.path().join("core.log");
        let identity = Identity {
            session_id: "0123456789abcdef".into(),
            started_at_ms: 0,
            release: "fixture".into(),
            commit: "fixture".into(),
            build_profile: "test".into(),
        };
        let header = format!(
            "{} session={}\n",
            section::CLIENT_SESSION_START,
            identity.session_id
        );
        std::fs::write(&client, format!("{header}old segment\n")).unwrap();
        let report = Report::new(Kind::Native, "fixture".into(), &identity, 1000);
        let prepared = Prepared::new(
            dir.path(),
            &report,
            &LogSources {
                session_id: Some(&identity.session_id),
                client: &client,
                core: &core,
                started_at_ms: 0,
            },
            1000,
        )
        .unwrap();
        std::fs::rename(&client, dir.path().join("client-19700101-000001.log")).unwrap();
        let current = format!("{header}latest segment\n");
        std::fs::write(&client, &current).unwrap();
        let mut scratch = vec![0; SCRATCH_BYTES];
        // SAFETY: This fixture owns the scratch buffer and calls the writer only once.
        unsafe {
            prepared.write(
                &SignalFault {
                    signal: libc::SIGABRT,
                    code: 0,
                    address: 0,
                },
                2000,
                &mut scratch,
            );
        }
        let folders = folder::list(dir.path());
        assert_eq!(folders.len(), 1);
        assert_eq!(
            std::fs::read_to_string(folders[0].join(folder::CLIENT_LOG_FILE)).unwrap(),
            current
        );
    }
}
