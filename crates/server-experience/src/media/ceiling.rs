//! Memory ceiling for the media decoder process. The helper binary installs
//! [`BoundedAllocator`] as its global allocator; [`contain_process`] arms it and adds the OS
//! limit, and the parent kills a child whose footprint exceeds [`HELPER_MEMORY_BYTES`].

use anyhow::{Result, ensure};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

/// Whole-process ceiling: kernel data limit on Linux, parent footprint watchdog on macOS.
pub const HELPER_MEMORY_BYTES: u64 = 512 * 1024 * 1024;
/// Rust heap ceiling inside the helper; native codec pools fall under the process ceiling.
pub const HELPER_HEAP_BYTES: usize = 192 * 1024 * 1024;

/// Counts live Rust heap bytes; once armed, an allocation past the limit fails (and aborts).
pub struct BoundedAllocator {
    used: AtomicUsize,
    limit: AtomicUsize, // zero while unarmed
}

impl BoundedAllocator {
    #[allow(
        clippy::new_without_default,
        reason = "const constructor for a global allocator"
    )]
    pub const fn new() -> Self {
        Self {
            used: AtomicUsize::new(0),
            limit: AtomicUsize::new(0),
        }
    }

    /// Caps live heap bytes from now on; already-live allocations count against the cap.
    pub fn arm(&self, limit: usize) {
        self.limit.store(limit.max(1), Ordering::Release);
    }

    pub fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }

    fn reserve(&self, bytes: usize) -> bool {
        let limit = self.limit.load(Ordering::Acquire);
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|next| limit == 0 || *next <= limit)
            })
            .is_ok()
    }

    fn release(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::AcqRel);
    }
}

// SAFETY: every path forwards to `System` with the caller's layout; accounting never alters it.
unsafe impl GlobalAlloc for BoundedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !self.reserve(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: forwarded unchanged under the caller's GlobalAlloc contract.
        let pointer = unsafe { System.alloc(layout) };
        if pointer.is_null() {
            self.release(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !self.reserve(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: forwarded unchanged under the caller's GlobalAlloc contract.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if pointer.is_null() {
            self.release(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` was returned by this allocator for `layout`.
        unsafe { System.dealloc(pointer, layout) };
        self.release(layout.size());
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        if new_size > old && !self.reserve(new_size - old) {
            return std::ptr::null_mut();
        }
        // SAFETY: forwarded unchanged under the caller's GlobalAlloc contract.
        let next = unsafe { System.realloc(pointer, layout, new_size) };
        if next.is_null() {
            if new_size > old {
                self.release(new_size - old);
            }
        } else if new_size < old {
            self.release(old - new_size);
        }
        next
    }
}

/// Proof that this process runs under the media ceiling; decoding requires it. Only this
/// module's parent builds one directly, for trusted fixtures in tests.
pub struct Contained(pub(super) ());

/// Arms `allocator` (which must be the process's global allocator) and the OS data limit.
pub fn contain_process(allocator: &'static BoundedAllocator) -> Result<Contained> {
    ensure!(
        !cfg!(windows),
        "media helper memory ceiling is not implemented on Windows"
    );
    let before = allocator.used();
    let probe = std::hint::black_box(vec![0u8; 4096]);
    ensure!(
        allocator.used() >= before + probe.len(),
        "bounded allocator is not the global allocator"
    );
    drop(probe);
    allocator.arm(HELPER_HEAP_BYTES);
    #[cfg(target_os = "linux")]
    {
        let limit = libc::rlimit {
            rlim_cur: HELPER_MEMORY_BYTES as libc::rlim_t,
            rlim_max: HELPER_MEMORY_BYTES as libc::rlim_t,
        };
        // SAFETY: plain syscall on a fully initialized struct.
        ensure!(
            unsafe { libc::setrlimit(libc::RLIMIT_DATA, &limit) } == 0,
            "RLIMIT_DATA unavailable"
        );
    }
    Ok(Contained(()))
}

/// Physical footprint of a child process, for the parent's watchdog.
#[cfg(target_os = "macos")]
pub fn footprint_bytes(pid: u32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: the buffer is a correctly sized rusage_info_v2 for flavor V2.
    let status = unsafe {
        libc::proc_pid_rusage(
            i32::try_from(pid).ok()?,
            libc::RUSAGE_INFO_V2,
            info.as_mut_ptr().cast::<libc::rusage_info_t>(),
        )
    };
    // SAFETY: a zero status means the kernel filled the struct.
    (status == 0).then(|| unsafe { info.assume_init() }.ri_phys_footprint)
}

/// Resident set of a child process, for the parent's watchdog.
#[cfg(target_os = "linux")]
pub fn footprint_bytes(pid: u32) -> Option<u64> {
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: sysconf has no memory-safety preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    pages.checked_mul(u64::try_from(page).ok()?)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn footprint_bytes(_pid: u32) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armed_allocator_refuses_growth_past_its_ceiling_and_recovers_after_free() {
        let allocator = BoundedAllocator::new();
        allocator.arm(1024);
        let small = Layout::from_size_align(512, 8).unwrap();
        let large = Layout::from_size_align(2048, 8).unwrap();
        // SAFETY: every successful allocation is freed with its own layout.
        unsafe {
            let first = allocator.alloc(small);
            assert!(!first.is_null());
            assert!(allocator.alloc(large).is_null());
            assert!(allocator.realloc(first, small, 2048).is_null());
            assert_eq!(allocator.used(), 512);
            let grown = allocator.realloc(first, small, 1024);
            assert!(!grown.is_null());
            assert!(allocator.alloc_zeroed(small).is_null());
            allocator.dealloc(grown, Layout::from_size_align(1024, 8).unwrap());
            assert_eq!(allocator.used(), 0);
            let again = allocator.alloc(small);
            assert!(!again.is_null());
            allocator.dealloc(again, small);
        }
    }

    #[test]
    fn helper_ceiling_aborts_an_oversized_allocation_in_a_real_process() {
        const CHILD: &str = "CINNABAR_MEDIA_CEILING_CHILD";
        if std::env::var_os(CHILD).is_some() {
            crate::TEST_ALLOCATOR.arm(crate::TEST_ALLOCATOR.used() + 8 * 1024 * 1024);
            let oversized = std::hint::black_box(vec![1u8; 64 * 1024 * 1024]);
            println!("ceiling ignored {}", oversized.len());
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "media::ceiling::tests::helper_ceiling_aborts_an_oversized_allocation_in_a_real_process",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!output.status.success(), "child survived: {stdout}");
        assert!(!stdout.contains("ceiling ignored"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("memory allocation"));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn watchdog_reads_a_live_process_footprint() {
        let bytes = footprint_bytes(std::process::id()).unwrap();
        assert!(bytes > 1024 * 1024 && bytes < HELPER_MEMORY_BYTES * 8);
    }
}
