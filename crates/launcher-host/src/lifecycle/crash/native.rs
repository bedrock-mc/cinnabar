//! Best-effort reporting only after Windows has exhausted exception handlers.

use std::{
    ffi::c_void,
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{native_address::Address, *};
use windows_sys::Win32::System::{
    Diagnostics::Debug::{
        EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS, LPTOP_LEVEL_EXCEPTION_FILTER,
        RtlCaptureStackBackTrace, SetUnhandledExceptionFilter,
    },
    LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW,
    },
    Threading::GetCurrentThreadId,
};

struct Capture {
    root: PathBuf,
    crash_dir: PathBuf,
    client_log: PathBuf,
    flush_logs: fn(),
    previous: LPTOP_LEVEL_EXCEPTION_FILTER,
}

static CAPTURE: OnceLock<Capture> = OnceLock::new();
static REPORTING: AtomicBool = AtomicBool::new(false);

#[derive(Serialize)]
struct NativeReport {
    #[serde(flatten)]
    common: Report<'static>,
    exception_code: String,
    fault: Address,
    thread: u32,
    /// These frames come from the reporting thread; the fault address is recorded separately.
    handler_backtrace: Vec<Address>,
}

/// Installs one terminal filter while preserving any previous terminal handler.
pub(super) fn install(layout: &InstallLayout, flush_logs: fn()) {
    CAPTURE.get_or_init(|| Capture {
        root: layout.user_data_root.clone(),
        crash_dir: layout.crash_dir(),
        client_log: layout.log_dir().join("client.log"),
        flush_logs,
        // SAFETY: The callback has the required ABI and lives for the process lifetime.
        previous: unsafe { SetUnhandledExceptionFilter(Some(unhandled)) },
    });
}

/// Leaves runtime-handled exceptions alone and continues normal terminal handling after capture.
unsafe extern "system" fn unhandled(info: *const EXCEPTION_POINTERS) -> i32 {
    let Some(capture) = CAPTURE.get() else {
        return EXCEPTION_CONTINUE_SEARCH;
    };
    if let Some(previous) = capture.previous {
        // SAFETY: Windows supplied this pointer to a terminal exception filter.
        let disposition = unsafe { previous(info) };
        if disposition != EXCEPTION_CONTINUE_SEARCH {
            return disposition;
        }
    }
    if REPORTING.swap(true, Ordering::Relaxed) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    // SAFETY: Windows supplies a valid exception record during this callback.
    let Some(record) =
        (unsafe { info.as_ref() }).and_then(|info| unsafe { info.ExceptionRecord.as_ref() })
    else {
        return EXCEPTION_CONTINUE_SEARCH;
    };
    let _ = std::panic::catch_unwind(|| {
        (capture.flush_logs)();
        let report = NativeReport {
            common: Report {
                source: "native",
                message: "Unhandled Windows exception".into(),
                backtrace: "See handler_backtrace and fault; frames are best effort.".into(),
                log_tail: tail(&capture.client_log, LOG_TAIL_BYTES),
                release: env!("CARGO_PKG_VERSION"),
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
                renderer: renderer_context(&capture.root),
            },
            exception_code: format!("0x{:08X}", record.ExceptionCode as u32),
            fault: resolve(record.ExceptionAddress),
            // SAFETY: Retrieves only the calling thread's identifier.
            thread: unsafe { GetCurrentThreadId() },
            handler_backtrace: backtrace(),
        };
        if let Ok(bytes) = serde_json::to_vec(&report) {
            write_native(capture, &bytes);
        }
    });
    EXCEPTION_CONTINUE_SEARCH
}

/// Resolves an address to a module basename without exposing the user's install path.
fn resolve(address: *mut c_void) -> Address {
    let mut module = std::ptr::null_mut();
    // SAFETY: FROM_ADDRESS treats the pointer as an address, not a UTF-16 string.
    let found = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address.cast(),
            &mut module,
        )
    };
    let mut name = [0_u16; 1024];
    // SAFETY: The buffer is writable for its supplied length; the handle came from Windows.
    let length = if found != 0 {
        unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) }
    } else {
        0
    };
    let location = (length > 0 && (length as usize) < name.len()).then(|| {
        let path = String::from_utf16_lossy(&name[..length as usize]);
        (
            path.rsplit(['\\', '/']).next().unwrap_or(&path).to_owned(),
            module as u64,
        )
    });
    Address::new(address as u64, location)
}

/// Captures a bounded stack from the terminal reporting thread without symbol-engine locks.
fn backtrace() -> Vec<Address> {
    let mut frames = [std::ptr::null_mut(); 48];
    // SAFETY: The buffer holds the requested number of frames; no hash output is requested.
    let count = unsafe {
        RtlCaptureStackBackTrace(
            0,
            frames.len() as u32,
            frames.as_mut_ptr(),
            std::ptr::null_mut(),
        )
    };
    frames[..count as usize]
        .iter()
        .map(|address| resolve(*address))
        .collect()
}

/// Keeps native reports local and flushes their bytes before returning to Windows.
fn write_native(capture: &Capture, bytes: &[u8]) {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    if fs::create_dir_all(&capture.crash_dir).is_err() {
        return;
    }
    for suffix in 0..MAX_REPORTS {
        let name = if suffix == 0 {
            format!("crash-{millis}-native.json")
        } else {
            format!("crash-{millis}-{suffix}-native.json")
        };
        match fs::File::options()
            .write(true)
            .create_new(true)
            .open(capture.crash_dir.join(name))
        {
            Ok(mut file) => {
                let _ = file.write_all(bytes).and_then(|()| file.sync_all());
                return;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return,
        }
    }
}
