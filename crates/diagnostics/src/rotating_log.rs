//! A bounded current log and one previous generation.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub(crate) struct RotatingLog {
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
    limit: u64,
}

impl RotatingLog {
    /// Starts a new session, retaining the previous session in a single backup.
    pub(crate) fn open(path: &Path, limit: u64) -> io::Result<Self> {
        if limit == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "log limit must be positive",
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if path.exists() {
            bound_existing(path, limit)?;
            replace_backup(path)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        Ok(Self {
            path: path.into(),
            file: Some(file),
            bytes: 0,
            limit,
        })
    }
}

impl Write for RotatingLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let retained = if bytes.len() as u64 > self.limit {
            &bytes[bytes.len() - self.limit as usize..]
        } else {
            bytes
        };
        if self.bytes + retained.len() as u64 > self.limit {
            if let Some(file) = &mut self.file {
                file.flush()?;
            }
            drop(self.file.take());
            replace_backup(&self.path)?;
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&self.path)?,
            );
            self.bytes = 0;
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file unavailable after rotation"))?
            .write_all(retained)?;
        self.bytes += retained.len() as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().map_or(Ok(()), Write::flush)
    }
}

/// Keeps the tail of a legacy oversized log before retaining it as the previous generation.
fn bound_existing(path: &Path, limit: u64) -> io::Result<()> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let bytes = file.metadata()?.len();
    if bytes <= limit {
        return Ok(());
    }
    file.seek(SeekFrom::Start(bytes - limit))?;
    let mut tail = Vec::new();
    (&mut file).take(limit).read_to_end(&mut tail)?;
    file.rewind()?;
    file.write_all(&tail)?;
    file.set_len(tail.len() as u64)?;
    Ok(())
}

/// Replaces the only retained generation, including on platforms that cannot overwrite by rename.
fn replace_backup(path: &Path) -> io::Result<()> {
    let backup = path.with_file_name(format!(
        "{}.1",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    match fs::remove_file(&backup) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::rename(path, backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sessions_and_oversized_writes_keep_only_two_bounded_files() {
        let path = std::env::temp_dir().join(format!("cinnabar-rotate-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(
            path.with_file_name(format!("cinnabar-rotate-{}.1", std::process::id())),
        );
        fs::write(&path, b"legacy oversized file").unwrap();
        let mut log = RotatingLog::open(&path, 8).unwrap();
        assert_eq!(
            fs::metadata(path.with_file_name(format!("cinnabar-rotate-{}.1", std::process::id())))
                .unwrap()
                .len(),
            8
        );
        log.write_all(b"first").unwrap();
        log.write_all(b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        let backup = path.with_file_name(format!("cinnabar-rotate-{}.1", std::process::id()));
        assert_eq!(fs::read(&backup).unwrap(), b"first");
        log.write_all(b"0123456789").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"23456789");
        drop(log);
        let _log = RotatingLog::open(&path, 8).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), b"23456789");
        fs::remove_file(&path).unwrap();
        fs::remove_file(backup).unwrap();
    }
}
