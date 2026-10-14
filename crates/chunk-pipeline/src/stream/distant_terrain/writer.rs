//! The writer thread: asks the store for content hashes, merges sampled columns into the
//! sections they share and submits the results.
//!
//! It is the only thread that talks to the store, so world workers never wait for disk,
//! and every section key has one writer, so concurrent columns never lose each other's
//! octants. It reads sections of the session's own epoch only, so an undecided candidate
//! never takes in a stored epoch's terrain, through a bounded cache in front of the store
//! client; the store answers a read only after every write submitted before it, so a
//! section evicted from the cache reads back as written.
//!
//! Each merge is prepared in one store [`SessionEpoch`] and sent with it: the cache is
//! dropped whenever the epoch moved on, and writes the store refuses because a session
//! changed while they were prepared (a candidate merged, or a session began or ended) are
//! merged again from what the store now holds. When the store's queue is full the writer
//! keeps the merged writes, and every column after them, until the frame nudges it, and
//! takes in nothing new before they are sent; until then ingestion is not settled.
//! Dropping them instead would lose final captures, which are never taken again. A column
//! whose writes the stopped store refused has its hash forgotten, so it is never skipped
//! as stored.

use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::{Arc, atomic::Ordering},
};

use lod::{
    codec::SectionCodec,
    geometry::{SectionKey, SectionPos},
    ingest::{SectionSource, merge_columns},
    section::Section,
};
use lod_store::{
    actor::{EpochWrite, ReadOutcome, ReadTarget, Request, SessionEpoch, StoreClient, SubmitError},
    identity::Dimension,
    minute::Minute,
};

use super::{
    LOD_SECTION_CACHE_ENTRIES, MERGE_BATCH_COLUMNS,
    blocks::MaterialClassTable,
    book::{StoredHash, Verdict},
    jobs,
    shared::{Capture, Counters, LodShared, SampledColumn, WriterInput},
};

/// The writer's state between batches.
#[derive(Default)]
pub(in crate::stream) struct Writer {
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
}

/// One dimension's merged columns and the writes that store them, prepared in `epoch`.
pub(in crate::stream) struct Prepared {
    dimension: Dimension,
    epoch: SessionEpoch,
    /// Kept to merge again should the store refuse the writes as stale; final captures
    /// among them stay counted until written.
    columns: Vec<SampledColumn>,
    requests: Vec<Request>,
    sections: u64,
}

/// Starts the writer thread for `shared`; it stops once ingestion is turned off.
pub(in crate::stream) fn spawn(shared: Arc<LodShared>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("lod-writer".into())
        .spawn(move || {
            crate::stream::workers::lower_beside_pool("lod-writer");
            run(&shared);
        })
        .map(drop)
}

/// The writer thread's loop. While the writer holds work back for the store's full queue
/// it waits only for the frame's nudge, and says so through [`LodShared::retry_due`]
/// before it stops being busy, so ingestion never reads as settled meanwhile.
fn run(shared: &Arc<LodShared>) {
    let mut writer = Writer::default();
    loop {
        let blocked = writer.is_blocked();
        shared.retry_due.store(blocked, Ordering::Release);
        let Some(batch) = shared.wait_for_work(MERGE_BATCH_COLUMNS, blocked) else {
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
    /// True while merged writes or accepted columns wait for room in the store's queue.
    pub(in crate::stream) fn is_blocked(&self) -> bool {
        self.unsent.is_some() || !self.held.is_empty()
    }

    /// Takes in `batch` (empty while blocked): checks go to the store, accepted samples
    /// join the held columns. Then sends and merges whatever is held, in order.
    pub(in crate::stream) fn step(&mut self, shared: &Arc<LodShared>, batch: Vec<WriterInput>) {
        for input in batch {
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

    /// Sends the unsent writes, then merges and sends the held columns a dimension at a
    /// time; false, keeping the rest, once the store's queue is full.
    fn flush(&mut self, shared: &LodShared) -> bool {
        loop {
            if let Some(prepared) = self.unsent.take()
                && !self.send(shared, prepared)
            {
                return false;
            }
            let Some(dimension) = self.held.front().map(|column| column.dimension) else {
                return true;
            };
            let (columns, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held)
                .into_iter()
                .partition(|column| column.dimension == dimension);
            self.held = rest.into();
            match self.prepare(shared, dimension, columns) {
                Ok(prepared) => self.unsent = Some(prepared),
                Err((LoadError::Full, columns)) => {
                    for column in columns.into_iter().rev() {
                        self.held.push_front(column);
                    }
                    return false;
                }
                Err((LoadError::Failed, columns)) => {
                    Counters::bump(&shared.counters.store_errors);
                    forget(shared, dimension, &columns);
                }
            }
        }
    }

    /// Merges `columns` of `dimension` into the sections they share, read through the
    /// cache in the store's current session epoch, and prepares their writes: sections,
    /// then each column's surface and content hash, which vouches for the sections before
    /// it. Hands the columns back when a read failed.
    pub(in crate::stream) fn prepare(
        &mut self,
        shared: &LodShared,
        dimension: Dimension,
        columns: Vec<SampledColumn>,
    ) -> Result<Prepared, (LoadError, Vec<SampledColumn>)> {
        let epoch = shared.client.session_epoch();
        let generation = shared.generation_at(epoch);
        if generation != self.cached_generation {
            self.cache.clear();
            self.cached_generation = generation;
        }
        if let Some(column) = columns.first() {
            self.classes
                .refresh(&column.blocks, &shared.client.registry());
        }
        let now = shared.now();
        let mut source = StoreSections {
            client: &shared.client,
            cache: &mut self.cache,
            codec: &mut self.codec,
            dimension,
            now,
        };
        let merged = match merge_columns(
            columns.iter().map(|column| &column.sample),
            &mut source,
            &self.classes,
        ) {
            Ok(merged) => merged,
            Err(error) => return Err((error, columns)),
        };
        let sections = merged.sections.len() as u64;
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
            requests,
            sections,
        })
    }

    /// Sends `prepared`; false, keeping it unsent, while the store's queue is full. Writes
    /// refused as stale drop the cache and hold their columns to merge again; a stopped
    /// store forgets their hashes.
    pub(in crate::stream) fn send(&mut self, shared: &LodShared, prepared: Prepared) -> bool {
        let Prepared {
            dimension,
            epoch,
            columns,
            requests,
            sections,
        } = prepared;
        match shared.client.write_in_epoch(epoch, requests) {
            Ok(EpochWrite::Taken) => {
                Counters::add(&shared.counters.merged_columns, columns.len() as u64);
                Counters::add(&shared.counters.sections_written, sections);
            }
            Ok(EpochWrite::Stale) => {
                Counters::bump(&shared.counters.stale_writes);
                self.cache.clear();
                for column in columns.into_iter().rev() {
                    self.held.push_front(column);
                }
            }
            Err(SubmitError::Full(requests)) => {
                Counters::bump(&shared.counters.store_queue_full);
                self.unsent = Some(Prepared {
                    dimension,
                    epoch,
                    columns,
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

/// Why a merge could not read a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) enum LoadError {
    /// The store's queue is full; retry later.
    Full,
    /// The store failed or stopped.
    Failed,
}

/// The store's sections of one dimension, read through the writer's cache.
struct StoreSections<'a> {
    client: &'a StoreClient,
    cache: &'a mut SectionCache,
    codec: &'a mut SectionCodec,
    dimension: Dimension,
    now: Minute,
}

impl SectionSource for StoreSections<'_> {
    type Error = LoadError;

    /// Cached sections, else the store's; corrupt records read as missing so the merge
    /// rebuilds them.
    fn load(&mut self, pos: SectionPos) -> Result<Option<Section>, LoadError> {
        let key = pos.key();
        if let Some(section) = self.cache.get(self.dimension, key) {
            return Ok(section);
        }
        let outcome = self
            .client
            .read(self.dimension, ReadTarget::OwnSection(key), self.now)
            .map_err(|error| match error {
                SubmitError::Full(()) => LoadError::Full,
                SubmitError::Stopped(()) => LoadError::Failed,
            })?;
        let section = match outcome {
            ReadOutcome::Found(record) => self.codec.decode_expecting(&record, key).ok(),
            ReadOutcome::Missing | ReadOutcome::Corrupt => None,
            ReadOutcome::ChunkHash(_) | ReadOutcome::Failed(_) => return Err(LoadError::Failed),
        };
        self.cache.insert(self.dimension, key, section.clone());
        Ok(section)
    }
}

/// Recently read or written sections by dimension and key, `None` for known misses,
/// bounded by [`LOD_SECTION_CACHE_ENTRIES`] with the least recently used dropped first.
#[derive(Default)]
struct SectionCache {
    entries: HashMap<(Dimension, SectionKey), (u64, Option<Section>)>,
    tick: u64,
}

impl SectionCache {
    /// The cached state of `key`; `None` when it is not cached.
    fn get(&mut self, dimension: Dimension, key: SectionKey) -> Option<Option<Section>> {
        self.tick += 1;
        let entry = self.entries.get_mut(&(dimension, key))?;
        entry.0 = self.tick;
        Some(entry.1.clone())
    }

    /// Caches `section` for `key`, dropping the least recently used quarter when full.
    fn insert(&mut self, dimension: Dimension, key: SectionKey, section: Option<Section>) {
        self.tick += 1;
        self.entries.insert((dimension, key), (self.tick, section));
        if self.entries.len() > LOD_SECTION_CACHE_ENTRIES {
            let mut ticks: Vec<u64> = self.entries.values().map(|(tick, _)| *tick).collect();
            let cut = ticks.len() / 4;
            let (_, &mut oldest_kept, _) = ticks.select_nth_unstable(cut);
            self.entries.retain(|_, (tick, _)| *tick >= oldest_kept);
        }
    }

    /// Forgets every section.
    fn clear(&mut self) {
        self.entries.clear();
    }
}
