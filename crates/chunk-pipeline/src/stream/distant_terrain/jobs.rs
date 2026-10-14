//! Sample jobs: content hash, level-0 sampling and surface signatures of one column. They
//! never wait: the store is only ever asked by the writer threads.

use std::{sync::Arc, time::Instant};

use lod::{
    epoch::ChunkSurface,
    sampler::{ColumnSample, ColumnSnapshot, sample_column},
};
use lod_store::identity::Dimension;

use super::{
    blocks::ColumnClassifier,
    book::Verdict,
    fingerprint::Fingerprint,
    hash::{block_hash, content_hash},
    shared::{Capture, CaptureKind, Counters, LodShared, SampledColumn, WriterInput},
};
use crate::stream::workers::{DispatchBatch, Lane, WORKERS};

/// Runs [`sample`] for `capture` on the Lod lane.
pub(in crate::stream) fn spawn(shared: Arc<LodShared>, capture: Capture) {
    Counters::bump(&shared.counters.jobs_queued);
    WORKERS.spawn(Lane::Lod, move || sample(&shared, capture));
}

/// Adds [`sample`] for `capture` to `batch`, a batch of Lod-lane jobs.
pub(in crate::stream) fn queue(
    batch: &mut DispatchBatch<'_>,
    shared: Arc<LodShared>,
    capture: Capture,
) {
    Counters::bump(&shared.counters.jobs_queued);
    batch.spawn(false, move || sample(&shared, capture));
}

/// Samples `capture`; a capture without a checked hash is hashed first and, unless it was
/// relit, skipped when the column book shows its content stored or a newer capture taken,
/// and a column whose stored hash the book does not know goes to the writer, which asks
/// the store and sends it back here when it changed. A changed capture reports what its
/// hash covered to the frame. The sample is then queued for the writer; a changed column
/// finding the inbox full goes back to the frame to be captured again ([`refused`]).
pub(in crate::stream) fn sample(shared: &Arc<LodShared>, capture: Capture) {
    if !shared.enabled() {
        return;
    }
    #[cfg(feature = "tracy")]
    let _zone = tracing::info_span!("stream.lod_sample").entered();
    let started = Instant::now();
    sample_timed(shared, capture);
    shared.counters.add_worker_time(started.elapsed());
}

/// [`sample`] without its timing.
fn sample_timed(shared: &Arc<LodShared>, mut capture: Capture) {
    let counters = &shared.counters;
    let input = &capture.input;
    let dimension = Dimension(input.chunk.dimension);
    let registry = shared.client.registry();
    let blocks = Arc::clone(&input.blocks);
    let mut classifier = ColumnClassifier::new(&blocks, &registry);
    let snapshot = input.snapshot();
    let hash = match capture.checked_hash {
        Some(hash) => hash,
        None => {
            let blocks_hash = block_hash(&snapshot, &mut classifier);
            if capture.kind == CaptureKind::Changed {
                shared.report_hashed(input.chunk, Fingerprint::of(input, blocks_hash));
            }
            let hash = content_hash(blocks_hash, &snapshot);
            let verdict = if capture.relit {
                Verdict::Changed
            } else {
                shared.lock_book().check(
                    (dimension, input.chunk),
                    capture.seq,
                    hash,
                    shared.generation(),
                )
            };
            match verdict {
                Verdict::Superseded => return Counters::bump(&counters.superseded),
                Verdict::Unchanged => return Counters::bump(&counters.skipped_unchanged),
                Verdict::Changed => hash,
                Verdict::Unknown => {
                    capture.checked_hash = Some(hash);
                    if let Err(input) = shared.push(WriterInput::Check(Box::new(capture))) {
                        refused(shared, input);
                    }
                    return;
                }
            }
        }
    };
    let Some((sample, surface)) = sample_snapshot(&snapshot, &mut classifier) else {
        return Counters::bump(&counters.unkeyed_columns);
    };
    Counters::bump(&counters.sampled);
    counters.add_classified(classifier.counts());
    let column = SampledColumn {
        seq: capture.seq,
        kind: capture.kind,
        dimension,
        chunk: input.chunk,
        sample,
        surface,
        hash,
        blocks: Arc::clone(&input.blocks),
        _final: capture.final_capture.take(),
    };
    if let Err(input) = shared.push(WriterInput::Sampled(Box::new(column))) {
        refused(shared, input);
    }
}

/// Hands a changed `input` the writer inbox refused while ingestion runs back to the frame
/// ([`LodShared::defer`]), so its column is captured again instead of losing its change.
/// A sampled column was already found changed or relit, so it is sampled again whatever its
/// hash; a check keeps its relit flag. Final captures are never refused while ingestion
/// runs, and nothing is kept once it is off.
pub(in crate::stream) fn refused(shared: &LodShared, input: WriterInput) {
    if !shared.enabled() || input.kind() != CaptureKind::Changed {
        return;
    }
    let resample = match &input {
        WriterInput::Check(capture) => capture.relit,
        WriterInput::Sampled(_) => true,
    };
    shared.defer(input.chunk(), resample);
}

/// Samples `snapshot` at level 0 with its surface signature; `None` for a column outside
/// the section key range.
fn sample_snapshot(
    snapshot: &ColumnSnapshot<'_>,
    classifier: &mut ColumnClassifier<'_>,
) -> Option<(ColumnSample, Box<ChunkSurface>)> {
    let sample = sample_column(snapshot, classifier).ok()?;
    let surface = Box::new(ChunkSurface::from_sample(&sample, classifier));
    Some((sample, surface))
}

/// Hashes and samples `input` as a sample job does, without the column book, with
/// materials assigned in `registry`; for benchmarks.
#[cfg(feature = "benchmark-support")]
pub(in crate::stream) fn hash_and_sample(
    input: &super::shared::ColumnInput,
    registry: &lod_store::store::SharedRegistry,
) -> Option<(u64, ColumnSample, Box<ChunkSurface>)> {
    let mut classifier = ColumnClassifier::new(&input.blocks, registry);
    let snapshot = input.snapshot();
    let hash = content_hash(block_hash(&snapshot, &mut classifier), &snapshot);
    let (sample, surface) = sample_snapshot(&snapshot, &mut classifier)?;
    Some((hash, sample, surface))
}
