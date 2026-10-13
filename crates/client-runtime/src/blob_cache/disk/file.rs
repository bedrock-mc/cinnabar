use redb::StorageBackend;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Mutex,
};

/// An exclusively leased file that cannot grow past the cache's physical byte ceiling.
#[derive(Debug)]
pub(super) struct CappedFile {
    file: Mutex<File>,
    cap: u64,
}

impl CappedFile {
    /// Opens the cache under a process-wide file lease; competing clients use memory only.
    pub(super) fn open(path: &Path, cap: u64) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.try_lock().map_err(io::Error::other)?;
        if file.metadata()?.len() > cap {
            return Err(io::Error::other("blob cache file exceeds its byte limit"));
        }
        Ok(Self {
            file: Mutex::new(file),
            cap,
        })
    }

    /// Rejects overflow and writes outside the file's hard cap before changing the file.
    fn check_end(&self, offset: u64, bytes: usize) -> io::Result<()> {
        if offset
            .checked_add(bytes as u64)
            .is_none_or(|end| end > self.cap)
        {
            Err(io::Error::other("blob cache disk limit reached"))
        } else {
            Ok(())
        }
    }
}

impl StorageBackend for CappedFile {
    /// Returns the current physical allocation length.
    fn len(&self) -> io::Result<u64> {
        self.file
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .metadata()
            .map(|m| m.len())
    }

    /// Reads only on the cache worker, using one file cursor protected from concurrent access.
    fn read(&self, offset: u64, out: &mut [u8]) -> io::Result<()> {
        self.check_end(offset, out.len())?;
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(out)
    }

    /// Enforces the physical byte cap even during a transaction or database growth.
    fn set_len(&self, len: u64) -> io::Result<()> {
        self.check_end(len, 0)?;
        self.file
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_len(len)
    }

    /// Makes a completed transaction durable on the cache worker.
    fn sync_data(&self) -> io::Result<()> {
        self.file
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sync_data()
    }

    /// Writes bounded transaction pages on the cache worker.
    fn write(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        self.check_end(offset, data.len())?;
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)
    }
}
