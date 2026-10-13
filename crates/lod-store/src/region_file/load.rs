//! Opening a region file: its header, commit slots and committed index chain.

use std::{fs::File, io, path::Path};

use super::{
    Access, COMMIT_SLOT_LEN, COMMIT_SLOT_OFFSETS, DATA_START, IndexChain, MAX_INDEX_CHAIN,
    RegionFile,
    header::{check_header, read_commit_slot},
    record_frame_len,
};
use crate::{
    error::{HeaderError, IndexFault, RegionError},
    frame::{FRAME_HEADER_LEN, FrameError, FrameHeader, FrameKind, MAX_RECORD_LEN},
    positional::read_exact_at,
    region_id::RegionId,
    region_index::{IndexFrame, RegionIndex},
};

impl RegionFile {
    /// Validates the header, then loads the newest committed index or rebuilds it.
    pub(super) fn load(
        file: File,
        path: &Path,
        id: RegionId,
        access: Access,
        len: u64,
    ) -> Result<Self, RegionError> {
        if len < DATA_START {
            return Err(HeaderError::Truncated(len).into());
        }
        let mut fixed = [0; DATA_START as usize];
        read_exact_at(&file, &mut fixed, 0)?;
        check_header(&fixed, id)?;
        let newest = COMMIT_SLOT_OFFSETS
            .iter()
            .filter_map(|&at| read_commit_slot(&fixed[at..at + COMMIT_SLOT_LEN]))
            .max_by_key(|&(seq, _)| seq);
        let mut region = Self::empty(file, path, id, access, len);
        region.seq = newest.map_or(0, |(seq, _)| seq);
        let fault = match newest {
            Some((seq, offset)) => region.load_chain(seq, offset)?.err(),
            None if len > DATA_START => Some(IndexFault::NoCommit),
            None => None,
        };
        if let Some(fault) = fault {
            region.rebuild(fault)?;
        }
        Ok(region)
    }

    /// Loads the index chain ending at the frame `offset` committed with `seq`.
    fn load_chain(&mut self, seq: u64, offset: u64) -> io::Result<Result<(), IndexFault>> {
        let mut frames: Vec<(u64, IndexFrame)> = Vec::new();
        let mut at = offset;
        let mut tag_bound = seq;
        let hashes_allowed = self.id.level() == 0;
        let mut payload = Vec::new();
        loop {
            if frames.len() > MAX_INDEX_CHAIN as usize {
                return Ok(Err(IndexFault::Chain { offset: at }));
            }
            let header = match read_frame_at(&self.file, at, self.written_end, &mut payload)? {
                Ok(header) => header,
                Err(error) => return Ok(Err(IndexFault::Frame { offset: at, error })),
            };
            let tag_ok = if frames.is_empty() {
                header.tag == tag_bound
            } else {
                header.tag < tag_bound
            };
            if header.kind != FrameKind::Index || !tag_ok {
                let error = FrameError::Unexpected {
                    expected: FrameKind::Index,
                    expected_tag: tag_bound,
                    kind: header.kind,
                    tag: header.tag,
                };
                return Ok(Err(IndexFault::Frame { offset: at, error }));
            }
            tag_bound = header.tag;
            let frame = match IndexFrame::decode(&payload, hashes_allowed) {
                Ok(frame) => frame,
                Err(error) => return Ok(Err(IndexFault::Index { offset: at, error })),
            };
            let previous = frame.previous;
            frames.push((header.frame_len(), frame));
            match previous {
                None => break,
                Some(previous) if (DATA_START..at).contains(&previous) => at = previous,
                Some(_) => return Ok(Err(IndexFault::Chain { offset: at })),
            }
        }

        let mut index = RegionIndex::default();
        let mut chain = IndexChain {
            last: Some(offset),
            ..IndexChain::default()
        };
        for (position, (frame_len, frame)) in frames.iter().rev().enumerate() {
            index.apply(frame);
            if position == 0 {
                chain.full_bytes = *frame_len;
            } else {
                chain.delta_bytes += frame_len;
                chain.deltas += 1;
            }
        }
        let mut live = 0;
        for (slot, entry) in index.records() {
            let end = entry.offset.checked_add(record_frame_len(entry));
            let in_bounds = entry.offset >= DATA_START
                && entry.len as usize <= MAX_RECORD_LEN
                && end.is_some_and(|end| end <= self.written_end);
            if !in_bounds {
                return Ok(Err(IndexFault::EntryOutOfBounds { slot: slot.raw() }));
            }
            live += record_frame_len(entry);
        }
        index.mark_clean();
        self.index = index;
        self.chain = chain;
        self.live_record_bytes = live;
        Ok(Ok(()))
    }
}

/// Reads and verifies the frame at `offset` of a file whose data ends at `end`, leaving
/// its payload in `payload`. Frames running past `end` are truncated, never read.
pub(super) fn read_frame_at(
    file: &File,
    offset: u64,
    end: u64,
    payload: &mut Vec<u8>,
) -> io::Result<Result<FrameHeader, FrameError>> {
    let available = end.saturating_sub(offset);
    let truncated = |needed: u64| FrameError::Truncated {
        needed: needed as usize,
        available: available as usize,
    };
    if available < FRAME_HEADER_LEN as u64 {
        return Ok(Err(truncated(FRAME_HEADER_LEN as u64)));
    }
    // A file that shrinks underneath reads as truncated, like one that ended early.
    let read = |buffer: &mut [u8], at: u64| match read_exact_at(file, buffer, at) {
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        other => other.map(|()| true),
    };
    let mut header_bytes = [0; FRAME_HEADER_LEN];
    if !read(&mut header_bytes, offset)? {
        return Ok(Err(truncated(FRAME_HEADER_LEN as u64)));
    }
    let header = match FrameHeader::read(&header_bytes) {
        Ok(header) => header,
        Err(error) => return Ok(Err(error)),
    };
    if header.frame_len() > available {
        return Ok(Err(truncated(header.frame_len())));
    }
    payload.clear();
    payload.resize(header.payload_len as usize, 0);
    if !read(payload, offset + FRAME_HEADER_LEN as u64)? {
        return Ok(Err(truncated(header.frame_len())));
    }
    Ok(header.verify(&header_bytes, payload).map(|()| header))
}
