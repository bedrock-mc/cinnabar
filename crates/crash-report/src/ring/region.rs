//! A file-backed shared mapping: what the process stores in it is in the file's pages, which the
//! operating system keeps and writes back after the process dies however it dies, so another
//! process can read the file afterwards.

use std::{fs, io, path::Path, ptr::NonNull, sync::atomic::AtomicU64};

/// A zero-filled file of `words` 64-bit words, mapped read-write and shared. Unmapped on drop;
/// the file stays.
pub(crate) struct MappedRegion {
    base: NonNull<AtomicU64>,
    words: usize,
}

// SAFETY: The region is only accessed through atomics, so any thread may use it.
unsafe impl Send for MappedRegion {}
// SAFETY: As above.
unsafe impl Sync for MappedRegion {}

impl MappedRegion {
    /// Creates or truncates the file at `path` to `words` zeroed words and maps it. The file's disk
    /// space is reserved first, so a full disk is an error here instead of a fault on first write.
    pub(crate) fn create(path: &Path, words: usize) -> io::Result<Self> {
        let bytes = words
            .checked_mul(8)
            .filter(|bytes| *bytes > 0)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty region"))?;
        let file = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        platform::reserve(&file, bytes as u64)?;
        let base = platform::map(&file, bytes)?;
        Ok(Self {
            base: base.cast(),
            words,
        })
    }

    /// The mapped words; they start zeroed.
    pub(crate) fn words(&self) -> &[AtomicU64] {
        // SAFETY: The view is `words` words long, page-aligned, and mapped until drop.
        unsafe { std::slice::from_raw_parts(self.base.as_ptr(), self.words) }
    }
}

impl Drop for MappedRegion {
    fn drop(&mut self) {
        platform::unmap(self.base.cast(), self.words * 8);
    }
}

#[cfg(windows)]
mod platform {
    use std::{fs::File, io, os::windows::io::AsRawHandle, ptr::NonNull};

    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Memory::{
            CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS,
            MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile,
        },
    };

    /// Extending a file on Windows allocates its space, so a full disk fails here.
    pub(super) fn reserve(file: &File, bytes: u64) -> io::Result<()> {
        file.set_len(bytes)
    }

    pub(super) fn map(file: &File, bytes: usize) -> io::Result<NonNull<u8>> {
        let size = bytes as u64;
        // SAFETY: The handle is the open file's; a null name makes an unnamed mapping.
        let mapping = unsafe {
            CreateFileMappingW(
                file.as_raw_handle(),
                std::ptr::null(),
                PAGE_READWRITE,
                (size >> 32) as u32,
                size as u32,
                std::ptr::null(),
            )
        };
        if mapping.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `mapping` is a valid section at least `bytes` long.
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, bytes) };
        let error = io::Error::last_os_error();
        // SAFETY: The view keeps the section alive without this handle.
        unsafe { CloseHandle(mapping) };
        NonNull::new(view.Value.cast()).ok_or(error)
    }

    pub(super) fn unmap(base: NonNull<u8>, _bytes: usize) {
        // SAFETY: `base` is a view `map` returned that is not used after this.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: base.as_ptr().cast(),
            })
        };
    }
}

#[cfg(unix)]
mod platform {
    use std::{fs::File, io, os::fd::AsRawFd, ptr::NonNull};

    /// Allocates the file's blocks before it is mapped. A sparse shared mapping would allocate on
    /// the first write to each page, and a full disk or quota then kills the process with SIGBUS.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(super) fn reserve(file: &File, bytes: u64) -> io::Result<()> {
        let len = libc::off_t::try_from(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "region too large"))?;
        // SAFETY: The descriptor is the open file's; `posix_fallocate` returns its error code.
        match unsafe { libc::posix_fallocate(file.as_raw_fd(), 0, len) } {
            0 => Ok(()),
            // A filesystem without `fallocate` (bionic does not emulate it) still gets real blocks.
            libc::EOPNOTSUPP => write_zeros(file, bytes),
            code => Err(io::Error::from_raw_os_error(code)),
        }
    }

    /// Allocates the file's blocks by writing them, so a full disk fails here instead of with
    /// SIGBUS on the first write through the mapping.
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub(super) fn reserve(file: &File, bytes: u64) -> io::Result<()> {
        write_zeros(file, bytes)
    }

    /// Writes `bytes` zeros from the start of the empty file.
    fn write_zeros(mut file: &File, bytes: u64) -> io::Result<()> {
        use std::io::Read;
        let written = io::copy(&mut io::repeat(0).take(bytes), &mut file)?;
        debug_assert_eq!(written, bytes);
        Ok(())
    }

    pub(super) fn map(file: &File, bytes: usize) -> io::Result<NonNull<u8>> {
        // SAFETY: Maps `bytes` of the open file shared; the result is checked below.
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        NonNull::new(base.cast()).ok_or_else(io::Error::last_os_error)
    }

    pub(super) fn unmap(base: NonNull<u8>, bytes: usize) {
        // SAFETY: `base` and `bytes` describe a mapping `map` made that is not used after this.
        unsafe { libc::munmap(base.as_ptr().cast(), bytes) };
    }
}
