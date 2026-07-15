# World publication performance Task 1 report

Date: 2026-07-15

Branch: `phase2-world-publication-performance`

Base: `4dca51c16068df4d65272e2d5477d5eb873c8b5b`

Commit: this report is part of the Task 1 commit `perf: skip unchanged light remeshes`; the final immutable hash is reported in the handoff because a commit cannot contain its own hash.

## Scope

This commit implements Task 1 only from
`docs/superpowers/plans/2026-07-15-world-publication-performance.md`.
It does not add latency attribution, change acceptance scripts, narrow
changed-light invalidation, modify `plan.md`, or alter light values, AO,
culling, cave connectivity, presentation, draw mode, or shader output.

The light worker now records exact sampled-nibble equality and exact
direct-sky-mask equality. Accepted completions are classified as no-op,
provenance-only, or value-changing. No-op publication retains both stored
`Arc` identities and the sampled light generation while advancing ownership to
the current block generation. Provenance-only publication retains the light
`Arc` and sampled generation, replaces only the direct-sky mask tagged with
that generation, and performs no mesh invalidation. Value changes retain the
existing full 27-dependent invalidation.

`MeshLightSlot` retains only the key, block generation, sampled light
generation, and `Arc<SubChunkLight>`. Snapshot construction still requires
`light_is_current`, including current direct-sky provenance, before removing
the direct-sky pointer from later mesh-output currentness checks.

`WorldStreamStats` now exposes saturating cumulative counters for accepted,
no-op, value-changing, provenance-only, and mesh-invalidating light
completions.

## TDD RED evidence

The clean base first passed the existing focused light scheduler and full
`world` suite.

1. Exact required no-op filter:

   `cargo test -p bedrock-client world_stream::tests::light_scheduler::unchanged --locked -- --nocapture`

   RED: both uniform and packed tests failed at `Arc::ptr_eq` because the old
   completion path replaced the stored light identity and invalidated mesh
   dependants even when all sampled output was equal.

2. Outcome interface:

   The same command then failed to compile with missing
   `accepted_light_jobs`, `noop_light_jobs`, `value_changed_light_jobs`,
   `provenance_only_light_jobs`, `light_mesh_invalidations`, and
   `direct_sky_changed` fields.

3. Provenance-only publication:

   `cargo test -p bedrock-client world_stream::tests::light_scheduler::provenance_only --locked -- --nocapture`

   RED: sampled light pointer identity changed under a direct-sky-only
   completion, which also made the held mesh output stale.

4. Changed-value accounting:

   `cargo test -p bedrock-client world_stream::tests::light_scheduler::changed_light_levels_dirty_a_renderable_mesh_generation --locked -- --nocapture`

   RED: the exact accepted/value-changing counters remained zero instead of
   incrementing once.

5. Fail-closed no-op acceptance:

   `cargo test -p bedrock-client world_stream::tests::light_scheduler::unchanged_completion_rejects_missing_prior_provenance_without_publication --locked -- --nocapture`

   RED: the first no-op implementation panicked when snapshotted provenance
   disappeared before acceptance. The final path rejects it as stale before
   changing ownership, revisions, durations, or accepted counters.

## GREEN evidence

- The exact `unchanged` filter passed both uniform and packed regressions.
- The `provenance_only` filter passed sampled mesh identity and affected
  neighbour stale-generation regressions.
- The real worker regression
  `worker_distinguishes_provenance_only_output_from_sampled_light_changes`
  passed with unchanged nibbles, changed direct provenance, stable light
  pointer identity, and zero mesh invalidations.
- The changed-value regression passed with a new light pointer, all 27 halo
  dependants dirty, and exact accepted/value/invalidation counters.
- `light_outcome_counters_saturate` passed for all new counter families.
- `cargo test -p bedrock-client world_stream::tests::light_scheduler --locked`
  passed 33 focused tests with one existing release-only benchmark ignored.
- `cargo test -p bedrock-client --locked` passed all client unit and integration
  targets, with the existing release-only benchmark ignored.
- `cargo test -p world --locked` passed all unit, integration, and doc-test
  targets.
- `cargo clippy -p bedrock-client -p world --all-targets --locked -- -D warnings`
  passed.
- `cargo fmt --all -- --check` and `git diff --check` passed after the report was
  added.

## Correctness review

- Existing target revision, block generation, residence, and previous sampled
  light-generation checks still run before every outcome branch.
- Failed solves retain the existing fatal path. Stale completions, eviction,
  waiter removal/requeueing, single-in-flight removal, and bounded worker
  behavior remain covered by the unchanged scheduler suite.
- No-op publication preserves both stored pointers. A missing or mismatched
  prior direct-sky record rejects fail-closed instead of panicking.
- Provenance-only publication tags the new mask with the preserved sampled
  generation. Changed faces still pass through the existing bounded neighbour
  scheduling path and reject older in-flight neighbour solves.
- Value-changing publication is still generation-checked by
  `commit_if_generation`, changes the sampled pointer/generation, and dirties
  the same 27 mesh dependants as before.
- Accepted outcomes increment exactly one classification counter. Every new
  counter uses `saturating_add`.

## Self-review and concerns

No known Task 1 correctness concerns remain. The deliberate non-goals are still
open: Task 2 latency authority, any narrower changed-light dependency mask, and
the full release/live two-second publication gate.
