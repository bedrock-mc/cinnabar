//! A fixed-capacity, lock-free ring of packet header records.
//!
//! Writers take sequence numbers with one atomic increment and publish each slot under a
//! per-slot stamp, so any number of threads may record while a reader takes a consistent
//! snapshot. A slot only moves forward: a writer whose record a newer one has already
//! replaced, or that finds an older writer still filling the slot, gives its record up rather
//! than mixing two records. Recording neither allocates nor formats.

use std::sync::atomic::{AtomicU64, Ordering, fence};

use protocol::packet_tap::{PacketDirection, PacketOutcome};

/// Records the ring keeps; older ones are overwritten.
pub const RING_CAPACITY: usize = 4096;

/// The outcome byte of a stamp whose slot is being written; no [`PacketOutcome`] has it.
const WRITING: u64 = 0xff;

/// One packet header as the ring stores it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeaderRecord {
    /// Position in the stream of every record this ring has accepted, starting at 1.
    pub sequence: u64,
    pub direction: PacketDirection,
    pub packet_id: u32,
    /// Body length in bytes.
    pub len: u32,
    /// Monotonic milliseconds since the packet log's epoch, as the recorder supplied them.
    pub elapsed_ms: u64,
    pub outcome: PacketOutcome,
}

/// A slot's stamp is `sequence << 8 | outcome` once published, `sequence << 8 | WRITING`
/// while that record is being written, and 0 while empty.
struct Slot {
    stamp: AtomicU64,
    header: AtomicU64,
    elapsed_ms: AtomicU64,
}

impl Slot {
    const fn new() -> Self {
        Self {
            stamp: AtomicU64::new(0),
            header: AtomicU64::new(0),
            elapsed_ms: AtomicU64::new(0),
        }
    }
}

pub(crate) struct HeaderRing {
    next: AtomicU64,
    slots: [Slot; RING_CAPACITY],
}

impl HeaderRing {
    pub(crate) const fn new() -> Self {
        Self {
            next: AtomicU64::new(0),
            slots: [const { Slot::new() }; RING_CAPACITY],
        }
    }

    /// Appends a record and returns its sequence number. In the rare race described in the
    /// module documentation the record is given up, and readers skip its sequence number.
    pub(crate) fn record(
        &self,
        direction: PacketDirection,
        packet_id: u32,
        len: u32,
        elapsed_ms: u64,
        outcome: PacketOutcome,
    ) -> u64 {
        let sequence = self.reserve();
        if self.begin(sequence) {
            self.publish(sequence, direction, packet_id, len, elapsed_ms, outcome);
        }
        sequence
    }

    /// Takes the next sequence number.
    pub(crate) fn reserve(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Claims record `sequence`'s slot for writing, unless a newer record owns it or an older
    /// writer is still filling it. Only a claimed record may be published.
    pub(crate) fn begin(&self, sequence: u64) -> bool {
        let slot = self.slot(sequence);
        let mut current = slot.stamp.load(Ordering::Relaxed);
        loop {
            if current >> 8 >= sequence || current & 0xff == WRITING {
                return false;
            }
            match slot.stamp.compare_exchange_weak(
                current,
                sequence << 8 | WRITING,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
        // Readers that see any field written below also see the slot as being written.
        fence(Ordering::Release);
        true
    }

    /// Fills and publishes record `sequence`, which [`Self::begin`] claimed. No other writer
    /// touches a slot while it is claimed.
    pub(crate) fn publish(
        &self,
        sequence: u64,
        direction: PacketDirection,
        packet_id: u32,
        len: u32,
        elapsed_ms: u64,
        outcome: PacketOutcome,
    ) {
        let slot = self.slot(sequence);
        slot.header
            .store(pack_header(direction, packet_id, len), Ordering::Relaxed);
        slot.elapsed_ms.store(elapsed_ms, Ordering::Relaxed);
        slot.stamp
            .store(stamp(sequence, outcome), Ordering::Release);
    }

    /// Sets the outcome of record `sequence` if it is published and not yet overwritten.
    pub(crate) fn settle(&self, sequence: u64, outcome: PacketOutcome) {
        let slot = self.slot(sequence);
        let current = slot.stamp.load(Ordering::Relaxed);
        if current >> 8 == sequence && current & 0xff != WRITING {
            let _ = slot.stamp.compare_exchange(
                current,
                stamp(sequence, outcome),
                Ordering::Release,
                Ordering::Relaxed,
            );
        }
    }

    /// Records accepted so far, including those overwritten.
    pub(crate) fn recorded(&self) -> u64 {
        self.next.load(Ordering::Acquire)
    }

    /// Visits the retained records oldest first. A record being written, given up or
    /// overwritten while it is read is left out rather than shown torn.
    pub(crate) fn for_each(&self, mut visit: impl FnMut(HeaderRecord)) {
        let end = self.recorded();
        let start = end.saturating_sub(RING_CAPACITY as u64);
        for sequence in start + 1..=end {
            if let Some(record) = self.read(sequence) {
                visit(record);
            }
        }
    }

    fn read(&self, sequence: u64) -> Option<HeaderRecord> {
        let slot = self.slot(sequence);
        let before = slot.stamp.load(Ordering::Acquire);
        if before >> 8 != sequence || before & 0xff == WRITING {
            return None;
        }
        let header = slot.header.load(Ordering::Relaxed);
        let elapsed_ms = slot.elapsed_ms.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        let after = slot.stamp.load(Ordering::Relaxed);
        // An outcome settled between the two loads changes only the low byte; a slot claimed
        // again carries a newer sequence.
        if after >> 8 != sequence {
            return None;
        }
        let (direction, packet_id, len) = unpack_header(header)?;
        Some(HeaderRecord {
            sequence,
            direction,
            packet_id,
            len,
            elapsed_ms,
            outcome: PacketOutcome::from_wire(after as u8)?,
        })
    }

    fn slot(&self, sequence: u64) -> &Slot {
        &self.slots[((sequence - 1) % RING_CAPACITY as u64) as usize]
    }
}

fn stamp(sequence: u64, outcome: PacketOutcome) -> u64 {
    sequence << 8 | u64::from(outcome as u8)
}

/// Packs direction, a 16-bit packet ID and the length; Bedrock packet IDs use 10 bits.
fn pack_header(direction: PacketDirection, packet_id: u32, len: u32) -> u64 {
    u64::from(direction as u8) << 48 | u64::from(packet_id & 0xffff) << 32 | u64::from(len)
}

fn unpack_header(header: u64) -> Option<(PacketDirection, u32, u32)> {
    Some((
        PacketDirection::from_wire((header >> 48) as u8)?,
        (header >> 32) as u32 & 0xffff,
        header as u32,
    ))
}
