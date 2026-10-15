//! The crashing session's part of an append-only log, found by its session header.

use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

/// Prefix of the header line that starts each client session in `client.log`.
pub const CLIENT_SESSION_START: &str = "CLIENT_SESSION_START";
/// Prefix of the header line that starts each core process in `core.log`.
pub const CORE_SESSION_START: &str = "CORE_SESSION_START";
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
    let (mut bytes, truncated) = read_tail(path, max_bytes)?;
    let cut = start(&bytes, marker, truncated);
    bytes.drain(..cut);
    Ok(bytes)
}

/// Reads every section of the log at `path` that began at or after `since_ms`, from at most
/// its final `max_bytes`; see [`since`]. A missing log yields an empty section.
pub fn read_since(path: &Path, marker: &str, max_bytes: u64, since_ms: u64) -> io::Result<Vec<u8>> {
    let (mut bytes, truncated) = read_tail(path, max_bytes)?;
    let cut = since(&bytes, marker, since_ms, truncated);
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

/// Offset in `tail`, the final bytes of a log, of the first `marker` header line whose
/// `timestamp_ms` is at or after `since_ms`, so one session's several sections stay together.
///
/// A header without a readable timestamp counts as current. A tail whose headers all precede
/// `since_ms` yields its length: nothing in it belongs to the session. A tail without any
/// header falls back to [`start`]. Allocation-free, so signal handlers may call it.
pub fn since(tail: &[u8], marker: &str, since_ms: u64, truncated: bool) -> usize {
    let mut older = false;
    let mut line_start = 0;
    while line_start < tail.len() {
        let line_end = tail[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(tail.len(), |newline| line_start + newline + 1);
        let line = &tail[line_start..line_end];
        if contains(line, marker) {
            match header_timestamp_ms(line) {
                Some(at) if at < since_ms => older = true,
                _ => return line_start,
            }
        }
        line_start = line_end;
    }
    if older {
        tail.len()
    } else {
        start(tail, marker, truncated)
    }
}

/// The final `max_bytes` of the log at `path`, and whether earlier bytes were left out.
fn read_tail(path: &Path, max_bytes: u64) -> io::Result<(Vec<u8>, bool)> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), false)),
        Err(error) => return Err(error),
    };
    let length = file.metadata()?.len();
    let offset = length.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(max_bytes).read_to_end(&mut bytes)?;
    Ok((bytes, offset > 0))
}

/// The `timestamp_ms=` value on a header line; the line may keep its trailing newline.
fn header_timestamp_ms(line: &[u8]) -> Option<u64> {
    let line = line.split(|byte| *byte == b'\n').next()?;
    let line = std::str::from_utf8(line).ok()?;
    let value = line.split_once("timestamp_ms=")?.1;
    let digits = value
        .find(|character: char| !character.is_ascii_digit())
        .map_or(value, |end| &value[..end]);
    digits.parse().ok()
}

/// Whether `text` holds `marker` anywhere; an empty marker matches nothing.
fn contains(text: &[u8], marker: &str) -> bool {
    let marker = marker.as_bytes();
    !marker.is_empty()
        && marker.len() <= text.len()
        && text.windows(marker.len()).any(|window| window == marker)
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
    }

    #[test]
    fn read_since_keeps_every_core_of_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let core = dir.path().join("core.log");
        fs::write(
            &core,
            "CORE_SESSION_START timestamp_ms=1\nprevious session\nCORE_SESSION_START timestamp_ms=20\naccount core\nCORE_SESSION_START timestamp_ms=40\ngame core\n",
        )
        .unwrap();
        assert_eq!(
            read_since(&core, CORE_SESSION_START, MAX_SECTION_BYTES, 10).unwrap(),
            b"CORE_SESSION_START timestamp_ms=20\naccount core\nCORE_SESSION_START timestamp_ms=40\ngame core\n"
        );
        assert_eq!(
            read_since(&core, CORE_SESSION_START, MAX_SECTION_BYTES, 20).unwrap(),
            read_since(&core, CORE_SESSION_START, MAX_SECTION_BYTES, 10).unwrap()
        );
        assert_eq!(
            read_since(&core, CORE_SESSION_START, MAX_SECTION_BYTES, 21).unwrap(),
            b"CORE_SESSION_START timestamp_ms=40\ngame core\n"
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

    #[test]
    fn in_memory_tails_cut_like_files() {
        let tail = b"ial line\nCORE_SESSION_START timestamp_ms=7\nlive\n";
        assert_eq!(start(tail, CORE_SESSION_START, true), 9);
        assert_eq!(start(b"ial line\nnext\n", CORE_SESSION_START, true), 9);
        assert_eq!(start(b"first\nnext\n", CORE_SESSION_START, false), 0);
        assert_eq!(start(b"no newline", CORE_SESSION_START, true), 10);
        assert_eq!(since(tail, CORE_SESSION_START, 7, true), 9);
        assert_eq!(since(tail, CORE_SESSION_START, 8, true), tail.len());
        // A truncated header-free tail is the middle of a session that began before it.
        assert_eq!(since(b"ial line\nnext\n", CORE_SESSION_START, 8, true), 9);
        assert_eq!(since(b"first\nnext\n", CORE_SESSION_START, 8, false), 0);
        let untimed = b"old\nCORE_SESSION_START\nlive\n";
        assert_eq!(since(untimed, CORE_SESSION_START, u64::MAX, false), 4);
    }
}
