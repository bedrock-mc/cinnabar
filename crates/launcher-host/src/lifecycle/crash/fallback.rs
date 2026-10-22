//! In-process crash folders for when no monitor is connected: before it is up, when it could
//! not start, and on Android and iOS, which have none. Panic and Windows native reports carry the
//! staging [`super::attachments`]; a Unix fatal signal writes only its prepared folder, which iOS,
//! without a native fault handler, never prepares.

use std::path::PathBuf;

use crash_report::{
    folder,
    report::{Kind, Thread},
};

use super::{Context, attachments::Attachments};

/// Writes a panic report with this session's log sections.
pub(super) fn write_panic(
    context: &Context,
    message: String,
    thread: Thread,
    backtrace: String,
    now_ms: u64,
) -> std::io::Result<PathBuf> {
    let mut report = super::report(
        context,
        Kind::Panic,
        message,
        &super::snapshot(context),
        now_ms,
    );
    report.thread = thread;
    report.backtrace = backtrace;
    write(context, &report, now_ms)
}

/// Writes `report` with this session's log sections and the staging attachments.
fn write(
    context: &Context,
    report: &crash_report::report::Report,
    now_ms: u64,
) -> std::io::Result<PathBuf> {
    let logs = folder::Logs::read(&context.log_sources());
    let attachments = context
        .staging
        .as_ref()
        .map(|staging| Attachments::read(staging.dir()))
        .unwrap_or_default();
    folder::write_with(
        &context.crash_dir,
        report,
        &logs,
        None,
        &attachments.as_folder(),
        now_ms,
    )
}

/// Writes a native report from the faulting thread. Windows exception handling has no signal
/// restrictions, so this builds the report as a panic does.
#[cfg(windows)]
pub(super) fn write_native(context: &Context, crash: &crash_handler::CrashContext) {
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;

    let _ = std::panic::catch_unwind(|| {
        let now_ms = super::now_ms();
        // A faulting thread may hold the state; report without it rather than wait.
        let snapshot = super::try_snapshot(context).unwrap_or_default();
        let mut report = super::report(
            context,
            Kind::Native,
            "Unhandled Windows exception".into(),
            &snapshot,
            now_ms,
        );
        // SAFETY: Retrieves only the calling thread's identifier.
        let thread = unsafe { GetCurrentThreadId() };
        report.thread = Thread {
            name: std::thread::current().name().map(str::to_owned),
            id: thread.to_string(),
        };
        // SAFETY: crash-handler passes the faulting thread's live exception pointers.
        let address = unsafe {
            crash
                .exception_pointers
                .as_ref()
                .and_then(|pointers| pointers.ExceptionRecord.as_ref())
                .map_or(0, |record| record.ExceptionAddress as u64)
        };
        report.native = Some(serde_json::json!({
            "exception_code": format!("0x{:08X}", crash.exception_code as u32),
            "fault": windows::resolve(address),
            "thread": thread,
        }));
        let _ = write(context, &report, now_ms);
    });
}

#[cfg(windows)]
mod windows {
    use crash_report::native::{Address, module_name};
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW,
    };

    /// Resolves an address to its module's file name and offset.
    pub(super) fn resolve(address: u64) -> Address {
        let mut module = std::ptr::null_mut();
        // SAFETY: FROM_ADDRESS treats the pointer as an address, not a UTF-16 string.
        let found = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                address as usize as *const u16,
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
            (module_name(&path).to_owned(), module as u64)
        });
        Address::new(address, location)
    }
}

/// The folder a fatal signal writes, prepared outside the handler. Only startup and the periods
/// without a monitor prepare new ones.
#[cfg(all(unix, not(target_os = "ios")))]
static PREPARED: std::sync::atomic::AtomicPtr<crash_report::prepared::Prepared> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
/// Set once a handler may be reading [`PREPARED`]; snapshots replaced after that stay allocated.
#[cfg(all(unix, not(target_os = "ios")))]
static READING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The section buffer, allocated once and used only by the first reporting handler.
#[cfg(all(unix, not(target_os = "ios")))]
static SCRATCH: std::sync::atomic::AtomicPtr<u8> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Prepares the folder a fatal signal writes from the current state.
#[cfg(all(unix, not(target_os = "ios")))]
pub(super) fn prepare(context: &Context) {
    use std::sync::atomic::Ordering;

    if READING.load(Ordering::SeqCst) {
        return;
    }
    if SCRATCH.load(Ordering::Acquire).is_null() {
        let scratch: *mut [u8] =
            Box::into_raw(vec![0_u8; crash_report::prepared::SCRATCH_BYTES].into_boxed_slice());
        SCRATCH.store(scratch.cast::<u8>(), Ordering::Release);
    }
    if std::fs::create_dir_all(&context.crash_dir).is_err() {
        return;
    }
    let now_ms = super::now_ms();
    let report = super::report(
        context,
        Kind::Native,
        "Fatal signal".into(),
        &super::snapshot(context),
        now_ms,
    );
    if let Some(prepared) = crash_report::prepared::Prepared::new(
        &context.crash_dir,
        &report,
        &context.log_sources(),
        now_ms,
    ) {
        let replaced = PREPARED.swap(Box::into_raw(Box::new(prepared)), Ordering::SeqCst);
        if !replaced.is_null() && !READING.load(Ordering::SeqCst) {
            // SAFETY: No handler has read `PREPARED` yet, and any that does from now on finds
            // the new snapshot; each replaced pointer is freed by the one swap that took it.
            drop(unsafe { Box::from_raw(replaced) });
        }
    }
}

/// Writes the prepared folder with async-signal-safe calls only.
#[cfg(all(unix, not(target_os = "ios")))]
pub(super) fn write_native(crash: &crash_handler::CrashContext) {
    use std::sync::atomic::Ordering;

    READING.store(true, Ordering::SeqCst);
    let prepared = PREPARED.load(Ordering::SeqCst);
    let scratch = SCRATCH.load(Ordering::Acquire);
    if prepared.is_null() || scratch.is_null() {
        return;
    }
    // SAFETY: Snapshots stay allocated once `READING` is set, and the scratch buffer for the
    // process lifetime; the caller admits one reporting handler, the scratch buffer's only user.
    unsafe {
        let scratch =
            std::slice::from_raw_parts_mut(scratch, crash_report::prepared::SCRATCH_BYTES);
        (*prepared).write(&fault(crash), signal_safe_now_ms(), scratch);
    }
}

/// The signal, its code and the fault address.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn fault(crash: &crash_handler::CrashContext) -> crash_report::prepared::SignalFault {
    const {
        assert!(
            std::mem::size_of::<libc::siginfo_t>() == std::mem::size_of::<libc::signalfd_siginfo>()
        );
    }
    // The handler stores the original siginfo_t bytes in its siginfo carrier.
    // SAFETY: Both carriers hold the same-sized native signal record. Reading unaligned
    // avoids imposing siginfo_t alignment on the carrier's field.
    let info =
        unsafe { std::ptr::read_unaligned((&raw const crash.siginfo).cast::<libc::siginfo_t>()) };
    let kernel_fault = info.si_code > 0
        && matches!(
            info.si_signo,
            libc::SIGSEGV | libc::SIGBUS | libc::SIGILL | libc::SIGFPE | libc::SIGTRAP
        );
    crash_report::prepared::SignalFault {
        signal: info.si_signo,
        code: info.si_code,
        // SAFETY: These kernel fault signals initialize the si_addr member of the union.
        address: if kernel_fault {
            unsafe { info.si_addr() as usize as u64 }
        } else {
            0
        },
    }
}

/// macOS reports Mach exceptions: `signal` is the signal the kernel would raise for it, `code`
/// the exception code and the address the subcode, the faulting address of a bad access.
#[cfg(target_os = "macos")]
fn fault(crash: &crash_handler::CrashContext) -> crash_report::prepared::SignalFault {
    const EXC_BAD_ACCESS: u32 = 1;
    const EXC_BAD_INSTRUCTION: u32 = 2;
    const EXC_ARITHMETIC: u32 = 3;
    const EXC_SOFTWARE: u32 = 5;
    const EXC_BREAKPOINT: u32 = 6;
    const EXC_SOFT_SIGNAL: u64 = 0x10003;
    let (kind, code, subcode) = crash.exception.map_or((0, 0, None), |exception| {
        (exception.kind, exception.code, exception.subcode)
    });
    let signal = match kind {
        EXC_BAD_ACCESS => libc::SIGSEGV,
        EXC_BAD_INSTRUCTION => libc::SIGILL,
        EXC_ARITHMETIC => libc::SIGFPE,
        EXC_BREAKPOINT => libc::SIGTRAP,
        EXC_SOFTWARE if code == EXC_SOFT_SIGNAL => subcode.unwrap_or_default() as i32,
        _ => 0,
    };
    crash_report::prepared::SignalFault {
        signal,
        code: code as i32,
        address: if kind == EXC_BAD_ACCESS {
            subcode.unwrap_or_default()
        } else {
            0
        },
    }
}

/// Unix milliseconds from the async-signal-safe `clock_gettime`.
#[cfg(all(unix, not(target_os = "ios")))]
fn signal_safe_now_ms() -> u64 {
    // SAFETY: clock_gettime only writes the supplied timespec.
    let mut now: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut now) } != 0 {
        return 0;
    }
    (now.tv_sec as u64)
        .saturating_mul(1000)
        .saturating_add(now.tv_nsec as u64 / 1_000_000)
}
