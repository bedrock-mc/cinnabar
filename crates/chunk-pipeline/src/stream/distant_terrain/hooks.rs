//! Where the stream feeds ingestion: changes, evictions, the poll's dispatch and the final
//! capture of a stream that stops ingesting.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, atomic::Ordering},
};

use client_world::{WorldAuthority, ingestion::default_biome_id};
use lod_store::identity::Dimension;
use world::{Chunk, ChunkKey, LightStoreSnapshot, SubChunkKey};

use super::{
    LOD_SETTLE_POLLS, LodIngest, MAX_LOD_BACKLOG, MAX_LOD_COLUMNS_EXAMINED_PER_POLL,
    MAX_LOD_COLUMNS_PER_POLL, MAX_LOD_FINAL_CAPTURES_PER_POLL, MAX_LOD_PENDING_FINAL_CAPTURES,
    blocks::SessionBlocks,
    book::Verdict,
    hash::content_hash,
    jobs,
    shared::{Capture, CaptureKind, ColumnBiomes, ColumnInput, Counters, InFlight, WriterInput},
};
use crate::stream::{
    WorldStream,
    lighting::Lighting,
    workers::{Lane, WORKERS},
};

/// What an eviction leaves for the stream to release off the frame: retired columns no
/// capture took, and captures that needed no job.
#[derive(Default)]
pub(in crate::stream) struct Released {
    columns: Vec<(ChunkKey, Chunk)>,
    captures: Vec<Capture>,
}

impl Released {
    /// True when there is nothing to release.
    pub(in crate::stream) fn is_empty(&self) -> bool {
        self.columns.is_empty() && self.captures.is_empty()
    }
}

/// Where a capture goes next.
enum Route {
    /// Nothing more to do: its content is stored, or a newer capture was taken.
    Skip(Capture),
    /// The writer asks the store for the column's hash first.
    Check(Capture),
    /// A sample job hashes it if needed and samples it.
    Sample(Capture),
}

impl Route {
    /// The capture, wherever it was going.
    fn into_capture(self) -> Capture {
        match self {
            Self::Skip(capture) | Self::Check(capture) | Self::Sample(capture) => capture,
        }
    }
}

impl WorldStream {
    /// Marks the columns of changed `sources` dirty, restarting their settle count.
    pub(in crate::stream) fn mark_lod_dirty(&mut self, sources: &BTreeSet<SubChunkKey>) {
        let Some(lod) = self
            .lod
            .as_deref_mut()
            .filter(|lod| lod.finishing.is_none())
        else {
            return;
        };
        for key in sources {
            lod.mark(key.chunk());
        }
    }

    /// Marks `column` relit after a light job changed its stored light values: it is
    /// sampled again once settled, even though its content hash leaves light out, so light
    /// a neighbour brought or took away reaches the store. A column still waiting for its
    /// first capture since it changed takes the new light with it, and its hash may skip
    /// it as before, so a revisit never resamples columns only for their light.
    pub(in crate::stream) fn mark_lod_relit(&mut self, column: ChunkKey) {
        let Some(lod) = self
            .lod
            .as_deref_mut()
            .filter(|lod| lod.finishing.is_none())
        else {
            return;
        };
        if lod.dirty.contains_key(&column) && !lod.relit.contains(&column) {
            return;
        }
        lod.relit.insert(column);
        lod.mark(column);
    }

    /// Takes the `retired` columns of an eviction as final captures, with the light of
    /// `keys` taken before the eviction removes it, and forgets their pending changes.
    /// Each capture needing a sample owns its retired column, which its job releases; the
    /// rest come back for the caller to release off the frame: every column while
    /// ingestion is off, captures that need nothing more, and those that would take more
    /// than [`MAX_LOD_PENDING_FINAL_CAPTURES`] unwritten ones, counted.
    pub(in crate::stream) fn capture_lod_eviction(
        &mut self,
        retired: Vec<(ChunkKey, Chunk)>,
        keys: &BTreeSet<SubChunkKey>,
    ) -> Released {
        if self.lod.is_none() {
            return Released {
                columns: retired,
                captures: Vec::new(),
            };
        }
        let light = Arc::new(self.solved_light(keys.iter().copied()));
        let lod = self.lod.as_deref_mut().expect("ingestion checked above");
        let blocks = session_blocks(&mut lod.blocks, &self.authority);
        let shared = Arc::clone(&lod.shared);
        let mut released = Released::default();
        let mut routed = Vec::with_capacity(retired.len());
        // Captures routed below count once sent.
        let mut pending = shared.final_captures.load(Ordering::SeqCst);
        for (chunk, column) in retired {
            lod.dirty.remove(&chunk);
            let relit = lod.relit.remove(&chunk);
            let fingerprint = lod.fingerprints.remove(&chunk);
            let input = ColumnInput {
                chunk,
                sub_chunks: column.shared_sub_chunks(),
                default_biome: default_biome_id(chunk.dimension),
                biomes: ColumnBiomes::Retired(column),
                light: Arc::clone(&light),
                blocks: Arc::clone(&blocks),
            };
            match lod.route(input, CaptureKind::Final, fingerprint.as_ref(), relit) {
                Route::Skip(capture) => released.captures.push(capture),
                route if pending < MAX_LOD_PENDING_FINAL_CAPTURES => {
                    pending += 1;
                    routed.push(route);
                }
                route => {
                    Counters::bump(&shared.counters.dropped_final_captures);
                    released.captures.push(route.into_capture());
                }
            }
        }
        lod.send(routed, &mut released.captures);
        released
    }

    /// Advances the settle count, keeps the fingerprints sample jobs reported for
    /// resident columns, nudges a writer the store's full queue stopped, and, within this
    /// poll's budget, takes up to [`MAX_LOD_COLUMNS_PER_POLL`] due columns. Those whose
    /// content is unchanged are skipped; the rest are queued while fewer than
    /// [`MAX_LOD_BACKLOG`] samples are outstanding, and dropped and counted beyond that.
    /// A stream finishing ingestion takes its next resident columns as final captures
    /// instead.
    pub(in crate::stream) fn dispatch_lod_work(&mut self) {
        let Some(lod) = self.lod.as_deref_mut() else {
            return;
        };
        lod.polls += 1;
        let terrain = self.authority.terrain();
        for (column, fingerprint) in lod.shared.take_hashed() {
            if terrain.chunk(column).is_some() {
                lod.fingerprints.insert(column, fingerprint);
            }
        }
        lod.shared.nudge_writer();
        if lod.finishing.is_some() {
            self.capture_final_columns(MAX_LOD_FINAL_CAPTURES_PER_POLL);
            return;
        }
        if lod.dirty.is_empty() || self.poll_budget_exhausted() {
            return;
        }
        let (resident, lighting) = (&self.resident, &self.lighting);
        let light_settled = |column: ChunkKey| {
            resident
                .column(column)
                .all(|key| light_solved(lighting, key))
        };
        let Some(lod) = self.lod.as_deref_mut() else {
            return;
        };
        let due = lod.take_due(light_settled);
        let mut blocks = lod.blocks.take();
        let inputs: Vec<ColumnInput> = due
            .into_iter()
            .filter_map(|column| {
                let column_blocks = session_blocks(&mut blocks, &self.authority);
                self.lod_column_input(column, column_blocks)
            })
            .collect();
        let Some(lod) = self.lod.as_deref_mut() else {
            return;
        };
        lod.blocks = blocks;
        let shared = Arc::clone(&lod.shared);
        let mut room = MAX_LOD_BACKLOG
            .saturating_sub(shared.in_flight.load(Ordering::Acquire) + shared.queued());
        let mut routed = Vec::with_capacity(inputs.len());
        for input in inputs {
            let column = input.chunk;
            let relit = lod.relit.remove(&column);
            let fingerprint = lod.fingerprints.remove(&column);
            let route = lod.route(input, CaptureKind::Changed, fingerprint.as_ref(), relit);
            if let Some(fingerprint) = fingerprint {
                lod.fingerprints.insert(column, fingerprint);
            }
            match route {
                Route::Skip(_) => {}
                route if room > 0 => {
                    room -= 1;
                    routed.push(route);
                }
                _ => {
                    // A final capture still needs to know the stored light is stale.
                    if relit {
                        lod.relit.insert(column);
                    }
                    Counters::bump(&shared.counters.dropped);
                }
            }
        }
        let mut skipped = Vec::new();
        lod.send(routed, &mut skipped);
    }

    /// Takes up to `budget` of the columns a finishing stream still has to capture as final
    /// captures with their light, so a stream that stops ingesting keeps its explored
    /// state, and stops ingesting once none is left, or at once when its owner already
    /// dropped the link. Columns evicted meanwhile were captured by their eviction.
    pub(in crate::stream) fn capture_final_columns(&mut self, budget: usize) {
        let Some(lod) = self.lod.as_deref_mut() else {
            return;
        };
        let Some(finishing) = lod.finishing.as_mut() else {
            return;
        };
        if !lod.shared.enabled() {
            self.lod = None;
            return;
        }
        let columns: Vec<ChunkKey> = finishing.drain(..budget.min(finishing.len())).collect();
        let done = finishing.is_empty();
        let mut blocks = lod.blocks.take();
        let inputs: Vec<ColumnInput> = columns
            .into_iter()
            .filter_map(|column| {
                let column_blocks = session_blocks(&mut blocks, &self.authority);
                self.lod_column_input(column, column_blocks)
            })
            .collect();
        let Some(lod) = self.lod.as_deref_mut() else {
            return;
        };
        lod.blocks = blocks;
        let routed = inputs
            .into_iter()
            .map(|input| {
                let relit = lod.relit.remove(&input.chunk);
                let fingerprint = lod.fingerprints.remove(&input.chunk);
                lod.route(input, CaptureKind::Final, fingerprint.as_ref(), relit)
            })
            .collect();
        let mut skipped = Vec::new();
        lod.send(routed, &mut skipped);
        if done {
            self.lod = None;
        }
    }
}

impl WorldStream {
    /// Captures resident `column` for a sample job: shared sub-chunks, the biomes of every
    /// Y the sampler may read and the column's current light; `None` when it is not
    /// resident.
    pub(in crate::stream) fn lod_column_input(
        &self,
        column: ChunkKey,
        blocks: Arc<SessionBlocks>,
    ) -> Option<ColumnInput> {
        let chunk = self.authority.terrain().chunk(column)?;
        let sub_chunks = chunk.shared_sub_chunks();
        // The stored range widened to whole level-0 sections, as an evicted column reads it.
        let ys = sub_chunks
            .keys()
            .next()
            .zip(sub_chunks.keys().next_back())
            .into_iter()
            .flat_map(|(&low, &high)| low.div_euclid(2) * 2..=high.div_euclid(2) * 2 + 1);
        let biomes = ys
            .filter_map(|y| Some((y, chunk.biome_storage(y)?)))
            .collect::<BTreeMap<_, _>>();
        let light = self.solved_light(self.resident.column(column).copied());
        Some(ColumnInput {
            chunk: column,
            sub_chunks,
            biomes: ColumnBiomes::Captured(biomes),
            default_biome: default_biome_id(column.dimension),
            light: Arc::new(light),
            blocks,
        })
    }

    /// The light of those of `keys` with no light work pending or running, whose light is
    /// solved; the others are left out, so sampling lights them by the column's heightmap
    /// rather than by a placeholder, and their solved light later counts as a change.
    fn solved_light(&self, keys: impl Iterator<Item = SubChunkKey>) -> LightStoreSnapshot {
        let lighting = &self.lighting;
        lighting
            .store
            .snapshot_keys(keys.filter(|key| light_solved(lighting, key)))
    }

    /// Captures, hashes and samples resident `column` as a sample job does, without the
    /// store, assigning materials in `registry`; returns the content hash and the sampled
    /// sub-chunk count. For benchmarks.
    #[cfg(feature = "benchmark-support")]
    pub(in crate::stream) fn sample_lod_column_for_bench(
        &self,
        column: ChunkKey,
        registry: &lod_store::store::SharedRegistry,
    ) -> Option<(u64, usize)> {
        let input = self.lod_column_input(column, session_blocks(&mut None, &self.authority))?;
        let (hash, sample, _) = jobs::hash_and_sample(&input, registry)?;
        Some((hash, sample.sub_chunks.len()))
    }
}

impl LodIngest {
    /// Marks `column` changed at this poll.
    pub(super) fn mark(&mut self, column: ChunkKey) {
        let polls = self.polls;
        if self.dirty.insert(column, polls) == Some(polls) {
            return;
        }
        self.order.push_back((polls, column));
    }

    /// The sequence number of a capture taken now.
    pub(in crate::stream) fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// Takes `input` as a capture of `kind` and decides where it goes. When `fingerprint`
    /// shows the column's allocations unchanged since a job hashed them, the frame finishes
    /// the hash and checks the column book itself, so an unchanged column queues no job;
    /// a `relit` column is sampled whatever its hash.
    fn route(
        &mut self,
        input: ColumnInput,
        kind: CaptureKind,
        fingerprint: Option<&super::fingerprint::Fingerprint>,
        relit: bool,
    ) -> Route {
        let seq = self.next_seq();
        let shared = &self.shared;
        let mut capture = Capture {
            seq,
            kind,
            input,
            checked_hash: None,
            relit,
            job: None,
            final_capture: None,
        };
        let Some(fingerprint) = fingerprint.filter(|print| print.matches(&capture.input)) else {
            return Route::Sample(capture);
        };
        let hash = content_hash(fingerprint.hash, &capture.input.snapshot());
        capture.checked_hash = Some(hash);
        if relit {
            return Route::Sample(capture);
        }
        let column = (
            Dimension(capture.input.chunk.dimension),
            capture.input.chunk,
        );
        let verdict = shared
            .lock_book()
            .check(column, capture.seq, hash, shared.generation());
        match verdict {
            Verdict::Superseded => {
                Counters::bump(&shared.counters.superseded);
                Route::Skip(capture)
            }
            Verdict::Unchanged => {
                Counters::bump(&shared.counters.skipped_unchanged);
                Route::Skip(capture)
            }
            Verdict::Changed => Route::Sample(capture),
            Verdict::Unknown => Route::Check(capture),
        }
    }

    /// Sends `routed` captures on: checks to the writer, samples as one batch of Lod jobs;
    /// skipped ones go to `skipped`. Final captures sent on count in
    /// [`super::shared::LodShared::final_captures`] until written. A check the full inbox
    /// refuses is dropped, counted.
    fn send(&mut self, routed: Vec<Route>, skipped: &mut Vec<Capture>) {
        let shared = &self.shared;
        let mut batch = WORKERS.batch(Lane::Lod);
        let start = |capture: &mut Capture| {
            capture.job = Some(InFlight::start(shared, InFlight::SAMPLE));
            if capture.kind == CaptureKind::Final {
                capture.final_capture = Some(InFlight::start(shared, InFlight::FINAL));
            }
        };
        for route in routed {
            match route {
                Route::Skip(capture) => skipped.push(capture),
                Route::Check(mut capture) => {
                    start(&mut capture);
                    if let Err(input) = shared.push(WriterInput::Check(Box::new(capture))) {
                        jobs::count_drop(shared, input.kind());
                    }
                }
                Route::Sample(mut capture) => {
                    start(&mut capture);
                    jobs::queue(&mut batch, Arc::clone(shared), capture);
                }
            }
        }
    }

    /// Removes and returns up to [`MAX_LOD_COLUMNS_PER_POLL`] columns that have settled
    /// and whose light `light_settled` reports done, checking at most
    /// [`MAX_LOD_COLUMNS_EXAMINED_PER_POLL`] columns; those still waiting for light move
    /// to the back.
    fn take_due(&mut self, light_settled: impl Fn(ChunkKey) -> bool) -> Vec<ChunkKey> {
        let mut due = Vec::new();
        let mut examined = 0;
        while due.len() < MAX_LOD_COLUMNS_PER_POLL && examined < MAX_LOD_COLUMNS_EXAMINED_PER_POLL {
            let Some(&(changed, column)) = self.order.front() else {
                break;
            };
            if self.dirty.get(&column) != Some(&changed) {
                self.order.pop_front();
                continue;
            }
            if self.polls.saturating_sub(changed) < LOD_SETTLE_POLLS {
                break;
            }
            self.order.pop_front();
            examined += 1;
            if light_settled(column) {
                self.dirty.remove(&column);
                due.push(column);
            } else {
                self.order.push_back((changed, column));
            }
        }
        due
    }
}

/// True when no light work is pending or running for `key`, so its stored light is solved.
fn light_solved(lighting: &Lighting, key: &SubChunkKey) -> bool {
    !lighting.jobs.pending.contains_key(key) && !lighting.jobs.in_flight.contains_key(key)
}

/// The session's block identities, rebuilt when its assets or custom states changed.
pub(in crate::stream) fn session_blocks(
    cached: &mut Option<Arc<SessionBlocks>>,
    authority: &WorldAuthority,
) -> Arc<SessionBlocks> {
    let assets = authority.runtime_assets();
    let mode = authority.network_id_mode();
    let custom = authority.custom_block_states();
    match cached {
        Some(blocks) if blocks.matches(assets, mode, custom) => Arc::clone(blocks),
        _ => {
            let blocks = Arc::new(SessionBlocks::new(
                Arc::clone(assets),
                mode,
                Arc::clone(custom),
            ));
            *cached = Some(Arc::clone(&blocks));
            blocks
        }
    }
}
