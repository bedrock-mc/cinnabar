//! Compaction: rewriting a region file with only its live records and one full index.

use std::fs::{self, File, OpenOptions};

use lod::geometry::SectionKey;

use super::{
    IndexChain, PENDING_WRITE_BYTES, RegionFile,
    header::{commit_slot_bytes, commit_slot_offset, header_bytes},
    temp_sibling,
};
use crate::{
    error::RegionError,
    frame::{FRAME_HEADER_LEN, FrameKind, append_frame},
    positional::write_all_at,
    region_index::{RecordEntry, RegionIndex},
};

/// What one compaction did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compaction {
    /// File length before.
    pub before: u64,
    /// File length after.
    pub after: u64,
    /// Keys whose records were corrupt and were dropped, with the content hashes of the
    /// chunks they covered; callers rebuild them.
    pub dropped: Vec<SectionKey>,
}

/// The rewritten file, ready to replace the original.
struct Rewritten {
    file: File,
    index: RegionIndex,
    len: u64,
    live_record_bytes: u64,
    index_offset: u64,
    index_frame_len: u64,
    dropped: Vec<SectionKey>,
}

impl RegionFile {
    /// Commits, rewrites the live records and the chunk hashes no dropped record covered
    /// to a temporary sibling, flushes it to disk and renames it over this file. On
    /// failure the file is left as it was and the sibling removed.
    pub fn compact(&mut self) -> Result<Compaction, RegionError> {
        self.require_writable()?;
        self.commit()?;
        let before = self.len();
        let temp = temp_sibling(&self.path);
        let rewritten = match self.rewrite(&temp) {
            Ok(rewritten) => rewritten,
            Err(error) => {
                let _ = fs::remove_file(&temp);
                return Err(error);
            }
        };
        if let Err(error) = fs::rename(&temp, &self.path) {
            let _ = fs::remove_file(&temp);
            return Err(error.into());
        }
        // The handle opened on the sibling now names the renamed file.
        self.file = rewritten.file;
        self.index = rewritten.index;
        self.written_end = rewritten.len;
        self.live_record_bytes = rewritten.live_record_bytes;
        self.chain = IndexChain {
            last: Some(rewritten.index_offset),
            full_bytes: rewritten.index_frame_len,
            delta_bytes: 0,
            deltas: 0,
        };
        self.seq = 1;
        self.force_full = false;
        Ok(Compaction {
            before,
            after: rewritten.len,
            dropped: rewritten.dropped,
        })
    }

    /// Writes the compacted file at `path`, committed with sequence 1 and synced.
    fn rewrite(&mut self, path: &std::path::Path) -> Result<Rewritten, RegionError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        let mut out = header_bytes(self.id).to_vec();
        let mut written = 0u64;
        let mut index = RegionIndex::default();
        let mut live_record_bytes = 0;
        let mut dropped = Vec::new();
        let mut record = Vec::new();
        let mut unvouched = Vec::new();
        let mut entries: Vec<_> = self.index.records().collect();
        entries.sort_unstable_by_key(|&(_, entry)| entry.offset);
        for (slot, entry) in entries {
            let Some(key) = self.id.key_at(slot) else {
                continue;
            };
            match self.read_record(key, entry, &mut record) {
                Ok(()) => {}
                Err(RegionError::Corrupt { .. }) => {
                    dropped.push(key);
                    unvouched.extend(slot.chunk_slots());
                    continue;
                }
                Err(error) => return Err(error),
            }
            let offset = written + out.len() as u64;
            append_frame(&mut out, FrameKind::Record, key.bits(), &record);
            let entry = RecordEntry {
                offset,
                len: record.len() as u32,
            };
            live_record_bytes += super::record_frame_len(entry);
            index.set_record(slot, entry);
            if out.len() >= PENDING_WRITE_BYTES {
                write_all_at(&file, &out, written)?;
                written += out.len() as u64;
                out.clear();
            }
        }
        for (slot, hash) in self.index.hashes() {
            if !unvouched.contains(&slot) {
                index.set_hash(slot, Some(hash));
            }
        }

        let seq = 1;
        let index_offset = written + out.len() as u64;
        index.encode(None, &mut self.scratch);
        append_frame(&mut out, FrameKind::Index, seq, &self.scratch);
        write_all_at(&file, &out, written)?;
        let len = written + out.len() as u64;
        write_all_at(
            &file,
            &commit_slot_bytes(seq, index_offset),
            commit_slot_offset(seq),
        )?;
        file.sync_all()?;
        index.mark_clean();
        Ok(Rewritten {
            file,
            index,
            len,
            live_record_bytes,
            index_offset,
            index_frame_len: (FRAME_HEADER_LEN + self.scratch.len()) as u64,
            dropped,
        })
    }
}
