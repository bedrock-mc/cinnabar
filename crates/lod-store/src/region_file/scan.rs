//! Rebuilding a region file's index by scanning every frame in file order.

use std::{fs::File, io};

use lod::{codec::RecordHeader, geometry::SectionKey};

use super::{Access, DATA_START, IndexChain, RegionFile, read_frame_at, record_frame_len};
use crate::{
    error::{IndexFault, Recovery},
    frame::{FRAME_HEADER_LEN, FRAME_MAGIC, FrameHeader, FrameKind},
    region_id::{RegionId, SectionSlot},
    region_index::{IndexFrame, RecordEntry, RegionIndex},
};

/// Bytes read per step while searching for the next frame after a corrupt one.
const RESYNC_BLOCK: usize = 64 * 1024;

impl RegionFile {
    /// Replaces the index with one rebuilt from the file's frames. Later frames win, so
    /// the newest record of each key survives and tombstones remove earlier records.
    /// Chunk hashes come from every readable index frame in order. After a corrupt frame
    /// the scan resumes at the next frame magic. Hashes may vouch for records the scan
    /// lost, so a lost level-0 record clears the hashes of its chunks. A lost index may
    /// have cleared hashes, as one does after a record is dropped as corrupt, and bytes
    /// whose frame cannot be told may have held anything, so either clears every hash.
    pub(super) fn rebuild(&mut self, fault: IndexFault) -> io::Result<()> {
        let mut index = RegionIndex::default();
        let mut skipped = 0;
        let mut lost = Lost::default();
        let mut at = DATA_START;
        let mut payload = Vec::new();
        while at < self.written_end {
            match read_frame_at(&self.file, at, self.written_end, &mut payload)? {
                Ok(header) => {
                    if !self.replay(header, at, &payload, &mut index) {
                        skipped += header.frame_len();
                    }
                    at += header.frame_len();
                }
                Err(_) => {
                    lost.note(self.lost_frame(at)?);
                    let next = find_magic(&self.file, at + 1, self.written_end)?
                        .unwrap_or(self.written_end);
                    skipped += next - at;
                    at = next;
                }
            }
        }
        // Only level-0 files hold hashes; elsewhere both branches find none.
        if lost.all_hashes {
            let hashed: Vec<_> = index.hashes().map(|(slot, _)| slot).collect();
            for slot in hashed {
                index.set_hash(slot, None);
            }
        } else {
            for slot in lost.records {
                for chunk in slot.chunk_slots() {
                    index.set_hash(chunk, None);
                }
            }
        }
        index.mark_clean();
        self.live_record_bytes = index
            .records()
            .map(|(_, entry)| record_frame_len(entry))
            .sum();
        self.recovery = Some(Recovery {
            fault,
            records: index.record_count(),
            skipped_bytes: skipped,
        });
        self.index = index;
        self.chain = IndexChain::default();
        self.force_full = self.access == Access::ReadWrite;
        Ok(())
    }

    /// What the unreadable frame at `offset` held, as far as its header tells.
    fn lost_frame(&self, offset: u64) -> io::Result<LostFrame> {
        let mut header = [0; FRAME_HEADER_LEN];
        let available = self.written_end.saturating_sub(offset);
        if available < FRAME_HEADER_LEN as u64 {
            return Ok(LostFrame::Unknown);
        }
        crate::positional::read_exact_at(&self.file, &mut header, offset)?;
        let Ok(header) = FrameHeader::read(&header) else {
            return Ok(LostFrame::Unknown);
        };
        Ok(match header.kind {
            FrameKind::Index => LostFrame::Index,
            FrameKind::Tombstone => LostFrame::Tombstone,
            // The header has no checksum of its own, so a key of another region is noise.
            FrameKind::Record => match SectionKey::from_bits(header.tag).map(RegionId::of_key) {
                Ok((region, slot)) if region == self.id => LostFrame::Record(slot),
                _ => LostFrame::Unknown,
            },
        })
    }

    /// Applies one valid frame to a rebuilding index; false when its contents are invalid
    /// for this region.
    fn replay(
        &mut self,
        header: FrameHeader,
        offset: u64,
        payload: &[u8],
        index: &mut RegionIndex,
    ) -> bool {
        match header.kind {
            FrameKind::Record => {
                let Ok(record) = RecordHeader::read(payload) else {
                    return false;
                };
                let (region, slot) = RegionId::of_key(record.key);
                if record.key.bits() != header.tag || region != self.id {
                    return false;
                }
                index.set_record(
                    slot,
                    RecordEntry {
                        offset,
                        len: header.payload_len,
                    },
                );
                true
            }
            FrameKind::Tombstone => {
                let Ok(key) = SectionKey::from_bits(header.tag) else {
                    return false;
                };
                let (region, slot) = RegionId::of_key(key);
                if region != self.id {
                    return false;
                }
                index.remove_record(slot);
                true
            }
            FrameKind::Index => {
                let Ok(frame) = IndexFrame::decode(payload, self.id.level() == 0) else {
                    return false;
                };
                index.apply_hashes(&frame);
                self.seq = self.seq.max(header.tag);
                true
            }
        }
    }
}

/// What a rebuilding scan could not read.
#[derive(Default)]
struct Lost {
    /// Slots of lost records.
    records: Vec<SectionSlot>,
    /// True when some lost bytes could have held any record or changed any hash.
    all_hashes: bool,
}

impl Lost {
    /// Adds one unreadable frame to what the scan lost.
    fn note(&mut self, frame: LostFrame) {
        match frame {
            LostFrame::Record(slot) => self.records.push(slot),
            LostFrame::Index | LostFrame::Unknown => self.all_hashes = true,
            LostFrame::Tombstone => {}
        }
    }
}

/// One unreadable frame, judged by its header.
enum LostFrame {
    /// A record of this file in this slot.
    Record(SectionSlot),
    /// A tombstone, whose removal the scan misses, so the record stays removed only if
    /// it was lost too.
    Tombstone,
    /// An index, whose hash changes the scan misses.
    Index,
    /// The header is unreadable too.
    Unknown,
}

/// Offset of the next [`FRAME_MAGIC`] in `from..end`.
fn find_magic(file: &File, from: u64, end: u64) -> io::Result<Option<u64>> {
    let overlap = FRAME_MAGIC.len() - 1;
    let mut block = vec![0; RESYNC_BLOCK];
    let mut at = from;
    while at < end {
        let len = RESYNC_BLOCK.min((end - at) as usize);
        crate::positional::read_exact_at(file, &mut block[..len], at)?;
        if let Some(found) = block[..len]
            .windows(FRAME_MAGIC.len())
            .position(|window| window == FRAME_MAGIC)
        {
            return Ok(Some(at + found as u64));
        }
        if len < RESYNC_BLOCK {
            break;
        }
        at += (len - overlap) as u64;
    }
    Ok(None)
}
