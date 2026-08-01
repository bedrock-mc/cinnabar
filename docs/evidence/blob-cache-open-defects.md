# Cinnabar blob-cache open defects

This document records Cinnabar implementation defects, not vanilla behavior.
Vanilla observations and deliberate Cinnabar divergences remain in
[vanilla-blob-cache-1.26.30.md](vanilla-blob-cache-1.26.30.md). The findings
below were checked against the named commits so that they remain available if
the implementation is resumed or discarded.

## Current PR head: `9b7085d`

These defects are open on the current PR head.

### 1. SubChunk abandonment has no recovery path

In `crates/protocol/src/blob_cache/resolver.rs`,
`pending_packet_recovery` returns a `ChunkResyncEvent` for
`PendingPacket::LevelChunk` but returns `None` for
`PendingPacket::SubChunk`. `abandon_pending_transaction` only queues recovery
when that function returns an event, so abandoning a cached SubChunk does not
produce a resync request.

The remaining path is the world scheduler's bounded retry handling. At retry
exhaustion, and on the other terminal retry failures,
`crates/client-world/src/stream/retries.rs` calls
`complete_requested_sub_chunk(key, false)` (including the call at line 246).
That function cancels the retry and removes the expected section but applies no
payload. The stream can retain retry/normalization counters, but it emits no
world-data recovery event. This is silent world-data loss and does not satisfy
the repository's lenient handling contract for well-formed but semantically
odd server data.

### 2. `reset_pending` discards queued world payloads without counting them

`BlobCacheResolver::reset_pending` in
`crates/protocol/src/blob_cache/resolver.rs` clears `pending`, `ready`,
`immediate_ready`, and `recovery_ready`. The last three queues can contain
already decoded world payloads or recovery work; `recovery_ready` exists
specifically to prevent abandoned cache work from becoming lost.

The function increments `pending_resets` at most once, and only when
`pending` or `ready` is non-empty. It does not count the entries discarded from
`immediate_ready` or `recovery_ready`, and it resets their published sizes to
zero. The reset is used by the transfer/disconnect boundary in
`crates/protocol/src/login.rs` and by `fail_session` at session-failure
boundaries. The result is queued world work being dropped with no per-item
accounting or recovery record.

### 3. Published skipped counters are never incremented at this head

`BlobCacheStats::skipped_packets` and `skipped_world_events` are declared in
`crates/protocol/src/blob_cache.rs` and published by the runtime telemetry and
phase-2 evidence paths, but the current `9b7085d` code contains no increment of
either field. The admission-time drop path that incremented them was removed
by `0a4eaa6`; the current admission path returns explicit backpressure instead
of silently dropping the event.

Therefore zero means that the corresponding admission drop does not exist at
this head. It must not be read as evidence that the counters are disconnected
or that the old drop behavior has been fixed without replacement.

## Parked branch: `fix/blobcache-outstanding-dedupe` at `1612c9e`

This branch is not merged. It adds an `outstanding` classification set so a
hash already owned by an in-flight transaction is omitted from later status
packets, and it adds exact per-section SubChunk recovery. Independent review
returned **NEEDS CHANGES** with the following Important findings.

### 4. Orphaned waiters

Suppose column A requests hash `H`, then column B references `H`. In
`crates/protocol/src/blob_cache/resolver/status.rs`, B moves `H` from
`missing` to `outstanding`; `into_packets` emits neither set, so B sends no
request for `H`. B nevertheless records `H` in its unresolved-hash count and
in `pending_by_hash`.

If A is abandoned and its original miss response is dropped, removing A does
not resolve H for B. B remains pending and keeps H classified as outstanding,
but no path reissues H or recovers B. An empty miss response is also a no-op in
the resolver, so it does not repair this state.

### 5. Partial placement loses recovery ranges

Admission in `crates/client-world/src/stream/construction.rs` reserves one
outbound request slot for a request-producing world event. In the parked
branch, `enqueue_exact_recovery_requests` in
`crates/client-world/src/stream/requests.rs` first uses that reservation for
one contiguous missing range. When that placement succeeds, it sets the
reservation to `None`, so every later disjoint range uses ordinary placement.

If ordinary placement fails for a later range, the code records
`OutboundRequestPlacementFailure` and continues. It adds those sections to
`requested_sub_chunks` only after placement succeeds. The failed range is
therefore neither marked expected nor retried; the cleanup at the end only
cancels an unused original reservation and does not repair later failures.

### 6. Recovery aggregation is quadratic

`enqueue_recovery` in the parked resolver linearly scans `recovery_ready` to
find a matching column. When it finds one, it linearly scans the existing
section list with `contains` while merging incoming section Y values.

Under the reviewed worst-case envelope of up to 256 retained transactions,
each contributing up to 256 recovery sections, there can be about 65,536
recovery contributions. If those contributions are arranged as distinct
columns, the linear `recovery_ready` scans alone approach
`65,536 * 65,535 / 2`, or roughly 2.1 billion equality comparisons. The
coordinates and section entries are server-controlled well-formed input, so
this is remotely triggerable. The branch has no indexing or work budget that
changes this aggregation cost to bounded work.

## Open BDS question

The resolution-blocking question is why BDS answers cache statuses with empty
miss responses. The observed run data was recorded as 257/184 empty responses
against 238-239 permanently pending transactions. Those counts are runtime
observations and cannot be independently derived from the source commits.

The resolver increments `empty_miss_responses` when a miss response contains
no blobs and returns without associating that response with an originating
status. The telemetry likewise publishes aggregate counters but no status or
response correlation. Code analysis therefore cannot determine whether those
empty responses followed missing-bearing statuses or have-only statuses.

`9b7085d` adds `redundant_missing_requests` to test one hypothesis. Its result
is not known here; this document records the question without drawing a
conclusion.
