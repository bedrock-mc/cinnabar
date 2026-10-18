//! Per-session log files: the current session writes `<stem>.<ext>`; opening a new session moves
//! the previous file to `<stem>-<UTC yyyymmdd-hhmmss>[-N].<ext>` and prunes those archives to a
//! count and byte budget, deleting the oldest first.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

/// Size and retention bounds for one family of session files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention {
    /// Largest current file; a session passing it archives what it wrote and continues in a
    /// fresh file that starts with the same header.
    pub session_bytes: u64,
    /// Most archived files kept.
    pub archives: usize,
    /// Most bytes kept across archived files.
    pub archive_bytes: u64,
}

impl Retention {
    /// The client and setup logs: 8 MiB per file, ten archives, 64 MiB of archives.
    pub const DEFAULT: Self = Self {
        session_bytes: 8 << 20,
        archives: 10,
        archive_bytes: 64 << 20,
    };
}

/// Where a session family lives: `dir/<stem>.<ext>` and its archives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    pub dir: PathBuf,
    pub stem: String,
    pub ext: String,
}

impl Family {
    pub fn new(dir: impl Into<PathBuf>, stem: impl Into<String>, ext: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            stem: stem.into(),
            ext: ext.into(),
        }
    }

    /// The current session's file.
    pub fn current(&self) -> PathBuf {
        self.dir.join(format!("{}.{}", self.stem, self.ext))
    }

    /// The single previous generation older builds kept beside the current file.
    fn legacy_backup(&self) -> PathBuf {
        self.dir.join(format!("{}.{}.1", self.stem, self.ext))
    }

    /// Archived sessions, oldest first.
    pub fn archives(&self) -> io::Result<Vec<PathBuf>> {
        let mut found = Vec::new();
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            if let Some(key) = self.archive_key(&name.to_string_lossy()) {
                found.push((key, entry.path()));
            }
        }
        found.sort();
        Ok(found.into_iter().map(|(_, path)| path).collect())
    }

    /// Orders archive names by their stamp, then by their collision suffix.
    fn archive_key(&self, name: &str) -> Option<(String, u32)> {
        crash_report::log_archive::key(name, &self.stem, &self.ext)
    }

    /// An archive path after every retained collision at `at`, so pruned suffixes stay unused.
    fn archive_path(&self, at: SystemTime) -> io::Result<PathBuf> {
        let stamp = utc_stamp(at);
        let newest = self
            .archives()?
            .iter()
            .filter_map(|path| {
                let (existing, suffix) = self.archive_key(path.file_name()?.to_str()?)?;
                (existing == stamp).then_some(suffix)
            })
            .max();
        let suffix = newest.map_or(Ok(0), |suffix| {
            suffix.checked_add(1).ok_or_else(|| {
                io::Error::new(io::ErrorKind::AlreadyExists, "log archive suffix exhausted")
            })
        })?;
        let name = if suffix == 0 {
            format!("{}-{stamp}.{}", self.stem, self.ext)
        } else {
            format!("{}-{stamp}-{suffix}.{}", self.stem, self.ext)
        };
        Ok(self.dir.join(name))
    }

    /// Moves `path` into the archive set when it exists.
    fn archive(&self, path: &Path, at: SystemTime) -> io::Result<()> {
        if !path.exists() {
            return Ok(());
        }
        fs::rename(path, self.archive_path(at)?)
    }

    /// Keeps the newest archives while both the count and byte budgets hold, then deletes the
    /// first archive that would break either budget and every older one. Tries every deletion
    /// and reports the first failure.
    pub fn prune(&self, retention: Retention) -> io::Result<()> {
        let mut kept = 0usize;
        let mut bytes = 0u64;
        let mut full = false;
        let mut failure = None;
        for path in self.archives()?.into_iter().rev() {
            let size = fs::metadata(&path).map_or(0, |metadata| metadata.len());
            full = full || kept >= retention.archives || bytes + size > retention.archive_bytes;
            if !full {
                kept += 1;
                bytes += size;
                continue;
            }
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

/// Formats `at` as a UTC `yyyymmdd-hhmmss` stamp; times before the epoch format as the epoch.
pub fn utc_stamp(at: SystemTime) -> String {
    let seconds = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let (days, of_day) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

/// Converts days since 1970-01-01 to a proleptic Gregorian date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The clock that stamps archives; tests substitute a fixed sequence.
pub type Clock = Box<dyn FnMut() -> SystemTime + Send>;

/// The current session's file, bounded by its [`Retention`].
pub struct SessionLog {
    family: Family,
    retention: Retention,
    header: Vec<u8>,
    file: Option<File>,
    bytes: u64,
    clock: Clock,
    #[cfg(unix)]
    signal_log: bool,
}

impl SessionLog {
    /// Archives the previous session, prunes archives, and starts the current file with
    /// `header` (written verbatim; include its trailing newline).
    pub fn open(family: Family, retention: Retention, header: Vec<u8>) -> io::Result<Self> {
        Self::open_with_clock(family, retention, header, Box::new(SystemTime::now))
    }

    /// [`SessionLog::open`] with an explicit archive clock.
    pub fn open_with_clock(
        family: Family,
        retention: Retention,
        header: Vec<u8>,
        mut clock: Clock,
    ) -> io::Result<Self> {
        if retention.session_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "log limit must be positive",
            ));
        }
        fs::create_dir_all(&family.dir)?;
        let now = clock();
        family.archive(&family.legacy_backup(), now)?;
        family.archive(&family.current(), now)?;
        // An archive that cannot be deleted now (held open elsewhere) must not cost this
        // session its log; the next session retries.
        let _ = family.prune(retention);
        let mut log = Self {
            family,
            retention,
            header,
            file: None,
            bytes: 0,
            clock,
            #[cfg(unix)]
            signal_log: false,
        };
        log.start()?;
        Ok(log)
    }

    /// Follows this log's owned file through rotation for fatal-signal output.
    #[cfg(unix)]
    pub(crate) fn follow_on_signal(&mut self) -> io::Result<()> {
        crate::console::native::signal_drain::publish_log(self.file.as_ref().unwrap())?;
        self.signal_log = true;
        Ok(())
    }

    /// The family this log writes.
    pub fn family(&self) -> &Family {
        &self.family
    }

    /// Creates the current file empty and writes the header. Writes append, so another writer
    /// appending to the same file, such as a fatal-signal drain, never gets overwritten.
    fn start(&mut self) -> io::Result<()> {
        let path = self.family.current();
        // Truncate through a write handle: Windows append-only handles cannot change the length.
        File::create(&path)?;
        let mut file = OpenOptions::new().append(true).open(&path)?;
        file.write_all(&self.header)?;
        #[cfg(unix)]
        if self.signal_log {
            crate::console::native::signal_drain::publish_log(&file)?;
        }
        self.bytes = self.header.len() as u64;
        self.file = Some(file);
        Ok(())
    }

    /// Archives the oversized current file and continues the session in a fresh one. When the
    /// archive cannot be moved, the session keeps appending to the current file.
    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        let now = (self.clock)();
        if let Err(error) = self.family.archive(&self.family.current(), now) {
            self.file = Some(
                OpenOptions::new()
                    .append(true)
                    .open(self.family.current())?,
            );
            return Err(error);
        }
        let _ = self.family.prune(self.retention);
        self.start()
    }
}

impl Write for SessionLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let room = self
            .retention
            .session_bytes
            .saturating_sub(self.header.len() as u64)
            .max(1);
        let retained = if bytes.len() as u64 > room {
            &bytes[bytes.len() - room as usize..]
        } else {
            bytes
        };
        if self.bytes + retained.len() as u64 > self.retention.session_bytes
            && self.bytes > self.header.len() as u64
        {
            self.rotate()?;
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

/// A session log several writers share, such as the output threads of one child process.
#[derive(Clone)]
pub struct SharedLog(Arc<Mutex<SessionLog>>);

impl SharedLog {
    pub fn new(log: SessionLog) -> Self {
        Self(Arc::new(Mutex::new(log)))
    }
}

impl Write for SharedLog {
    /// Writes all of `bytes` under one lock, so concurrent writers never interleave mid-write.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut log = self.0.lock().unwrap_or_else(|error| error.into_inner());
        log.write_all(bytes)?;
        log.flush()?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .flush()
    }
}

#[cfg(test)]
mod tests;
