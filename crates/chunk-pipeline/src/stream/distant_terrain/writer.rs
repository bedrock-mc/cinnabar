//! The writer threads: they ask the store for content hashes, merge sampled columns into
//! the sections they share and submit the results.
//!
//! Writers and the mesher thread are the only store clients, so world and mesh workers
//! never wait for disk; the mesher's reads share the store's queue. Each writer owns whole
//! [`lod::ingest::WriterRegion`]s ([`super::shared::Writers`]), which hold every section a
//! column writes, and takes only its own regions' columns, so every section key has one
//! writer, concurrent columns never lose each other's octants, and a column's captures
//! reach one writer in order. Columns inside one region therefore merge on one writer. A neighbour slab re-mip across a region border is handed to the
//! neighbour's owner once the merge that caused it is stored
//! ([`lod::ingest::merge_owned`]); the owner merges it next and reads that write. A
//! writer reads sections of the session's own epoch only, so an undecided candidate never
//! takes in a stored epoch's terrain, through a bounded cache in front of the store
//! client; the store answers a read only after every write submitted before it, so a
//! section evicted from the cache reads back as written, and another writer's section is
//! read again once that writer hands over a slab facing it.
//!
//! Each merge is prepared in one store [`SessionEpoch`] and sent with it: the cache is
//! dropped whenever the epoch moved on, and writes the store refuses because a session
//! changed while they were prepared (a candidate merged, or a session began or ended) are
//! merged again from what the store now holds. When the store's queue is full the writer
//! keeps the merged writes, and every column and slab after them, until the frame nudges
//! it, and takes in nothing new before they are sent; until then ingestion is not settled.
//! Dropping them instead would lose final captures, which are never taken again. A column
//! whose writes the stopped store refused, or whose merge a failed read stopped, has its
//! hash forgotten, so it is never skipped as stored. Handed-over slabs of a merge a failed
//! read stopped are held and merged again after the next nudge, up to
//! [`MAX_SLAB_MERGE_ATTEMPTS`] times, since nothing else would ask for them again.

use std::{
    collections::{BTreeSet, VecDeque},
    io,
    sync::{Arc, atomic::Ordering},
};

use lod::{
    codec::SectionCodec,
    geometry::SectionKey,
    ingest::{SlabWork, merge_owned},
};
use lod_store::{
    actor::{EpochWrite, ReadOutcome, ReadTarget, Request, SessionEpoch, SubmitError},
    identity::Dimension,
};

use super::{
    LOD_SECTION_CACHE_BYTES, MERGE_BATCH_COLUMNS,
    blocks::MaterialClassTable,
    book::{StoredHash, Verdict},
    jobs,
    sections::{LoadError, SectionCache, StoreSections},
    shared::{Capture, Counters, LodShared, SampledColumn, WriterBatch, WriterInput, Writers},
};

/// Merges a failed read stops before a writer drops the handed-over slabs they held.
pub(in crate::stream) const MAX_SLAB_MERGE_ATTEMPTS: u32 = 8;

/// One writer thread's state between batches.
#[derive(Default)]
pub(in crate::stream) struct Writer {
    /// This writer's index among [`super::shared::Writers`].
    index: usize,
    cache: SectionCache,
    /// The ingestion generation the cached sections were read in.
    cached_generation: u64,
    classes: MaterialClassTable,
    codec: SectionCodec,
    /// Merged writes the store's full queue refused; sent before anything else.
    unsent: Option<Prepared>,
    /// Accepted columns not merged yet, oldest first: a read found the store's queue full,
    /// or the store refused their merge as stale.
    held: VecDeque<SampledColumn>,
    /// Slabs handed over and not merged yet.
    held_slabs: BTreeSet<(Dimension, SlabWork)>,
    /// Merges with handed-over slabs a failed read stopped in a row.
    failed_slab_merges: u32,
}

/// Work a merge handed back because a read failed: why, the columns and the slabs.
pub(in crate::stream) type Unmerged = (LoadError, Vec<SampledColumn>, Vec<SlabWork>);

/// One dimension's merged columns and slabs and the writes that store them, prepared in
/// `epoch`.
pub(in crate::stream) struct Prepared {
    dimension: Dimension,
    epoch: SessionEpoch,
    /// Kept to merge again should the store refuse the writes as stale; final captures
    /// among them stay counted until written.
    columns: Vec<SampledColumn>,
    /// Handed-over slabs the merge took, kept like the columns.
    slabs: Vec<SlabWork>,
    /// Slabs of other writers' sections to hand over once the writes are taken.
    foreign: Vec<SlabWork>,
    /// The sections the writes store, for the mesher.
    written: Vec<SectionKey>,
    requests: Vec<Request>,
    sections: u64,
}

/// Starts `shared`'s writer threads; they stop once ingestion is turned off. A thread
/// that cannot start turns ingestion off again.
pub(in crate::stream) fn spawn(shared: &Arc<LodShared>) -> io::Result<()> {
    for index in 0..shared.writers.count() {
        let thread_shared = Arc::clone(shared);
        let name = format!("lod-writer-{index}");
        let started = std::thread::Builder::new()
            .name(name.clone())
            .spawn(move || {
                crate::stream::workers::lower_beside_pool(&name);
                run(&thread_shared, index);
            });
        if let Err(error) = started {
            shared.disable();
            return Err(error);
        }
    }
    Ok(())
}

/// Writer thread `index`'s loop. While the writer holds work back for the store's full
/// queue it waits only for the frame's nudge, and says so through its
/// [`super::shared::WriterFlags::retry_due`] before it stops being busy, so ingestion
/// never reads as settled meanwhile.
fn run(shared: &Arc<LodShared>, index: usize) {
    let mut writer = Writer::new(index, shared.writers);
    loop {
        let blocked = writer.is_blocked();
        shared.flags[index]
            .retry_due
            .store(blocked, Ordering::Release);
        let Some(batch) = shared.wait_for_work(index, MERGE_BATCH_COLUMNS, blocked) else {
            return;
        };
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.lod_writer").entered();
        writer.step(shared, batch);
    }
}

/// Asks the store for the content hash of `capture`'s column, then skips the capture
/// when the store holds it and hands it back to the Lod lane for sampling otherwise.
fn check(shared: &Arc<LodShared>, capture: Capture) {
    let chunk = capture.input.chunk;
    let key = (Dimension(chunk.dimension), chunk);
    let hash = capture.checked_hash.expect("checks carry their hash");
    let asked = shared.generation();
    let read = shared.client.read(
        key.0,
        ReadTarget::ChunkHash {
            x: chunk.x,
            z: chunk.z,
        },
        shared.now(),
    );
    match read {
        Ok(ReadOutcome::ChunkHash(stored)) => {
            let stored = stored.map_or(StoredHash::Missing, StoredHash::Hash);
            let mut book = shared.lock_book();
            book.learn(key, stored, asked);
            match book.check(key, capture.seq, hash, shared.generation()) {
                Verdict::Superseded => return Counters::bump(&shared.counters.superseded),
                Verdict::Unchanged => return Counters::bump(&shared.counters.skipped_unchanged),
                Verdict::Changed | Verdict::Unknown => {}
            }
        }
        // A full queue only costs the skip; the column is sampled anyway.
        Err(SubmitError::Full(())) => {}
        Ok(_) | Err(SubmitError::Stopped(())) => {
            return Counters::bump(&shared.counters.store_errors);
        }
    }
    jobs::spawn(Arc::clone(shared), capture);
}

impl Writer {
    /// The state of writer thread `index` of `writers`, with its share of the section
    /// cache budget.
    pub(in crate::stream) fn new(index: usize, writers: Writers) -> Self {
        Self {
            index,
            cache: SectionCache::new(LOD_SECTION_CACHE_BYTES / writers.count()),
            ..Self::default()
        }
    }

    /// True while merged writes, accepted columns or handed-over slabs wait for room in
    /// the store's queue.
    pub(in crate::stream) fn is_blocked(&self) -> bool {
        self.unsent.is_some() || !self.held.is_empty() || !self.held_slabs.is_empty()
    }

    /// Takes in `batch` (empty while blocked): checks go to the store, accepted samples
    /// join the held columns and slabs the held slabs, whose facing sections are read
    /// again. Then sends and merges whatever is held, in order.
    pub(in crate::stream) fn step(&mut self, shared: &Arc<LodShared>, batch: WriterBatch) {
        for (dimension, slab) in batch.slabs {
            if let Some(changed) = slab.pos.neighbour(slab.face) {
                self.cache.forget(dimension, changed.key());
            }
            self.held_slabs.insert((dimension, slab));
        }
        for input in batch.items {
            match input {
                WriterInput::Check(capture) => check(shared, *capture),
                WriterInput::Sampled(column) => {
                    let key = (column.dimension, column.chunk);
                    let accepted = shared.lock_book().accept(
                        key,
                        column.seq,
                        column.hash,
                        shared.generation(),
                    );
                    if accepted {
                        self.held.push_back(*column);
                    } else {
                        Counters::bump(&shared.counters.superseded);
                    }
                }
            }
        }
        if self.flush(shared) && shared.nothing_outstanding() {
            shared.lock_book().trim_when_idle();
        }
    }

    /// Sends the unsent writes, then merges and sends the held columns and slabs a
    /// dimension at a time; false, keeping the rest, once the store's queue is full or a
    /// failed read held handed-over slabs back.
    fn flush(&mut self, shared: &LodShared) -> bool {
        loop {
            if let Some(prepared) = self.unsent.take()
                && !self.send(shared, prepared)
            {
                return false;
            }
            let Some(dimension) = self
                .held
                .front()
                .map(|column| column.dimension)
                .or_else(|| self.held_slabs.first().map(|(dimension, _)| *dimension))
            else {
                return true;
            };
            let (columns, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held)
                .into_iter()
                .partition(|column| column.dimension == dimension);
            self.held = rest.into();
            let (slabs, rest): (BTreeSet<_>, BTreeSet<_>) = std::mem::take(&mut self.held_slabs)
                .into_iter()
                .partition(|(slab_dimension, _)| *slab_dimension == dimension);
            self.held_slabs = rest;
            let slabs = slabs.into_iter().map(|(_, slab)| slab).collect();
            match self.prepare(shared, dimension, columns, slabs) {
                Ok(prepared) => {
                    self.failed_slab_merges = 0;
                    self.unsent = Some(prepared);
                }
                Err((LoadError::Full, columns, slabs)) => {
                    self.hold(dimension, columns, slabs);
                    return false;
                }
                Err((LoadError::Failed, columns, slabs)) => {
                    Counters::bump(&shared.counters.store_errors);
                    forget(shared, dimension, &columns);
                    if !slabs.is_empty() && self.failed_slab_merges < MAX_SLAB_MERGE_ATTEMPTS {
                        self.failed_slab_merges += 1;
                        self.hold(dimension, Vec::new(), slabs);
                        return false;
                    }
                    self.failed_slab_merges = 0;
                }
            }
        }
    }

    /// Holds `columns` ahead of every held column and `slabs` with the held slabs, to
    /// merge again.
    fn hold(&mut self, dimension: Dimension, columns: Vec<SampledColumn>, slabs: Vec<SlabWork>) {
        for column in columns.into_iter().rev() {
            self.held.push_front(column);
        }
        self.held_slabs
            .extend(slabs.into_iter().map(|slab| (dimension, slab)));
    }

    /// Merges `columns` and handed-over `slabs` of `dimension` into the sections this
    /// writer owns, read through the cache in the store's current session epoch, and
    /// prepares their writes: sections, then each column's surface and content hash,
    /// which vouches for the sections before it. Hands the work back when a read failed.
    pub(in crate::stream) fn prepare(
        &mut self,
        shared: &LodShared,
        dimension: Dimension,
        columns: Vec<SampledColumn>,
        slabs: Vec<SlabWork>,
    ) -> Result<Prepared, Unmerged> {
        let epoch = shared.client.session_epoch();
        let generation = shared.generation_at(epoch);
        if generation != self.cached_generation {
            self.cache.clear();
            self.cached_generation = generation;
        }
        let blocks = match columns.first() {
            Some(column) => {
                shared.note_blocks(&column.blocks);
                Some(Arc::clone(&column.blocks))
            }
            None => shared.latest_blocks(),
        };
        if let Some(blocks) = blocks {
            self.classes.refresh(&blocks, &shared.client.registry());
        }
        let now = shared.now();
        let mut source = StoreSections {
            client: &shared.client,
            cache: &mut self.cache,
            codec: &mut self.codec,
            dimension,
            now,
        };
        let (writers, index) = (shared.writers, self.index);
        let merged = match merge_owned(
            columns.iter().map(|column| &column.sample),
            slabs.iter().copied(),
            |pos| writers.owns(index, pos),
            &mut source,
            &self.classes,
        ) {
            Ok(merged) => merged,
            Err(error) => return Err((error, columns, slabs)),
        };
        let sections = merged.sections.len() as u64;
        let written = merged
            .sections
            .iter()
            .map(|section| section.pos().key())
            .collect();
        let mut requests = Vec::with_capacity(merged.sections.len() + 2 * columns.len());
        for section in merged.sections {
            let record = self.codec.encode(&section);
            self.cache
                .insert(dimension, section.pos().key(), Some(section));
            requests.push(Request::Put {
                dimension,
                record: record.into(),
                now,
            });
        }
        for column in &columns {
            if !column.sample.sub_chunks.is_empty() {
                requests.push(Request::Surface {
                    dimension,
                    chunk_x: column.chunk.x,
                    chunk_z: column.chunk.z,
                    surface: column.surface.clone(),
                    now,
                });
            }
            requests.push(Request::ChunkHash {
                dimension,
                x: column.chunk.x,
                z: column.chunk.z,
                hash: Some(column.hash),
            });
        }
        Ok(Prepared {
            dimension,
            epoch,
            columns,
            slabs,
            foreign: merged.foreign,
            written,
            requests,
            sections,
        })
    }

    /// Sends `prepared`; false, keeping it unsent, while the store's queue is full. Taken
    /// writes hand their foreign slabs over; writes refused as stale drop the cache and
    /// hold their columns and slabs to merge again; a stopped store forgets their hashes.
    pub(in crate::stream) fn send(&mut self, shared: &LodShared, prepared: Prepared) -> bool {
        let Prepared {
            dimension,
            epoch,
            columns,
            slabs,
            foreign,
            written,
            requests,
            sections,
        } = prepared;
        match shared.client.write_in_epoch(epoch, requests) {
            Ok(EpochWrite::Taken) => {
                Counters::add(&shared.counters.merged_columns, columns.len() as u64);
                Counters::add(&shared.counters.sections_written, sections);
                shared.notify_written(dimension, &written);
                shared.hand_over(dimension, foreign);
            }
            Ok(EpochWrite::Stale) => {
                Counters::bump(&shared.counters.stale_writes);
                self.cache.clear();
                self.hold(dimension, columns, slabs);
            }
            Err(SubmitError::Full(requests)) => {
                Counters::bump(&shared.counters.store_queue_full);
                self.unsent = Some(Prepared {
                    dimension,
                    epoch,
                    columns,
                    slabs,
                    foreign,
                    written,
                    requests,
                    sections,
                });
                return false;
            }
            Err(SubmitError::Stopped(_)) => {
                Counters::bump(&shared.counters.store_errors);
                forget(shared, dimension, &columns);
            }
        }
        true
    }
}

/// Forgets the hashes of `columns` of `dimension`, whose writes were lost.
fn forget(shared: &LodShared, dimension: Dimension, columns: &[SampledColumn]) {
    let mut book = shared.lock_book();
    for column in columns {
        book.forget((dimension, column.chunk));
    }
}
