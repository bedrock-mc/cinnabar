//! The crashing session's part of an append-only log, found by its session header.

pub mod header;
pub mod session;

use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

/// Prefix of the header line that starts each client session in `client.log`.
pub const CLIENT_SESSION_START: &str = "CLIENT_SESSION_START";
/// Prefix of the header line that starts each core process in `core.log`.
pub const CORE_SESSION_START: &str = "CORE_SESSION_START";
/// Most header bytes inspected to identify a log segment.
pub const HEADER_BYTES: usize = 4096;
/// Largest log section copied into a crash folder.
pub const MAX_SECTION_BYTES: u64 = 1 << 20;

/// Returns `text` from the line holding the last `marker`, or all of `text` without one.
pub fn latest<'a>(text: &'a str, marker: &str) -> &'a str {
    // The cut follows a newline or the start, so it is always a character boundary.
    latest_start(text.as_bytes(), marker).map_or(text, |start| &text[start..])
}

/// Reads the latest session of the log at `path` from at most its final `max_bytes`.
///
/// When the header is older than the bound, the section starts at the first whole line inside
/// it. A missing log yields an empty section.
pub fn read(path: &Path, marker: &str, max_bytes: u64) -> io::Result<Vec<u8>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    read_tail(file, marker, max_bytes)
}

/// Like [`read`], but checks the latest header's timestamp before truncating its section.
pub fn read_since(path: &Path, marker: &str, max_bytes: u64, since_ms: u64) -> io::Result<Vec<u8>> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let timestamp = header::latest_timestamp(marker, &mut [0; 4096], |bytes| file.read(bytes))?;
    if timestamp.is_some_and(|at| at < since_ms) {
        return Ok(Vec::new());
    }
    let length = file.metadata()?.len();
    let offset = length.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(max_bytes).read_to_end(&mut bytes)?;
    let cut = since(&bytes, marker, since_ms, offset > 0);
    bytes.drain(..cut);
    Ok(bytes)
}

/// Selects every section whose header is current, preserving earlier processes of one session.
/// The caller checks the full file's latest timestamp before passing a truncated tail here.
pub fn since(tail: &[u8], marker: &str, since_ms: u64, truncated: bool) -> usize {
    let mut older = false;
    let mut offset = 0;
    for line in tail.split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(marker.as_bytes()) {
            match header::first_timestamp(line, marker) {
                Some(at) if at < since_ms => older = true,
                _ => return offset,
            }
        }
        offset += line.len();
    }
    if older {
        tail.len()
    } else {
        start(tail, marker, truncated)
    }
}

/// Reads a bounded session tail from the same open file used to check its timestamp.
fn read_tail(mut file: fs::File, marker: &str, max_bytes: u64) -> io::Result<Vec<u8>> {
    let length = file.metadata()?.len();
    let offset = length.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(max_bytes).read_to_end(&mut bytes)?;
    let cut = start(&bytes, marker, offset > 0);
    bytes.drain(..cut);
    Ok(bytes)
}

/// Offset in `tail`, the final bytes of a log, where its latest session begins.
///
/// Without a header, a `truncated` tail starts at its first whole line and a whole log at 0.
/// Allocation-free, so signal handlers may call it.
pub fn start(tail: &[u8], marker: &str, truncated: bool) -> usize {
    match latest_start(tail, marker) {
        Some(cut) => cut,
        None if truncated => tail
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(tail.len(), |newline| newline + 1),
        None => 0,
    }
}

/// Whether `section` opens with a `marker` header whose `timestamp_ms` precedes `since_ms`.
///
/// Allocation-free, so signal handlers may call it.
pub fn is_stale(section: &[u8], marker: &str, since_ms: u64) -> bool {
    header::first_timestamp(section, marker).is_some_and(|at| at < since_ms)
}

/// Byte offset of the start of the line holding the last `marker`.
fn latest_start(text: &[u8], marker: &str) -> Option<usize> {
    let marker = marker.as_bytes();
    if marker.is_empty() || marker.len() > text.len() {
        return None;
    }
    let at = text
        .windows(marker.len())
        .rposition(|window| window == marker)?;
    Some(
        text[..at]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |newline| newline + 1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_starts_at_the_last_client_session() {
        let text =
            "old\nCLIENT_SESSION_START session=a\nfirst\nCLIENT_SESSION_START session=b\nlast\n";
        assert_eq!(
            latest(text, CLIENT_SESSION_START),
            "CLIENT_SESSION_START session=b\nlast\n"
        );
        assert_eq!(latest("no header\n", CLIENT_SESSION_START), "no header\n");
        assert_eq!(
            latest(
                "x\n12:00 INFO CLIENT_SESSION_START s\ny",
                CLIENT_SESSION_START
            ),
            "12:00 INFO CLIENT_SESSION_START s\ny"
        );
    }

    #[test]
    fn read_extracts_the_session_at_both_markers() {
        let dir = tempfile::tempdir().unwrap();
        let client = dir.path().join("client.log");
        fs::write(
            &client,
            "CLIENT_SESSION_START session=a\nprevious\nCLIENT_SESSION_START session=b\ncurrent\n",
        )
        .unwrap();
        assert_eq!(
            read(&client, CLIENT_SESSION_START, MAX_SECTION_BYTES).unwrap(),
            b"CLIENT_SESSION_START session=b\ncurrent\n"
        );
        let core = dir.path().join("core.log");
        fs::write(
            &core,
            "CORE_SESSION_START timestamp_ms=1\nstale\nCORE_SESSION_START timestamp_ms=30\nlive\n",
        )
        .unwrap();
        assert_eq!(
            read(&core, CORE_SESSION_START, MAX_SECTION_BYTES).unwrap(),
            b"CORE_SESSION_START timestamp_ms=30\nlive\n"
        );
        assert_eq!(
            read_since(&core, CORE_SESSION_START, MAX_SECTION_BYTES, 20).unwrap(),
            b"CORE_SESSION_START timestamp_ms=30\nlive\n"
        );
        assert!(
            read_since(&core, CORE_SESSION_START, MAX_SECTION_BYTES, 31)
                .unwrap()
                .is_empty()
        );
        assert!(
            read(&dir.path().join("missing.log"), CORE_SESSION_START, 64)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn sections_are_bounded_to_the_final_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("client.log");
        let mut text = String::from("CLIENT_SESSION_START session=a\n");
        for line in 0..100 {
            text.push_str(&format!("line {line:03}\n"));
        }
        fs::write(&log, &text).unwrap();
        let section = read(&log, CLIENT_SESSION_START, 30).unwrap();
        assert!(section.len() <= 30);
        assert_eq!(section, b"line 097\nline 098\nline 099\n");
        let whole = read(&log, CLIENT_SESSION_START, text.len() as u64).unwrap();
        assert_eq!(whole, text.as_bytes());
        // A header inside the bound still wins over the byte limit.
        fs::write(&log, format!("{text}CLIENT_SESSION_START session=b\nnew\n")).unwrap();
        assert_eq!(
            read(&log, CLIENT_SESSION_START, 64).unwrap(),
            b"CLIENT_SESSION_START session=b\nnew\n"
        );
    }

    /// Session age is checked even when the section header is outside the retained tail.
    #[test]
    fn read_since_rejects_an_oversized_previous_core_session() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("core.log");
        let payload = "old line\n".repeat(128);
        for (timestamp, stale) in [(1, true), (50, false)] {
            fs::write(
                &log,
                format!("{CORE_SESSION_START} timestamp_ms={timestamp}\n{payload}"),
            )
            .unwrap();
            let section = read_since(&log, CORE_SESSION_START, 64, 20).unwrap();
            assert_eq!(section.is_empty(), stale);
            assert!(section.len() <= 64);
        }
    }

    #[test]
    fn read_since_keeps_all_current_core_processes() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("core.log");
        fs::write(&log, "CORE_SESSION_START timestamp_ms=1\nold\nCORE_SESSION_START timestamp_ms=20\naccount\nCORE_SESSION_START timestamp_ms=30\ngame\n").unwrap();
        assert_eq!(read_since(&log, CORE_SESSION_START, MAX_SECTION_BYTES, 10).unwrap(), b"CORE_SESSION_START timestamp_ms=20\naccount\nCORE_SESSION_START timestamp_ms=30\ngame\n");
    }

    #[test]
    fn in_memory_tails_cut_like_files() {
        let tail = b"ial line\nCORE_SESSION_START timestamp_ms=7\nlive\n";
        assert_eq!(start(tail, CORE_SESSION_START, true), 9);
        assert_eq!(start(b"ial line\nnext\n", CORE_SESSION_START, true), 9);
        assert_eq!(start(b"first\nnext\n", CORE_SESSION_START, false), 0);
        assert_eq!(start(b"no newline", CORE_SESSION_START, true), 10);
        let section = &tail[9..];
        assert!(is_stale(section, CORE_SESSION_START, 8));
        assert!(!is_stale(section, CORE_SESSION_START, 7));
        assert!(!is_stale(b"live\n", CORE_SESSION_START, u64::MAX));
    }
}
