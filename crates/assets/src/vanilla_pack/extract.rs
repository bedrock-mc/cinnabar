//! Parallel extraction of a validated plan, re-enforcing the byte caps on actual output because
//! a hostile central directory can lie.

use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use rayon::prelude::*;
use zip::ZipArchive;

use super::{
    UnpackError, UnpackLimits,
    entries::{self, PlannedFile, zip_error},
    rejected,
};

const COPY_BUFFER_BYTES: usize = 256 << 10;

pub(super) fn extract(
    archive: &Path,
    staging: &Path,
    limits: &UnpackLimits,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<(), UnpackError> {
    let reader = SharedFile::open(archive)
        .map_err(UnpackError::io(format!("open {}", archive.display())))?;
    let mut zip = ZipArchive::new(reader.clone()).map_err(zip_error)?;
    let plan = entries::plan(&mut zip, &mut reader.clone(), limits)?;
    for directory in &plan.directories {
        let path = staging.join(directory);
        fs::create_dir_all(&path).map_err(UnpackError::io(format!("create {}", path.display())))?;
    }
    let total = AtomicU64::new(0);
    plan.files.par_iter().try_for_each_init(
        || (zip.clone(), vec![0; COPY_BUFFER_BYTES]),
        |(zip, buffer), file| {
            if cancelled() {
                return Err(UnpackError::Cancelled);
            }
            write_entry(zip, file, staging, limits, &total, buffer)
        },
    )
}

fn write_entry(
    zip: &mut ZipArchive<SharedFile>,
    file: &PlannedFile,
    staging: &Path,
    limits: &UnpackLimits,
    total: &AtomicU64,
    buffer: &mut [u8],
) -> Result<(), UnpackError> {
    let raw = &file.raw;
    let mut entry = zip.by_index(file.index).map_err(zip_error)?;
    let destination = staging.join(&file.relative);
    let mut output = File::create_new(&destination)
        .map_err(UnpackError::io(format!("create {}", destination.display())))?;
    let mut written = 0u64;
    loop {
        let read = entry
            .read(buffer)
            .map_err(UnpackError::io(format!("read ZIP entry '{raw}'")))?;
        if read == 0 {
            return Ok(());
        }
        output
            .write_all(&buffer[..read])
            .map_err(UnpackError::io(format!("write {}", destination.display())))?;
        written += read as u64;
        if written > limits.max_file_bytes {
            return Err(rejected(format!(
                "ZIP entry '{raw}' expanded size exceeded the maximum {} bytes during extraction",
                limits.max_file_bytes
            )));
        }
        if total.fetch_add(read as u64, Ordering::Relaxed) + read as u64 > limits.max_total_bytes {
            return Err(rejected(format!(
                "archive total expanded size exceeded the maximum {} bytes during extraction",
                limits.max_total_bytes
            )));
        }
    }
}

/// One open archive shared by every worker through positional reads, so clones are cheap and all
/// read the file that was validated.
#[derive(Clone)]
struct SharedFile {
    file: Arc<File>,
    len: u64,
    position: u64,
}

impl SharedFile {
    fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            file: Arc::new(file),
            len,
            position: 0,
        })
    }
}

impl Read for SharedFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        #[cfg(unix)]
        let read = std::os::unix::fs::FileExt::read_at(&*self.file, buffer, self.position)?;
        #[cfg(windows)]
        let read = std::os::windows::fs::FileExt::seek_read(&*self.file, buffer, self.position)?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for SharedFile {
    fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
        let position = match target {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(delta) => self.len.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        };
        self.position = position.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the archive",
            )
        })?;
        Ok(self.position)
    }
}
