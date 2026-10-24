//! Deterministic Linux faults with the stack pointer inside an unreadable guard mapping.

use minidump::{Minidump, MinidumpException, MinidumpSystemInfo, MinidumpThreadList};

/// Linux's lightweight guard advice, which libc does not expose yet.
const MADV_GUARD_INSTALL: libc::c_int = 102;

/// Reports support for lightweight guards and names the missing fixture when unavailable.
#[expect(
    clippy::print_stderr,
    reason = "names the unavailable kernel fixture when skipping"
)]
pub(super) fn lightweight_supported() -> bool {
    // SAFETY: A zero-length, null-address query only checks whether the advice is supported.
    if unsafe { libc::madvise(std::ptr::null_mut(), 0, MADV_GUARD_INSTALL) } == 0 {
        true
    } else {
        eprintln!("SKIP: kernel fixture MADV_GUARD_INSTALL is unavailable");
        false
    }
}

/// Faults inside a lightweight guard whose mapping still has read and write permissions.
pub(super) fn lightweight() -> ! {
    // SAFETY: sysconf has no pointer arguments and queries the system page size.
    let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).unwrap();
    // SAFETY: A new anonymous mapping has no aliases and lives until this process crashes.
    let base = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            5 * page,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(base, libc::MAP_FAILED);
    // SAFETY: The middle three pages belong to this mapping.
    let middle = unsafe { base.byte_add(page) };
    // SAFETY: The range is page-aligned and lies inside our mapping.
    assert_eq!(
        unsafe { libc::mprotect(middle, 3 * page, libc::PROT_READ | libc::PROT_WRITE) },
        0
    );
    // SAFETY: These three pages are writable and have no guard installed yet.
    unsafe { middle.cast::<u8>().write_bytes(0x5a, 3 * page) };
    // SAFETY: The selected guard page lies in the middle of the writable range.
    let guard = unsafe { middle.byte_add(page) };
    // SAFETY: Installs a guard inside this fixture's own anonymous mapping.
    assert_eq!(unsafe { libc::madvise(guard, page, MADV_GUARD_INSTALL) }, 0);
    // SAFETY: This address is inside the guard, immediately below the readable stack.
    fault(unsafe { guard.byte_add(page - std::mem::size_of::<usize>()) })
}

/// Faults just below a readable stack, or deeper than the writer's guard-page search.
pub(super) fn crash(below_guard: bool) -> ! {
    // SAFETY: sysconf has no pointer arguments and queries the system page size.
    let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).unwrap();
    let guard_len = 2 * 1024 * 1024;
    // SAFETY: A new anonymous mapping has no aliases and lives until this process crashes.
    let base = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            guard_len + 2 * page,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(base, libc::MAP_FAILED);
    // SAFETY: This page belongs to the mapping above and starts at a page boundary.
    let readable = unsafe { base.byte_add(guard_len) };
    // SAFETY: mprotect changes only the final page of our new mapping.
    assert_eq!(
        unsafe { libc::mprotect(readable, page, libc::PROT_READ | libc::PROT_WRITE) },
        0
    );
    // SAFETY: The final page is writable and remains allocated until the process ends.
    unsafe { readable.cast::<u8>().write_bytes(0x5a, page) };
    // SAFETY: Both choices are within our mapping; the push below them hits the guard.
    let stack = if below_guard {
        unsafe { base.byte_add(page) }
    } else {
        unsafe { readable.byte_sub(std::mem::size_of::<usize>()) }
    };
    fault(stack)
}

/// Faults on a stack backed by an empty file, which no process-memory reader can copy.
pub(super) fn unreadable() -> ! {
    use std::os::fd::AsRawFd;

    let file = tempfile::tempfile().unwrap();
    // SAFETY: sysconf has no pointer arguments and queries the system page size.
    let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).unwrap();
    let reserved = 2 * 1024 * 1024 + page;
    // SAFETY: A new anonymous reservation keeps other mappings outside the guard search.
    let base = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            reserved,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(base, libc::MAP_FAILED);
    // SAFETY: Replaces only the first page of our reservation with the empty backing file.
    let base = unsafe {
        libc::mmap(
            base,
            page,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_FIXED,
            file.as_raw_fd(),
            0,
        )
    };
    assert_ne!(base, libc::MAP_FAILED);
    // SAFETY: The address is inside the mapping; accessing it deliberately raises SIGBUS.
    fault(unsafe { base.byte_add(page / 2) })
}

/// Switches to an invalid stack and faults without returning to the Rust caller.
fn fault(stack: *mut libc::c_void) -> ! {
    // SAFETY: This disposable fixture intentionally faults on a push into unreadable memory.
    // The installed handler uses an alternate signal stack; this code never returns to Rust.
    unsafe {
        std::arch::asm!(
            "mov rsp, {stack}",
            "push 0",
            "ud2",
            stack = in(reg) stack,
            options(noreturn),
        )
    }
}

/// Checks that an unreadable stack cannot discard the crash or other threads' stacks.
pub(super) fn assert_dump(run: super::Run, has_stack: bool) {
    assert_eq!(run.folders.len(), 1);
    super::assert_report(&run.folders[0], crash_report::report::Kind::Native, true);
    assert_eq!(run.shown.as_ref(), Some(&run.folders[0]));
    let dump = Minidump::read_path(run.folders[0].join(crash_report::folder::DUMP_FILE)).unwrap();
    let exception = dump.get_stream::<MinidumpException>().unwrap();
    let system = dump.get_stream::<MinidumpSystemInfo>().unwrap();
    let threads = dump.get_stream::<MinidumpThreadList>().unwrap();
    let crashed = threads.get_thread(exception.thread_id).unwrap();
    let context = crashed.context(&system, None).unwrap();
    let exception_context = exception.context(&system, None).unwrap();
    assert_eq!(
        context.get_stack_pointer(),
        exception_context.get_stack_pointer()
    );
    assert!(context.get_instruction_pointer() > 0);
    assert_eq!(crashed.raw.stack.memory.data_size > 0, has_stack);
    if has_stack {
        assert!(crashed.raw.stack.start_of_memory_range > context.get_stack_pointer());
        let memory = dump.get_memory().unwrap();
        assert!(
            crashed
                .stack_memory(&memory)
                .unwrap()
                .bytes()
                .iter()
                .all(|byte| *byte == 0x5a)
        );
    }
    assert!(threads.threads.iter().any(|thread| {
        thread.raw.thread_id != exception.thread_id && thread.raw.stack.memory.data_size > 0
    }));
}
