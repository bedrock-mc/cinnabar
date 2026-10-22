//! Main-loop progress the crash monitor watches for hangs.
//!
//! The client maps a small file in its crash directory and the monitor maps the same file, so
//! the monitor sees progress without the client sending anything: a stalled main thread needs no
//! IPC to be noticed. The watched thread counts its frames with [`tick`]; [`Loading`] marks known
//! long blocking phases, which are not hangs. Until the first [`tick`] nothing counts as a hang,
//! so startup before the first frame is never reported.

use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
    sync::{
        OnceLock,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use memmap2::MmapMut;

/// How long the watched thread may go without a [`tick`], outside [`Loading`], before the
/// monitor reports a hang.
pub const STALL_AFTER: Duration = Duration::from_secs(10);

/// Identifies a heartbeat file of this layout.
const MAGIC: u32 = 0x4342_4842;

/// The shared region, at the start of the mapped file.
#[repr(C)]
pub struct Region {
    /// Frames the watched thread finished; written by the client only.
    frame: AtomicU64,
    /// Open [`Loading`] phases; written by the client only.
    loading: AtomicU32,
    /// [`MAGIC`] once the client initialized the region.
    magic: AtomicU32,
    /// The platform id of the watched thread; written by the client only.
    thread: AtomicU64,
    /// Samples the monitor took; written by the monitor only.
    checks: AtomicU64,
    /// The last authenticated dump announcement received; written by the monitor only.
    dump_receipt: AtomicU64,
}

/// Bytes of the heartbeat file.
const REGION_BYTES: u64 = std::mem::size_of::<Region>() as u64;

impl Region {
    /// Frames the watched thread finished.
    pub fn frame(&self) -> u64 {
        self.frame.load(Ordering::Relaxed)
    }

    /// Whether a known long blocking phase is open.
    pub fn loading(&self) -> bool {
        self.loading.load(Ordering::Relaxed) > 0
    }

    /// The platform id of the watched thread: the Win32 thread id, the Linux tid or the macOS
    /// `pthread_threadid_np` value; `0` when unknown.
    pub fn thread(&self) -> u64 {
        self.thread.load(Ordering::Relaxed)
    }

    /// How many samples the monitor took.
    pub fn checks(&self) -> u64 {
        self.checks.load(Ordering::Relaxed)
    }

    /// Counts one monitor sample; only the monitor calls this.
    pub fn record_check(&self) {
        self.checks.fetch_add(1, Ordering::Relaxed);
    }

    /// Acknowledges the exact dump announcement after its preceding socket messages were read.
    pub fn acknowledge_dump(&self, request: u64) {
        self.dump_receipt.store(request, Ordering::Release);
    }

    /// Whether the monitor received this announcement, rather than an earlier request.
    pub fn dump_acknowledged(&self, request: u64) -> bool {
        self.dump_receipt.load(Ordering::Acquire) == request
    }
}

/// A mapped heartbeat file.
pub struct Heartbeat {
    map: MmapMut,
}

impl Heartbeat {
    /// Creates the heartbeat file at `path`, which must not exist, and initializes it with the
    /// calling thread as the watched one.
    pub fn create(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        file.set_len(REGION_BYTES)?;
        let heartbeat = Self::map(&file)?;
        let region = heartbeat.region();
        region.thread.store(current_thread_id(), Ordering::Relaxed);
        region.magic.store(MAGIC, Ordering::Release);
        Ok(heartbeat)
    }

    /// Maps the heartbeat file a client created at `path`.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        if file.metadata()?.len() < REGION_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "heartbeat file is too short",
            ));
        }
        let heartbeat = Self::map(&file)?;
        if heartbeat.region().magic.load(Ordering::Acquire) != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a heartbeat file",
            ));
        }
        Ok(heartbeat)
    }

    fn map(file: &File) -> io::Result<Self> {
        // SAFETY: Only the client and its monitor map this file, and both access it solely through
        // the atomics of `Region`; nobody truncates it while mapped.
        let map = unsafe {
            memmap2::MmapOptions::new()
                .len(REGION_BYTES as usize)
                .map_mut(file)?
        };
        Ok(Self { map })
    }

    /// The shared region.
    pub fn region(&self) -> &Region {
        // SAFETY: The mapping is page-aligned and `REGION_BYTES` long, and `Region` is atomics
        // only, so any bytes are a valid value and concurrent access is sound.
        unsafe { &*self.map.as_ptr().cast::<Region>() }
    }
}

/// This process's heartbeat, which [`tick`] and [`Loading`] write, once [`watch`] created it.
static WATCHED: OnceLock<Heartbeat> = OnceLock::new();

/// Creates a heartbeat file for this process in `crash_dir` named by `pid` and `nonce`, owned
/// by the calling thread, and makes [`tick`] and [`Loading`] write it. Returns its path for the
/// monitor; the mapping lives as long as the process. Fails when a heartbeat already exists.
/// iOS starts no monitor, so it never watches and [`tick`] and [`Loading`] write nothing.
#[cfg(not(target_os = "ios"))]
pub(super) fn watch(crash_dir: &Path, pid: u32, nonce: u64) -> io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(crash_dir)?;
    let path = crash_dir.join(format!(".heartbeat-{pid}-{nonce:016x}"));
    let heartbeat = Heartbeat::create(&path)?;
    if WATCHED.set(heartbeat).is_err() {
        let _ = std::fs::remove_file(&path);
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "this process already has a heartbeat",
        ));
    }
    Ok(path)
}

/// The process-lifetime shared region, also safe to read from native crash handlers.
pub(super) fn watched() -> Option<&'static Region> {
    WATCHED.get().map(Heartbeat::region)
}

/// How many samples the monitor took of this process's heartbeat, so a caller can tell that
/// the watchdog has seen its progress; `0` without a heartbeat.
pub fn monitor_checks() -> u64 {
    watched().map_or(0, Region::checks)
}

/// Counts one finished frame of the watched thread: one relaxed load and store, and nothing
/// before the monitor started. Only the thread that installed crash capture calls this.
pub fn tick() {
    if let Some(region) = watched() {
        let frame = region.frame.load(Ordering::Relaxed);
        region.frame.store(frame.wrapping_add(1), Ordering::Relaxed);
    }
}

/// Marks a known long blocking phase, such as loading carriers, joining a world before its
/// first frame or compiling packs, for as long as it lives; phases may nest and may be held on
/// any thread. The monitor counts a stall from when the last phase ends.
#[must_use = "the phase ends when this is dropped"]
#[derive(Debug)]
pub struct Loading(());

impl Loading {
    /// Opens a loading phase.
    pub fn begin() -> Self {
        if let Some(region) = watched() {
            region.loading.fetch_add(1, Ordering::Relaxed);
        }
        Self(())
    }
}

impl Drop for Loading {
    fn drop(&mut self) {
        if let Some(region) = watched() {
            region.loading.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// The calling thread's platform id, as [`Region::thread`] records it.
fn current_thread_id() -> u64 {
    #[cfg(windows)]
    {
        // SAFETY: No preconditions.
        u64::from(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() })
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // SAFETY: No preconditions.
        u64::try_from(unsafe { libc::syscall(libc::SYS_gettid) }).unwrap_or(0)
    }
    #[cfg(target_vendor = "apple")]
    {
        let mut id = 0_u64;
        // SAFETY: A null thread means the calling one; `id` is a valid out pointer.
        unsafe { libc::pthread_threadid_np(0, &mut id) };
        id
    }
    #[cfg(not(any(windows, unix)))]
    {
        0
    }
}
