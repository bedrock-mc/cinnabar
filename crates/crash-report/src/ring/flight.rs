//! The log flight recorder ring: the last [`CAPACITY`] formatted log events, each a line of at
//! most [`MAX_TEXT_BYTES`] bytes with the Unix milliseconds it was recorded at. Recording copies
//! the line and never allocates.
//!
//! File layout after the shared ring header (see [`super`]): word 2 counts records accepted;
//! each slot is [`SLOT_WORDS`] words: stamp, Unix milliseconds, text length, then the text packed
//! eight bytes to a little-endian word.

use std::{
    io,
    path::Path,
    sync::atomic::{AtomicU64, Ordering, fence},
};

use super::{HEADER_WORDS, Words};

/// Lines the ring keeps; older ones are overwritten.
pub const CAPACITY: usize = 2_000;
/// The ring file's format version.
pub const VERSION: u32 = 1;
/// The first word of a flight ring file.
pub const MAGIC: u64 = u64::from_le_bytes(*b"CNBRFLT\0");
/// Words per slot, so 512 bytes.
pub const SLOT_WORDS: usize = 64;
/// Longest line a slot holds; longer lines are cut to this many bytes.
pub const MAX_TEXT_BYTES: usize = (SLOT_WORDS - TEXT_WORD) * 8;
/// Words in a flight ring file.
pub const WORDS: usize = HEADER_WORDS + CAPACITY * SLOT_WORDS;
const NEXT_WORD: usize = 2;
const TIME_WORD: usize = 1;
const LEN_WORD: usize = 2;
const TEXT_WORD: usize = 3;
const LAYOUT: u64 = super::layout(VERSION, CAPACITY);
/// The state byte of a published line.
const LINE: u8 = 1;

/// A log flight recorder ring, in memory until [`FlightRing::attach`].
pub struct FlightRing {
    words: Words<WORDS>,
}

impl Default for FlightRing {
    fn default() -> Self {
        Self::new()
    }
}

impl FlightRing {
    pub const fn new() -> Self {
        Self {
            words: Words::new(),
        }
    }

    /// Moves the ring, with the lines so far, into a new mapped ring file at `path`; see
    /// [`super`]. A second attach fails and leaves the first file alone.
    pub fn attach(&self, path: &Path) -> io::Result<()> {
        self.words.attach(path, MAGIC, LAYOUT)
    }

    /// Appends `text`, cut to [`MAX_TEXT_BYTES`], recorded at `unix_ms`. In the race the
    /// [`super`] module describes the line is given up.
    pub fn record(&self, unix_ms: u64, text: &[u8]) {
        let words = self.words.get();
        let sequence = words[NEXT_WORD].fetch_add(1, Ordering::Relaxed) + 1;
        let slot = slot(words, sequence);
        if !super::claim(&slot[0], sequence) {
            return;
        }
        let text = &text[..text.len().min(MAX_TEXT_BYTES)];
        slot[TIME_WORD].store(unix_ms, Ordering::Relaxed);
        slot[LEN_WORD].store(text.len() as u64, Ordering::Relaxed);
        for (word, chunk) in slot[TEXT_WORD..].iter().zip(text.chunks(8)) {
            let mut bytes = [0; 8];
            bytes[..chunk.len()].copy_from_slice(chunk);
            word.store(u64::from_le_bytes(bytes), Ordering::Relaxed);
        }
        slot[0].store(super::published(sequence, LINE), Ordering::Release);
    }

    /// Lines accepted so far, including those overwritten.
    pub fn recorded(&self) -> u64 {
        recorded(self.words.get())
    }

    /// The retained lines, oldest first; see [`read`].
    pub fn lines(&self) -> Vec<FlightLine> {
        lines(self.words.get())
    }
}

/// One recorded line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlightLine {
    /// Position among every line the ring accepted, starting at 1.
    pub sequence: u64,
    pub unix_ms: u64,
    /// The line, lossily decoded where a cut split a character.
    pub text: String,
}

/// A flight ring file read back.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FlightImage {
    /// Lines accepted, including those overwritten.
    pub recorded: u64,
    /// The retained lines, oldest first.
    pub lines: Vec<FlightLine>,
}

impl FlightImage {
    /// The `flight.log` text: a summary line, then one `<UTC time>  <line>` per kept line.
    pub fn to_log(&self) -> String {
        let mut log = format!(
            "flight recorder: {} lines recorded, last {} kept\n",
            self.recorded,
            self.lines.len()
        );
        for line in &self.lines {
            log.push_str(&crate::utc::iso_ms(line.unix_ms));
            log.push_str("  ");
            log.push_str(&line.text);
            log.push('\n');
        }
        log
    }
}

/// Reads a flight ring file's bytes, as its writer left them. A line being written or
/// overwritten while it is read is left out rather than shown torn.
pub fn read(bytes: &[u8]) -> io::Result<FlightImage> {
    let words = super::image::<WORDS>(bytes, MAGIC, LAYOUT)?;
    Ok(FlightImage {
        recorded: recorded(&words),
        lines: lines(&words),
    })
}

fn recorded(words: &[AtomicU64]) -> u64 {
    words[NEXT_WORD].load(Ordering::Acquire)
}

fn slot(words: &[AtomicU64], sequence: u64) -> &[AtomicU64] {
    let index = ((sequence - 1) % CAPACITY as u64) as usize;
    let start = HEADER_WORDS + index * SLOT_WORDS;
    &words[start..start + SLOT_WORDS]
}

fn lines(words: &[AtomicU64]) -> Vec<FlightLine> {
    let mut lines = Vec::new();
    let mut text = Vec::with_capacity(MAX_TEXT_BYTES);
    for sequence in super::retained(recorded(words), CAPACITY) {
        let slot = slot(words, sequence);
        if super::published_state(slot[0].load(Ordering::Acquire), sequence) != Some(LINE) {
            continue;
        }
        let unix_ms = slot[TIME_WORD].load(Ordering::Relaxed);
        let len = (slot[LEN_WORD].load(Ordering::Relaxed) as usize).min(MAX_TEXT_BYTES);
        text.clear();
        for word in &slot[TEXT_WORD..] {
            text.extend_from_slice(&word.load(Ordering::Relaxed).to_le_bytes());
        }
        fence(Ordering::Acquire);
        if super::published_state(slot[0].load(Ordering::Relaxed), sequence) != Some(LINE) {
            continue;
        }
        lines.push(FlightLine {
            sequence,
            unix_ms,
            text: String::from_utf8_lossy(&text[..len]).into_owned(),
        });
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mapped_ring_round_trips_through_its_file() {
        static RING: FlightRing = FlightRing::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flight.ring");
        RING.record(10, b"before the attach");
        RING.attach(&path).unwrap();
        let long = "x".repeat(MAX_TEXT_BYTES + 5);
        RING.record(20, long.as_bytes());
        RING.record(30, "DEBUG net: caf\u{e9}".as_bytes());
        let image = read(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(image.recorded, 3);
        let lines: Vec<(u64, u64, &str)> = image
            .lines
            .iter()
            .map(|line| (line.sequence, line.unix_ms, line.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (1, 10, "before the attach"),
                (2, 20, &long[..MAX_TEXT_BYTES]),
                (3, 30, "DEBUG net: caf\u{e9}"),
            ]
        );
        assert_eq!(RING.lines(), image.lines);
    }

    #[test]
    fn the_ring_keeps_the_newest_lines() {
        static RING: FlightRing = FlightRing::new();
        for index in 0..CAPACITY + 2 {
            RING.record(index as u64, index.to_string().as_bytes());
        }
        let lines = RING.lines();
        assert_eq!(RING.recorded(), CAPACITY as u64 + 2);
        assert_eq!(lines.len(), CAPACITY);
        assert_eq!(lines[0].text, "2");
        assert_eq!(lines[CAPACITY - 1].text, (CAPACITY + 1).to_string());
    }

    #[test]
    fn a_packet_ring_file_is_not_a_flight_ring() {
        let bytes = vec![0; WORDS * 8];
        assert_eq!(read(&bytes).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
