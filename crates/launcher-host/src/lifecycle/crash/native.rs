//! Best-effort reporting only after Windows has exhausted exception handlers.

use std::{
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{Context, native_address::Address};
use crash_report::report::{Kind, Thread};
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
    context: &'static Context,
    flush_logs: fn(),
    previous: LPTOP_LEVEL_EXCEPTION_FILTER,
}

static CAPTURE: OnceLock<Capture> = OnceLock::new();
static REPORTING: AtomicBool = AtomicBool::new(false);

/// Installs one terminal filter while preserving any previous terminal handler.
pub(super) fn install(context: &'static Context, flush_logs: fn()) {
    CAPTURE.get_or_init(|| Capture {
        context,
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
        let context = capture.context;
        let now_ms = super::now_ms();
        let attempt = super::renderer_context(&context.data_root);
        let mut report = super::report(
            context,
            Kind::Native,
            "Unhandled Windows exception".into(),
            attempt.as_ref(),
            now_ms,
        );
        // SAFETY: Retrieves only the calling thread's identifier.
        let thread = unsafe { GetCurrentThreadId() };
        report.thread = Thread {
            name: std::thread::current().name().map(str::to_owned),
            id: thread.to_string(),
        };
        report.native = Some(serde_json::json!({
            "exception_code": format!("0x{:08X}", record.ExceptionCode as u32),
            "fault": resolve(record.ExceptionAddress),
            "thread": thread,
            // These frames come from the reporting thread; the fault address is recorded separately.
            "handler_backtrace": backtrace(),
        }));
        let _ = super::write(context, &report, now_ms);
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
