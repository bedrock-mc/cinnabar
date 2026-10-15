//! Fatal Unix signals write prebuilt metadata with async-signal-safe calls, then restore handling.

use crate::renderer_startup::Attempt;
use launcher::install_layout::InstallLayout;
use std::{
    ffi::CString,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

pub(crate) const SIGNALS: [i32; 5] = [
    libc::SIGSEGV,
    libc::SIGBUS,
    libc::SIGILL,
    libc::SIGFPE,
    libc::SIGABRT,
];
static CAPTURE: AtomicPtr<Capture> = AtomicPtr::new(std::ptr::null_mut());
static METADATA: AtomicPtr<Vec<u8>> = AtomicPtr::new(std::ptr::null_mut());
static REPORTING: AtomicBool = AtomicBool::new(false);

struct Capture {
    path: CString,
    previous: [libc::sigaction; SIGNALS.len()],
}

/// Opens a private report destination and saves every previous handler before installing ours.
pub(super) fn install(layout: &InstallLayout) {
    use std::{
        os::unix::ffi::OsStrExt,
        time::{SystemTime, UNIX_EPOCH},
    };
    if !CAPTURE.load(Ordering::Acquire).is_null() {
        return;
    }
    if std::fs::create_dir_all(layout.crash_dir()).is_err() {
        return;
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis());
    let Ok(path) = CString::new(
        layout
            .crash_dir()
            .join(format!("crash-{millis}-{}-unix.json", std::process::id()))
            .as_os_str()
            .as_bytes(),
    ) else {
        return;
    };
    // SAFETY: A zeroed sigaction is initialized fully before installation and query writes all fields.
    let mut capture = Box::new(Capture {
        path,
        previous: unsafe { std::mem::zeroed() },
    });
    for (i, signal) in SIGNALS.iter().enumerate() {
        // SAFETY: A null new action queries the current handler into valid owned storage.
        unsafe {
            libc::sigaction(*signal, std::ptr::null(), &mut capture.previous[i]);
        }
    }
    update(super::renderer_context(&layout.user_data_root).as_ref());
    let previous = capture.previous;
    CAPTURE.store(Box::into_raw(capture), Ordering::Release);
    for (signal, saved) in SIGNALS.into_iter().zip(previous) {
        // SAFETY: All fields are set for the three-argument signal callback.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = unhandled as *const () as libc::sighandler_t;
        action.sa_flags = libc::SA_SIGINFO
            | (saved.sa_flags & (libc::SA_ONSTACK | libc::SA_NODEFER | libc::SA_RESTART));
        action.sa_mask = saved.sa_mask;
        unsafe {
            libc::sigaction(signal, &action, std::ptr::null_mut());
        }
    }
}

/// Publishes immutable metadata outside the handler; old snapshots stay valid until process exit.
pub(crate) fn update(attempt: Option<&Attempt>) {
    let Ok(mut bytes) = serde_json::to_vec(&serde_json::json!({
        "source": "native", "release": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "renderer": attempt,
    })) else {
        return;
    };
    bytes.pop(); // The signal callback appends its numeric fault fields and the closing brace.
    METADATA.store(Box::into_raw(Box::new(bytes)), Ordering::Release);
}

/// Writes without allocation or locks, then forwards the original fault to the saved handler.
unsafe extern "C" fn unhandled(
    signal: i32,
    info: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) {
    let capture = CAPTURE.load(Ordering::Acquire);
    if capture.is_null() {
        return;
    }
    // SAFETY: Published snapshots are immutable and retained for the process lifetime.
    let capture = unsafe { &*capture };
    if !REPORTING.swap(true, Ordering::AcqRel) {
        let metadata = METADATA.load(Ordering::Acquire);
        // SAFETY: The path is prebuilt and open/write/close are async-signal-safe.
        let fd = unsafe {
            libc::open(
                capture.path.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY,
                0o600,
            )
        };
        if fd >= 0 {
            if !metadata.is_null() {
                let bytes = unsafe { &*metadata };
                unsafe {
                    write_all(fd, bytes);
                }
            }
            let mut buffer = Buffer {
                bytes: [0; 160],
                len: 0,
            };
            buffer.text(b",\"signal\":");
            buffer.number(signal as u64);
            buffer.text(b",\"signal_code\":");
            let code = if info.is_null() {
                0
            } else {
                unsafe { (*info).si_code }
            };
            if code < 0 {
                buffer.text(b"-");
            }
            buffer.number(code.unsigned_abs() as u64);
            buffer.text(b",\"fault_address\":");
            buffer.number(if info.is_null() {
                0
            } else {
                unsafe { (*info).si_addr() as u64 }
            });
            buffer.text(b"}");
            unsafe {
                write_all(fd, &buffer.bytes[..buffer.len]);
                libc::close(fd);
            }
        }
    }
    if let Some(index) = SIGNALS.iter().position(|candidate| *candidate == signal) {
        // SAFETY: The saved action and the kernel-supplied fault pointers remain valid here.
        unsafe {
            forward(signal, info, context, &capture.previous[index]);
        }
    }
}

/// Calls a saved handler with the original fault, or lets a hardware instruction fault again.
unsafe fn forward(
    signal: i32,
    info: *mut libc::siginfo_t,
    context: *mut libc::c_void,
    previous: &libc::sigaction,
) {
    let handler = previous.sa_sigaction;
    // SAFETY: Restore the disposition before returning or invoking a saved handler.
    unsafe {
        libc::sigaction(signal, previous, std::ptr::null_mut());
    }
    if handler == libc::SIG_IGN {
        return;
    }
    if handler == libc::SIG_DFL {
        let code = if info.is_null() {
            0
        } else {
            unsafe { (*info).si_code }
        };
        // BSD reserves the high 16 bits for sender codes; Linux uses nonpositive sender codes.
        // xnu's sendsig gives SIGSEGV, SIGBUS and SIGILL fixed positive fault codes and no sender
        // pid even when they are raised, so Apple targets cannot tell the two apart and always
        // re-raise. That is also correct for a real fault: the signal stays blocked until this
        // handler returns, and its default action then ends the process before the instruction
        // runs again.
        let hardware = !cfg!(target_vendor = "apple")
            && code > 0
            && code < 0x10000
            && matches!(
                signal,
                libc::SIGSEGV | libc::SIGBUS | libc::SIGILL | libc::SIGFPE
            );
        if !hardware {
            // SAFETY: Software signals have no faulting instruction to repeat after this return.
            unsafe {
                libc::raise(signal);
            }
        }
        return;
    }
    if previous.sa_flags & libc::SA_RESETHAND != 0 {
        // SAFETY: A one-shot saved handler must have the default disposition while it runs.
        let mut reset: libc::sigaction = unsafe { std::mem::zeroed() };
        reset.sa_sigaction = libc::SIG_DFL;
        unsafe {
            libc::sigaction(signal, &reset, std::ptr::null_mut());
        }
    }
    if previous.sa_flags & libc::SA_SIGINFO != 0 {
        // SAFETY: SA_SIGINFO selects the three-argument C callback stored in the action.
        let callback: unsafe extern "C" fn(i32, *mut libc::siginfo_t, *mut libc::c_void) =
            unsafe { std::mem::transmute(handler) };
        unsafe {
            callback(signal, info, context);
        }
    } else {
        // SAFETY: Without SA_SIGINFO the saved action holds a one-argument C callback.
        let callback: unsafe extern "C" fn(i32) = unsafe { std::mem::transmute(handler) };
        unsafe {
            callback(signal);
        }
    }
}

/// Writes until complete or an OS error without calling Rust's I/O runtime.
unsafe fn write_all(fd: i32, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written <= 0 {
            break;
        }
        bytes = &bytes[written as usize..];
    }
}

struct Buffer {
    bytes: [u8; 160],
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
    use super::*;
    static FAULT_ADDRESS: AtomicPtr<libc::c_void> = AtomicPtr::new(std::ptr::null_mut());
    /// Signal the kernel raises for reading a `PROT_NONE` page; macOS reports a bus error.
    const PROTECTION_FAULT: i32 = if cfg!(target_os = "macos") {
        libc::SIGBUS
    } else {
        libc::SIGSEGV
    };

    /// Signal a fixture run ends with: a page fault, a raised segmentation fault or an abort.
    fn fixture_signal(hardware: bool, software_fault: bool) -> i32 {
        if hardware {
            PROTECTION_FAULT
        } else if software_fault {
            libc::SIGSEGV
        } else {
            libc::SIGABRT
        }
    }

    /// Checks that chaining retains the kernel's fault address, cause and machine context.
    unsafe extern "C" fn previous_siginfo_handler(
        signal: i32,
        info: *mut libc::siginfo_t,
        context: *mut libc::c_void,
    ) {
        let original = signal == PROTECTION_FAULT
            && !info.is_null()
            && !context.is_null()
            && unsafe {
                (*info).si_code > 0 && (*info).si_addr() == FAULT_ADDRESS.load(Ordering::Relaxed)
            };
        unsafe {
            libc::_exit(if original { 77 } else { 78 });
        }
    }
    /// Exits through the handler installed before capture, proving it remains in control.
    unsafe extern "C" fn previous_handler(_signal: i32) {
        unsafe {
            libc::_exit(77);
        }
    }

    #[test]
    fn signal_fixture() {
        let Some(root) = std::env::var_os("CINNABAR_SIGNAL_FIXTURE") else {
            return;
        };
        let mut layout = launcher::test_support::checkout();
        layout.user_data_root = std::path::PathBuf::from(root);
        let hardware = std::env::var_os("CINNABAR_SIGNAL_HARDWARE").is_some();
        let software_fault = std::env::var_os("CINNABAR_SIGNAL_SOFTWARE_FAULT").is_some();
        let signal = fixture_signal(hardware, software_fault);
        // SAFETY: This test runs in a disposable subprocess; disable core files and install its handler.
        unsafe {
            let limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &limit);
            if std::env::var_os("CINNABAR_SIGNAL_CHAIN").is_some() {
                if hardware {
                    let mut action: libc::sigaction = std::mem::zeroed();
                    action.sa_sigaction =
                        previous_siginfo_handler as *const () as libc::sighandler_t;
                    action.sa_flags = libc::SA_SIGINFO;
                    libc::sigaction(signal, &action, std::ptr::null_mut());
                } else {
                    libc::signal(signal, previous_handler as *const () as libc::sighandler_t);
                }
            } else {
                libc::signal(signal, libc::SIG_DFL);
            }
        }
        install(&layout);
        let attempt = crate::renderer_startup::Attempt {
            backend: launcher::graphics_backend::Backend::Metal,
            adapter: None,
            stage: crate::renderer_startup::Stage::CreatingDevice,
            explicit: false,
        };
        update(Some(&attempt));
        unsafe {
            if hardware {
                let page_size = libc::sysconf(libc::_SC_PAGESIZE);
                assert!(page_size > 0);
                let address = libc::mmap(
                    std::ptr::null_mut(),
                    page_size as usize,
                    libc::PROT_NONE,
                    libc::MAP_PRIVATE | libc::MAP_ANON,
                    -1,
                    0,
                );
                assert_ne!(address, libc::MAP_FAILED);
                FAULT_ADDRESS.store(address, Ordering::Relaxed);
                std::ptr::read_volatile(address.cast::<u8>());
            } else {
                libc::raise(signal);
            }
        }
        panic!("fatal signal unexpectedly returned");
    }

    #[test]
    fn unix_signal_capture_records_context_and_preserves_previous_handling() {
        use std::{os::unix::process::ExitStatusExt, process::Command};
        for (chain, hardware, software_fault) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (false, false, true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "lifecycle::crash::unix::tests::signal_fixture",
                    "--nocapture",
                ])
                .env("CINNABAR_SIGNAL_FIXTURE", directory.path());
            if chain {
                command.env("CINNABAR_SIGNAL_CHAIN", "1");
            }
            if hardware {
                command.env("CINNABAR_SIGNAL_HARDWARE", "1");
            }
            if software_fault {
                command.env("CINNABAR_SIGNAL_SOFTWARE_FAULT", "1");
            }
            let signal = fixture_signal(hardware, software_fault);
            let mut child = command.spawn().unwrap();
            let mut status = None;
            let complete = test_time::wait_until(test_time::DEFAULT_TIMEOUT, || {
                status = child.try_wait().unwrap();
                status.is_some()
            });
            if !complete {
                let _ = child.kill();
                let _ = child.wait();
            }
            assert!(complete, "signal fixture must terminate");
            let status = status.unwrap();
            if chain {
                assert_eq!(status.code(), Some(77));
            } else {
                assert_eq!(status.signal(), Some(signal));
            }
            let reports = super::super::reports(&directory.path().join("crashes"));
            assert_eq!(reports.len(), 1);
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&reports[0]).unwrap()).unwrap();
            assert_eq!(report["signal"], signal);
            if hardware {
                assert!(report["signal_code"].as_i64().unwrap() > 0);
                assert!(report["fault_address"].as_u64().unwrap() > 0);
            }
            assert_eq!(report["renderer"]["stage"], "creating_device");
            assert_eq!(report["source"], "native");
        }
    }

    #[test]
    fn fault_numbers_fit_without_allocating_or_truncating() {
        let mut buffer = Buffer {
            bytes: [0; 160],
            len: 0,
        };
        buffer.text(b"{");
        buffer.number(0);
        buffer.text(b",");
        buffer.number(u64::MAX);
        buffer.text(b"}");
        assert_eq!(&buffer.bytes[..buffer.len], b"{0,18446744073709551615}");
    }
}
