# World Publication Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate unchanged-light remesh churn, expose exact publication-stage latency, and prove the full release radius-16 path meets the two-second gate.

**Architecture:** Preserve the sparse solver and generation checks. Add explicit no-op/provenance-only completion outcomes, keep sampled-value identities stable when output is equal, remove provenance from mesh-output identity, and join world-stream queue metrics with existing render/presentation evidence.

**Tech Stack:** Rust, Bevy, Rayon, sparse light volumes, deterministic release benchmarks, PowerShell/Bash acceptance scripts.

## Global Constraints

- Do not weaken generation, residence, block-identity, fatal-error, eviction, or bounded-queue checks.
- Do not change culling, cave connectivity, presentation mode, draw mode, light values, AO, or GPU shader output in this tranche.
- No per-subchunk logging; use cumulative counters and bounded max/duration gauges.
- Preserve palette-native block storage and sparse uniform/packed light storage.
- Write each behavioral regression test first and record its expected red failure.

---

### Task 1: Exact light-completion outcome and no-op publication

**Files:**
- Modify: `app/src/world_stream.rs`
- Test: the existing `light_scheduler` module in `app/src/world_stream.rs`

**Interfaces:**
- `SolvedLightJob` exposes `light_levels_changed: bool` and `direct_sky_changed: bool` without dead-code allowances.
- `WorldStreamStats` adds `accepted_light_jobs`, `noop_light_jobs`, `value_changed_light_jobs`, `provenance_only_light_jobs`, and `light_mesh_invalidations` as saturating `u64` counters.
- `MeshLightSlot` retains only identities that can change `MeshLightSampler` output: key, block generation, light-value generation, and `Arc<SubChunkLight>`.

- [x] **Step 1: Write failing no-op completion tests**

Prepare current uniform and packed light/direct-sky states, force a new current solve revision with equal output, and accept its completion. Assert stored light/direct pointers are `Arc::ptr_eq` to the originals, ownership advances to the current block generation while preserving the sampled light generation, no mesh revision/pending job changes, no in-flight mesh becomes stale, waiters are released, and the no-op counters increment exactly once.

- [x] **Step 2: Run and record red**

Run `cargo test -p bedrock-client world_stream::tests::light_scheduler::unchanged --locked -- --nocapture`. Expected: failure because every completion currently replaces identities and invalidates 27 mesh dependants.

- [x] **Step 3: Compute exact worker equality**

Compare the replacement nibbles with the prior `SubChunkLight` using `light_levels_equal` and compare the complete `DirectSkyMask` value with the prior mask. Carry both booleans in `SolvedLightJob`; keep `changed_faces` unchanged for neighbour propagation.

- [x] **Step 4: Implement the generation-safe no-op path**

After all existing freshness checks and successful solving, branch before `commit_if_generation`. If both values are unchanged, preserve both stored `Arc`s, update `LightOwnership.block_generation` while retaining its existing `light_revision`, clear only the current solver revision, update counters/durations, wake waiters, and process the all-false/unchanged face summary without calling `mark_light_mesh_dependents`.

- [x] **Step 5: Implement and test provenance-only behavior**

For equal nibbles with a changed direct-sky mask, preserve the old `Arc<SubChunkLight>` and its generation, replace only `StoredDirectSky.mask` while tagging it with that preserved generation, advance ownership to the current block generation, and clear only the current solver revision. This is permitted only after the existing freshness checks and single-in-flight-target removal. Propagate changed faces so affected neighbour revisions reject any older in-flight solve, and perform zero mesh invalidation. Remove direct-sky `Arc` identity from `MeshLightSlot` and its currentness check after proving snapshot creation still requires `light_is_current`. Assert a mesh using unchanged nibble `Arc` data is not rejected solely by provenance replacement and a subsequently prepared light job observes the new mask.

- [x] **Step 6: Preserve changed-value behavior**

Add a regression where one block/sky nibble changes. Assert stored value identity changes, the source and all existing halo dependants are dirtied exactly as before, stale mesh work requeues losslessly, and counters distinguish this path from no-op/provenance-only.

- [x] **Step 7: Verify and commit**

Run `cargo test -p bedrock-client --locked`, `cargo test -p world --locked`, `cargo clippy -p bedrock-client -p world --all-targets --locked -- -D warnings`, and `cargo fmt --all -- --check`. Commit with `perf: skip unchanged light remeshes`.

### Task 2: Publication-stage latency authority

**Files:**
- Modify: `app/src/world_stream.rs`
- Modify: `app/src/main.rs`
- Modify: `scripts/acceptance.ps1`
- Modify: `scripts/acceptance.sh`
- Modify: `scripts/tests/acceptance.Tests.ps1`
- Modify: `scripts/tests/acceptance_test.sh`

**Interfaces:**
- Decode/light/mesh jobs carry their enqueue `Instant` through completion and expose bounded maximum queue wait separately from worker duration.
- Acceptance output records accepted/no-op/value/provenance light counts, mesh invalidations, stale jobs, pending/in-flight gauges, max queue waits, worker maxima, upload queue/bytes, draw mode, present provenance, and build profile in one periodic snapshot.

- [ ] **Step 1: Write failing metric and script-contract tests**

Use fixed test instants to prove queue wait excludes worker duration, counters saturate, snapshots remain deterministic, and both scripts require every exact field. Assert debug and release, Direct and MDI, and FIFO and Immediate cannot be conflated in one result row.

- [ ] **Step 2: Run and record red**

Run the focused client stats tests plus `Invoke-Pester scripts/tests/acceptance.Tests.ps1` and `bash scripts/tests/acceptance_test.sh`. Expected: failures for missing stage fields.

- [ ] **Step 3: Thread enqueue times through bounded jobs**

Add `queued_at: Instant` to decode, light, and mesh jobs at their existing admission point. At worker start/dispatch, calculate queue wait with saturating duration semantics; retain the existing worker timers. Update only cumulative maxima/counters in `WorldStreamStats`.

- [ ] **Step 4: Emit one coherent periodic snapshot**

Join the world-stream values with the render/upload and visibility/presentation provenance already captured for the same acceptance run. Do not print one line per completion. Shell parsers must fail closed on absent, duplicate, malformed, or profile/mode-mismatched fields.

- [ ] **Step 5: Verify and commit**

Run `cargo test -p bedrock-client --locked`, both acceptance script suites, strict workspace Clippy, and rustfmt. Commit with `feat: attribute world publication latency`.

### Task 3: Full release radius-16 benchmark and live gate

**Files:**
- Modify: `app/src/world_stream.rs` test module or create `app/tests/world_publication.rs`
- Modify: `plan.md`
- Modify: the Phase 2 acceptance report selected by the scripts

**Interfaces:**
- The ignored release benchmark covers 33×33×24 subchunks through accepted light, current mesh completion, render-queue extraction, and upload acknowledgement.
- The live result includes exact commit/profile/backend/adapter/driver/draw/present identities and stage counters.

- [ ] **Step 1: Write the failing end-to-end benchmark**

Start from the existing 26,136 known-air scheduler fixture, drain light jobs, dispatch/accept all required meshes, drain `WorldMeshChange`, enqueue render changes, and acknowledge bounded uploads. Assert every resident renderable key reaches a current published generation with zero pending/in-flight jobs and no duplicate publication.

- [ ] **Step 2: Run the release benchmark before optimization**

Run the exact ignored test with `cargo test -p bedrock-client --release --locked full_view_publication -- --ignored --nocapture`. Record total and per-stage time plus counts; expected pre-fix evidence is redundant invalidation/publication or a miss of the two-second complete-path gate.

- [ ] **Step 3: Run after Tasks 1–2 and require the binding gate**

Repeat the identical command. Require 26,136 current subchunks, zero stale/pending/in-flight work at completion, no no-op-generated mesh invalidations, and total complete-path time at most two seconds on the reference class.

- [ ] **Step 4: Run live BDS join and teleport acceptance**

Use the stable approved BDS/core/client paths and release FIFO default. Capture initial join and a teleport separately; an Immediate run is a separately labelled A/B. Require full view publication within two seconds, no visible stalls, combined RSS at most 650 MB, and settled CPU at most 15%.

- [ ] **Step 5: Update plan truthfully, review, commit, and push**

Record actual values and close only the publication blocker if both deterministic and live gates pass. Run one focused independent review, fix Critical/Important findings, preserve history, then push to `bedrock-mc/cinnabar:phase2-textures`.
