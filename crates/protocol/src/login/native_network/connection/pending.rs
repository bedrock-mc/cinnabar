//! Original received FIFO with lazy validated batches and bounded replayed prefixes.

use bedrock_net::BatchPackets;
use bytes::Bytes;
use std::{collections::VecDeque, iter::Peekable};

#[derive(Default)]
pub(super) struct PendingPackets {
    entries: VecDeque<Entry>,
}

enum Entry {
    Packet(Bytes),
    Batch(Peekable<BatchPackets>),
}

impl PendingPackets {
    /// Retains one actual validated wire batch without collecting its packet metadata.
    pub(super) fn push_batch(&mut self, packets: BatchPackets) {
        if !packets.is_empty() {
            self.entries.push_back(Entry::Batch(packets.peekable()));
        }
    }

    /// Restores an original consumed prefix ahead of every unread batch sibling.
    pub(super) fn push_front(&mut self, packet: Bytes) {
        self.entries.push_front(Entry::Packet(packet));
    }

    /// Borrows the next original packet, preserving it until its consumer commits.
    pub(super) fn front(&mut self) -> Option<&Bytes> {
        self.discard_empty();
        match self.entries.front_mut()? {
            Entry::Packet(packet) => Some(packet),
            Entry::Batch(packets) => packets.peek(),
        }
    }

    /// Commits exactly one original packet without advancing another FIFO entry.
    pub(super) fn pop_front(&mut self) -> Option<Bytes> {
        self.discard_empty();
        match self.entries.front_mut()? {
            Entry::Batch(packets) => return packets.next(),
            Entry::Packet(_) => {}
        }
        let Entry::Packet(packet) = self.entries.pop_front()? else {
            unreachable!("the original front was a replayed packet")
        };
        Some(packet)
    }

    /// Releases exhausted batch cursors while preserving the next original payload.
    fn discard_empty(&mut self) {
        loop {
            let empty = match self.entries.front_mut() {
                Some(Entry::Batch(packets)) => packets.peek().is_none(),
                _ => false,
            };
            if !empty {
                return;
            }
            drop(self.entries.pop_front());
        }
    }
}
