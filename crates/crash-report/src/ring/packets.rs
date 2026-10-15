//! The packet header ring: the last [`CAPACITY`] packet headers a session saw, each with its
//! direction, packet ID, body length, time and decode outcome as wire bytes the packet tap
//! owner defines. Recording neither allocates nor formats.
//!
//! File layout after the shared ring header (see [`super`]): word 2 counts records accepted;
//! each slot is three words: stamp (state byte = outcome), `direction << 48 | id << 32 | len`,
//! and elapsed milliseconds.

use std::{
    io,
    path::Path,
    sync::atomic::{AtomicU64, Ordering, fence},
};

use super::{HEADER_WORDS, Words};

/// Records the ring keeps; older ones are overwritten.
pub const CAPACITY: usize = 4096;
/// The ring file's format version.
pub const VERSION: u32 = 1;
/// The first word of a packet ring file.
pub const MAGIC: u64 = u64::from_le_bytes(*b"CNBRPKT\0");
/// Words in a packet ring file.
pub const WORDS: usize = HEADER_WORDS + CAPACITY * SLOT_WORDS;
const SLOT_WORDS: usize = 3;
const NEXT_WORD: usize = 2;
const LAYOUT: u64 = super::layout(VERSION, CAPACITY);

/// One packet header as the ring stores it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketHeader {
    /// Position in the stream of every record this ring has accepted, starting at 1.
    pub sequence: u64,
    /// The direction's wire byte.
    pub direction: u8,
    /// The packet ID's low 16 bits; Bedrock packet IDs use 10.
    pub packet_id: u32,
    /// Body length in bytes.
    pub len: u32,
    /// Monotonic milliseconds since the recorder's epoch.
    pub elapsed_ms: u64,
    /// The outcome's wire byte; never [`super::WRITING`].
    pub outcome: u8,
}

/// A packet header ring, in memory until [`PacketRing::attach`].
pub struct PacketRing {
    words: Words<WORDS>,
}

impl Default for PacketRing {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketRing {
    pub const fn new() -> Self {
        Self {
            words: Words::new(),
        }
    }

    /// Moves the ring, with the records so far, into a new mapped ring file at `path`; see
    /// [`super`]. A second attach fails and leaves the first file alone.
    pub fn attach(&self, path: &Path) -> io::Result<()> {
        self.words.attach(path, MAGIC, LAYOUT)
    }

    /// Appends a record and returns its sequence number. In the race the [`super`] module
    /// describes the record is given up, and readers skip its sequence number.
    #[inline]
    pub fn record(
        &self,
        direction: u8,
        packet_id: u32,
        len: u32,
        elapsed_ms: u64,
        outcome: u8,
    ) -> u64 {
        let sequence = self.reserve();
        if self.begin(sequence) {
            self.publish(sequence, direction, packet_id, len, elapsed_ms, outcome);
        }
        sequence
    }

    /// Takes the next sequence number.
    #[inline]
    pub fn reserve(&self) -> u64 {
        self.words.get()[NEXT_WORD].fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Claims record `sequence`'s slot; see [`super::claim`].
    #[inline]
    pub fn begin(&self, sequence: u64) -> bool {
        super::claim(&slot(self.words.get(), sequence)[0], sequence)
    }

    /// Fills and publishes record `sequence`, which [`Self::begin`] claimed.
    #[inline]
    pub fn publish(
        &self,
        sequence: u64,
        direction: u8,
        packet_id: u32,
        len: u32,
        elapsed_ms: u64,
        outcome: u8,
    ) {
        let slot = slot(self.words.get(), sequence);
        slot[1].store(
            u64::from(direction) << 48 | u64::from(packet_id & 0xffff) << 32 | u64::from(len),
            Ordering::Relaxed,
        );
        slot[2].store(elapsed_ms, Ordering::Relaxed);
        slot[0].store(super::published(sequence, outcome), Ordering::Release);
    }

    /// Sets the outcome of record `sequence` if it is published and not yet overwritten.
    #[inline]
    pub fn settle(&self, sequence: u64, outcome: u8) {
        if sequence == 0 {
            return;
        }
        let stamp = &slot(self.words.get(), sequence)[0];
        let current = stamp.load(Ordering::Relaxed);
        if super::published_state(current, sequence).is_some() {
            let _ = stamp.compare_exchange(
                current,
                super::published(sequence, outcome),
                Ordering::Release,
                Ordering::Relaxed,
            );
        }
    }

    /// Records accepted so far, including those overwritten.
    pub fn recorded(&self) -> u64 {
        recorded(self.words.get())
    }

    /// Visits the retained records oldest first; see [`for_each`].
    pub fn for_each(&self, visit: impl FnMut(PacketHeader)) {
        for_each(self.words.get(), visit);
    }
}

/// A packet ring file read back.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PacketImage {
    /// Records accepted, including those overwritten.
    pub recorded: u64,
    /// The retained records, oldest first.
    pub headers: Vec<PacketHeader>,
}

/// Reads a packet ring file's bytes, as its writer left them.
pub fn read(bytes: &[u8]) -> io::Result<PacketImage> {
    let words = super::image::<WORDS>(bytes, MAGIC, LAYOUT)?;
    let mut headers = Vec::new();
    for_each(&words, |header| headers.push(header));
    Ok(PacketImage {
        recorded: recorded(&words),
        headers,
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

/// Visits the retained records oldest first. A record being written, given up or overwritten
/// while it is read is left out rather than shown torn.
fn for_each(words: &[AtomicU64], mut visit: impl FnMut(PacketHeader)) {
    for sequence in super::retained(recorded(words), CAPACITY) {
        let slot = slot(words, sequence);
        let before = slot[0].load(Ordering::Acquire);
        if super::published_state(before, sequence).is_none() {
            continue;
        }
        let header = slot[1].load(Ordering::Relaxed);
        let elapsed_ms = slot[2].load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        // An outcome settled between the loads changes only the state byte; a slot claimed
        // again carries a newer sequence.
        let Some(outcome) = super::published_state(slot[0].load(Ordering::Relaxed), sequence)
        else {
            continue;
        };
        visit(PacketHeader {
            sequence,
            direction: (header >> 48) as u8,
            packet_id: (header >> 32) as u32 & 0xffff,
            len: header as u32,
            elapsed_ms,
            outcome,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_of(words: &[AtomicU64]) -> Vec<u8> {
        words
            .iter()
            .flat_map(|word| word.load(Ordering::Relaxed).to_le_bytes())
            .collect()
    }

    #[test]
    fn a_mapped_ring_round_trips_through_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("packets.ring");
        let ring = Box::new(PacketRing::new());
        ring.record(0, 0x8a, 12, 5, 0);
        ring.attach(&path).unwrap();
        let second = ring.record(1, 0x1234_0001, u32::MAX, 9, 0);
        ring.settle(second, 2);
        let image = read(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(image.recorded, 2);
        assert_eq!(
            image.headers,
            [
                PacketHeader {
                    sequence: 1,
                    direction: 0,
                    packet_id: 0x8a,
                    len: 12,
                    elapsed_ms: 5,
                    outcome: 0,
                },
                PacketHeader {
                    sequence: 2,
                    direction: 1,
                    packet_id: 1,
                    len: u32::MAX,
                    elapsed_ms: 9,
                    outcome: 2,
                },
            ]
        );
    }

    #[test]
    fn the_file_keeps_the_newest_capacity_records() {
        let ring = Box::new(PacketRing::new());
        for index in 0..CAPACITY as u64 + 3 {
            ring.record(0, 1, index as u32, index, 0);
        }
        let mut file = file_of(ring.words.get());
        file[..8].copy_from_slice(&MAGIC.to_le_bytes());
        file[8..16].copy_from_slice(&LAYOUT.to_le_bytes());
        let image = read(&file).unwrap();
        assert_eq!(image.recorded, CAPACITY as u64 + 3);
        assert_eq!(image.headers.len(), CAPACITY);
        assert_eq!(image.headers[0].sequence, 4);
        assert_eq!(image.headers[CAPACITY - 1].len, CAPACITY as u32 + 2);
    }

    #[test]
    fn a_claimed_slot_is_left_out_and_a_stale_writer_gives_up() {
        let ring = Box::new(PacketRing::new());
        let first = ring.reserve();
        assert!(ring.begin(first));
        ring.record(0, 2, 1, 1, 0);
        let mut seen = Vec::new();
        ring.for_each(|header| seen.push(header.sequence));
        assert_eq!(seen, [2]);
        // Once the slot moved on, the old writer may not claim it again.
        for _ in 0..CAPACITY {
            ring.record(0, 3, 1, 1, 0);
        }
        assert!(!ring.begin(first));
    }

    #[test]
    fn foreign_and_resized_files_are_rejected() {
        let ring = Box::new(PacketRing::new());
        let file = file_of(ring.words.get());
        assert_eq!(read(&file).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            read(&file[..file.len() - 8]).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn a_ring_attaches_once_and_keeps_its_first_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        let ring = Box::new(PacketRing::new());
        ring.attach(&path).unwrap();
        ring.record(0, 1, 1, 1, 0);
        assert_eq!(
            ring.attach(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        let image = read(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(image.recorded, 1);
        ring.settle(0, 2);
    }
}
