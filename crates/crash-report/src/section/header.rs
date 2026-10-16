//! Finds session timestamps while streaming a log, without retaining its lines or allocating.

/// The timestamp field emitted in session headers.
const TIMESTAMP_FIELD: &str = "timestamp_ms=";

/// Finds the last session header's timestamp before a caller truncates the log's tail.
/// `read` fills the supplied nonempty scratch buffer; errors propagate to the caller.
/// This helper does not allocate or lock, so a signal-safe reader may use it too.
pub fn latest_timestamp<E>(
    marker: &str,
    scratch: &mut [u8],
    mut read: impl FnMut(&mut [u8]) -> Result<usize, E>,
) -> Result<Option<u64>, E> {
    if scratch.is_empty() || marker.is_empty() {
        return Ok(None);
    }
    let mut scan = HeaderScan::new(marker.as_bytes());
    loop {
        let count = read(scratch)?;
        if count == 0 {
            scan.finish_line();
            return Ok(scan.latest);
        }
        scan.feed(&scratch[..count]);
    }
}

/// Reads only the first line, using the same grammar as the streaming scanner.
pub(super) fn first_timestamp(section: &[u8], marker: &str) -> Option<u64> {
    if marker.is_empty() {
        return None;
    }
    let mut scan = HeaderScan::new(marker.as_bytes());
    scan.feed(section.split(|byte| *byte == b'\n').next()?);
    scan.finish_line();
    scan.latest
}

/// Progress through one line; only lines beginning with the exact marker are headers.
enum Line {
    Marker(usize),
    Field(usize),
    Number { value: Option<u64>, digits: bool },
    Finished(Option<u64>),
    Other,
}

struct HeaderScan<'a> {
    marker: &'a [u8],
    line: Line,
    latest: Option<u64>,
}

impl<'a> HeaderScan<'a> {
    /// Starts at a line boundary without keeping any log bytes.
    fn new(marker: &'a [u8]) -> Self {
        Self {
            marker,
            line: Line::Marker(0),
            latest: None,
        }
    }

    /// Finishes a header even when its timestamp is missing or malformed, replacing older data.
    fn finish_line(&mut self) {
        match self.line {
            Line::Field(_) => self.latest = None,
            Line::Number { value, digits } => self.latest = value.filter(|_| digits),
            Line::Finished(value) => self.latest = value,
            Line::Marker(_) | Line::Other => {}
        }
        self.line = Line::Marker(0);
    }

    /// Consumes arbitrary chunks, preserving partial markers, field names and numbers.
    fn feed(&mut self, bytes: &[u8]) {
        let field = TIMESTAMP_FIELD.as_bytes();
        for &byte in bytes {
            if byte == b'\n' {
                self.finish_line();
                continue;
            }
            self.line = match self.line {
                Line::Marker(index) if self.marker[index] == byte => {
                    if index + 1 == self.marker.len() {
                        Line::Field(0)
                    } else {
                        Line::Marker(index + 1)
                    }
                }
                Line::Marker(_) => Line::Other,
                Line::Field(index) if field[index] == byte => {
                    if index + 1 == field.len() {
                        Line::Number {
                            value: Some(0),
                            digits: false,
                        }
                    } else {
                        Line::Field(index + 1)
                    }
                }
                Line::Field(_) => Line::Field(usize::from(byte == field[0])),
                Line::Number { value, .. } if byte.is_ascii_digit() => Line::Number {
                    value: value.and_then(|value| {
                        value.checked_mul(10)?.checked_add(u64::from(byte - b'0'))
                    }),
                    digits: true,
                },
                Line::Number { value, digits } => Line::Finished(value.filter(|_| digits)),
                Line::Finished(value) => Line::Finished(value),
                Line::Other => Line::Other,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    /// Header fields may cross every read boundary, including the end of the file.
    #[test]
    fn headers_survive_arbitrary_chunk_boundaries_and_long_lines() {
        let text = format!(
            "CORE timestamp_ms=1\n{}\nCORE version=1 timestamp_ms=12345",
            "x".repeat(1024)
        );
        for size in 1..64 {
            let mut source = Cursor::new(text.as_bytes());
            let mut scratch = vec![0; size];
            assert_eq!(
                latest_timestamp("CORE", &mut scratch, |bytes| source.read(bytes)).unwrap(),
                Some(12345)
            );
        }
    }

    /// An unreadable timestamp in the latest header never inherits an older session's time.
    #[test]
    fn malformed_latest_headers_replace_older_timestamps() {
        for header in [
            "CORE",
            "CORE timestamp_ms=",
            "CORE timestamp_ms=no",
            "CORE timestamp_ms=18446744073709551616",
        ] {
            let text = format!("CORE timestamp_ms=1\n{header}\npayload\n");
            let mut source = Cursor::new(text);
            assert_eq!(
                latest_timestamp("CORE", &mut [0; 7], |bytes| source.read(bytes)).unwrap(),
                None
            );
        }
    }
}
