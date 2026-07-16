# Native Legacy Cloud Parity Implementation Plan

> **Execution:** Use `superpowers:subagent-driven-development`, strict red-green-refactor TDD,
> one implementation task at a time, an independent review after each accepted tranche, and
> immediate pushes to `bedrock-mc/cinnabar:phase2-textures` without rewriting history.

**Goal:** Replace the rejected preview-source, fixed-grid, opaque cloud behavior with the
matching Bedrock 1.26.33.1 legacy cloud path while retaining compact immutable geometry.

**Design authority:** `docs/superpowers/specs/2026-07-16-native-cloud-parity-design.md`

## Global constraints

- Never commit Mojang assets, compiled local asset carriers, screenshots, or native binaries.
- Preserve the eight-byte packed cloud quad and one-draw/one-resource-family architecture.
- Do not guess native `mesh_size` or `grid_size` world semantics; calibrate them.
- No production behavior is added before a focused failing test demonstrates the gap.
- Mark `plan.md` complete only from recorded automated and live evidence.

### Task 1: Exact local cloud override and provenance

**Likely files:** `crates/assets/src/atmosphere.rs`, `crates/assets/src/bin/assetc.rs`,
`crates/assets/src/lib.rs`, `crates/assets/tests/atmosphere.rs`, `Makefile`,
`app/tests/assets.rs`.

- [ ] Add failing tests for an explicit cloud override: exact accepted SHA/dimensions, wrong
  hash, wrong dimensions, oversized input, missing path, and unchanged pinned sun/moon inputs.
- [ ] Add an options-based compiler interface and `assetc atmosphere --clouds-override PATH`.
  The override replaces only the cloud bytes, keeps the canonical logical source path, records
  independent hashes, and fails closed.
- [ ] Thread a portable `CINNABAR_CLOUDS_PNG` Make variable into the atmosphere command without
  introducing a checked-in local path. Keep the existing no-override fixture path deterministic.
- [ ] Verify focused assets tests, asset CLI tests, Make contract tests, formatting, and strict
  Clippy. Independently review, commit, and push.

### Task 2: Native cloud configuration model

**Likely files:** `crates/render/src/cloud_mesh.rs`, `crates/render/src/cloud_render.rs`,
`crates/render/tests/cloud_mesh.rs`, `crates/render/tests/cloud_render.rs`.

- [ ] Add failing tests for Low/Medium/High/Ultra exact grid, mesh, distance, distance-control,
  and lighting records; default to High.
- [ ] Implement bounded `CloudQuality` and `CloudRenderConfig` without changing current coverage
  math or claiming unproven world-space semantics.
- [ ] Add a native calibration harness/report contract that records matching views and derived
  coverage semantics. It must fail if asked to publish an uncalibrated mapping.
- [ ] Verify focused render tests, formatting, strict Clippy, review, commit, and push.

### Task 3: Transparent depth-aware legacy material

**Likely files:** `crates/render/src/cloud_render.rs`, `crates/render/src/cloud.wgsl`,
`crates/render/tests/cloud_render.rs`, `crates/render/tests/atmosphere.rs`.

- [ ] Add failing tests proving the legacy cloud item uses the transparent world phase, alpha
  blend, deterministic transparent ordering, reversed-Z depth testing, and no color-pass depth
  writes. Retain Metal-compatible binding visibility checks.
- [ ] Move queue/draw integration from opaque to transparent without adding resources or draws.
- [ ] Emit bounded cloud alpha and retain terrain depth occlusion, fog, identity caching, and
  zero-record behavior.
- [ ] Verify render and client suites, WGSL/Naga validation, formatting, strict Clippy, review,
  commit, and push.

### Task 4: Directional lighting and exact weather response

**Likely files:** `crates/render/src/cloud.wgsl`, `crates/render/src/atmosphere.rs`,
`app/src/environment.rs`, focused shader/environment tests.

- [ ] Add failing shader/CPU tests for known sun directions and exact rain `[191,191,191]` /
  thunder `[30,30,30]` colours at `0.95` contribution.
- [ ] Replace fixed face multipliers with normal/directional diffuse lighting from the existing
  atmosphere frame, preserving bounded clear daylight, fog, and alpha ordering.
- [ ] Verify shader semantics on WGSL/Naga and Metal stage validation, full focused suites,
  formatting, strict Clippy, review, commit, and push.

### Task 5: Native coverage calibration and config-driven geometry

- [ ] Capture matching native Low/Medium/High/Ultra reference views and record the exact
  relationship between `grid_size`, `cloud_mesh_size`, distance scale, and world coverage.
- [ ] Add failing origin/count/distance tests from that evidence, including negative coordinates,
  period crossings, and edge-fog coverage.
- [ ] Replace fixed 3x3 origins with the calibrated config-driven mapping while retaining one
  instanced draw and immutable geometry.
- [ ] Verify focused/full render and client suites, review, commit, and push.

### Task 6: Release live parity acceptance

- [ ] Rebuild ignored assets using the exact local cloud override and record only hashes and
  source version, never the path or bytes.
- [ ] Run BDS/core/release client with FIFO first. Capture temporary GDI views below, above,
  within, grazing, period crossings, and negative coordinates; compare against matching native.
- [ ] Prove no opaque slabs/edge pop/black rectangles, one steady draw, zero steady uploads,
  exact asset/shader identities, and correct weather/day-night response.
- [ ] Record actual CPU, RSS, frame-time, and teleport/remesh values. Do not close Phase 2.7 if
  any existing motion-artifact or two-second publication gate remains red.
- [ ] Update `plan.md`, independently review the evidence, commit, and push.
