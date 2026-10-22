//! Locates a reporting session after another client has archived its open logs.

use super::{CORE_SESSION_START, HEADER_BYTES, read_since, read_tail};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

const SESSION_FIELD: &str = "session=";

/// Whether the retained header prefix belongs to `session_id`, using a bounded header read.
fn belongs(file: &mut File, marker: &str, session_id: &str) -> io::Result<bool> {
    let mut header = [0; HEADER_BYTES];
    let count = file.read(&mut header)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(matches_header(&header[..count], marker, session_id))
}

/// Matches a bounded header prefix without allocation or IO, including in signal handlers.
pub fn matches_header(header: &[u8], marker: &str, session_id: &str) -> bool {
    let line = header
        .split(|byte| *byte == b'\n')
        .rfind(|line| line.starts_with(marker.as_bytes()))
        .unwrap_or_default();
    let Ok(line) = std::str::from_utf8(line) else {
        return false;
    };
    line.split_ascii_whitespace()
        .any(|field| field.strip_prefix(SESSION_FIELD) == Some(session_id))
}

/// Opens this session's current or retained segments, in archive order, with the current file last.
fn files(path: &Path, marker: &str, session_id: &str) -> io::Result<Vec<File>> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = path.extension();
    let mut files = Vec::new();
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let candidate = entry.path();
        let name = entry.file_name();
        let key = if candidate == path {
            (true, String::new(), 0)
        } else {
            let Some((stamp, suffix)) = crate::log_archive::key(
                &name.to_string_lossy(),
                &stem,
                &ext.unwrap_or_default().to_string_lossy(),
            ) else {
                continue;
            };
            (false, stamp, suffix)
        };
        let Ok(mut file) = File::open(&candidate) else {
            continue;
        };
        if !file.metadata()?.is_file() {
            continue;
        }
        if !belongs(&mut file, marker, session_id)? {
            continue;
        }
        files.push((key, file));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files.into_iter().map(|(_, file)| file).collect())
}

/// Keeps an open reference to the newest owned segment before a signal can arrive.
#[cfg_attr(
    not(any(unix, test)),
    expect(
        dead_code,
        reason = "only Unix signal reports open segments ahead of a fault"
    )
)]
pub(crate) fn open(path: &Path, marker: &str, session_id: Option<&str>) -> Option<File> {
    match session_id {
        Some(session_id) => files(path, marker, session_id).ok()?.pop().or_else(|| {
            (marker == CORE_SESSION_START && !identified(path))
                .then(|| File::open(path).ok())
                .flatten()
        }),
        None => File::open(path).ok(),
    }
}

/// Reads only the reporting client's retained segments, bounded across all of them.
pub(crate) fn read(
    path: &Path,
    marker: &str,
    session_id: &str,
    max_bytes: u64,
) -> io::Result<Vec<u8>> {
    let mut combined = Vec::new();
    for file in files(path, marker, session_id)? {
        let part = read_tail(file, marker, max_bytes)?;
        combined.extend_from_slice(&part);
        let excess = combined
            .len()
            .saturating_sub(usize::try_from(max_bytes).unwrap_or(usize::MAX));
        if excess > 0 {
            combined.drain(..excess);
        }
    }
    Ok(combined)
}

/// Whether a current log explicitly names a client, preventing a legacy timestamp fallback.
fn identified(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return true;
    };
    let mut header = [0; HEADER_BYTES];
    let Ok(count) = file.read(&mut header) else {
        return true;
    };
    String::from_utf8_lossy(&header[..count])
        .split_ascii_whitespace()
        .any(|field| field.starts_with(SESSION_FIELD))
}

/// Selects a core log by client identity, with timestamp fallback for older untagged logs.
pub(crate) fn core(
    path: &Path,
    session_id: &str,
    max_bytes: u64,
    since_ms: u64,
) -> io::Result<Vec<u8>> {
    let selected = read(path, CORE_SESSION_START, session_id, max_bytes)?;
    if selected.is_empty() && !identified(path) {
        read_since(path, CORE_SESSION_START, max_bytes, since_ms)
    } else {
        Ok(selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_audit_equal_mtimes_preserve_rotation_order() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("core.log");
        for (name, event) in [
            ("core-20260101-000000.log", "first"),
            ("core-20260101-000000-2.log", "second"),
            ("core-20260101-000000-10.log", "third"),
            ("core.log", "last"),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, format!("CORE_SESSION_START session=own\n{event}\n")).unwrap();
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH)
                .unwrap();
        }
        let text = String::from_utf8(core(&current, "own", 1024, 0).unwrap()).unwrap();
        let positions: Vec<_> = ["first", "second", "third", "last"]
            .map(|event| text.find(event).unwrap())
            .into();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{text}");
    }

    #[test]
    fn core_logs_follow_the_reporting_client_across_other_launches() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("core.log");
        let own = dir.path().join("core-20260101-000000.log");
        fs::write(
            &own,
            "CORE_SESSION_START timestamp_ms=10 session=first\nown core\n",
        )
        .unwrap();
        fs::write(
            &current,
            "CORE_SESSION_START timestamp_ms=20 session=second\nother core\n",
        )
        .unwrap();
        assert_eq!(
            core(&current, "first", 1024, 10).unwrap(),
            fs::read(&own).unwrap()
        );
        assert!(core(&current, "missing", 1024, 0).unwrap().is_empty());
        let mut opened = open(&current, CORE_SESSION_START, Some("first")).unwrap();
        let mut bytes = Vec::new();
        opened.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, fs::read(&own).unwrap());
    }
}
