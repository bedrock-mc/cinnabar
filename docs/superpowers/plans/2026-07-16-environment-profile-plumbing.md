# Environment Profile Plumbing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Compile and route authoritative biome visual and fog metadata for the default, Nether, and End environments without guessing native lighting or celestial behavior.

**Architecture:** Extend the separately hashed atmosphere asset envelope with bounded profile tables, expose exact fog distance resolution in `assets`, sample the camera biome and committed radius from `WorldStream`, and select/apply the route in the app after server clock/weather derivation. Keep unknown/custom inputs fail-safe and retain the current provisional lighting shader unchanged.

**Tech Stack:** Rust 2024, serde/serde_json, Bevy ECS resources and systems, the existing `MCBEATM` binary envelope, Cargo tests, Clippy, rustfmt, and `devtool verify-affected`.

## Global Constraints

- Start from exact `phase2-textures` commit `1e507aa` and preserve history.
- Do not commit Mojang bytes, generated runtime blobs, screenshots, or local cache paths.
- Do not modify actor, interpolation, scheduler, runtime graphics override, or celestial shader behavior.
- Do not invent native level `0..15` lighting RGB; keep current lighting floors explicit and provisional.
- Every production behavior requires a focused failing test observed before implementation.

---

### Task 1: Bounded environment profile compilation and envelope

**Files:**
- Modify: `crates/assets/src/atmosphere.rs`
- Modify: `crates/assets/src/lib.rs`
- Modify: `crates/asset-compiler/src/atmosphere.rs`
- Modify: `crates/asset-compiler/tests/atmosphere.rs`
- Modify: atmosphere synthetic constructors in `app/tests/assets.rs`, `crates/render/src/atmosphere_render.rs`, and `crates/asset-compiler/src/bin/assetc.rs`

**Interfaces:**
- Produces: `BiomeVisualProfile`, `FogProfile`, `FogDistance`, `FogMedium`, `FogDistanceMode`, lookup accessors on `RuntimeAtmosphereAssets`.
- Consumes: the already pinned resource-pack directory and source manifest.

- [ ] Add a synthetic compiler test containing plains/default, hell, and the End plus their fog files; assert exact identifiers, End RGB `0x000000`, default render-relative air `.92..1.0`, Nether fixed air `10..96`, and End `#0B080C`.
- [ ] Run `cargo test -p asset-compiler --test atmosphere environment_profiles` and confirm it fails because the compiled profile API is absent.
- [ ] Add the bounded serde inputs, validation, deterministic sorting, and compiled profile types.
- [ ] Run the focused compiler test and confirm it passes.
- [ ] Add an encode/decode round-trip test asserting every profile field survives and malformed counts/references fail closed.
- [ ] Run the round-trip test and confirm it fails against the v1 envelope.
- [ ] Extend the atmosphere envelope/version with a canonical bounded environment section and runtime lookup accessors; update synthetic constructors and provenance expectations.
- [ ] Run `cargo test -p assets -p asset-compiler atmosphere` and confirm it passes.

### Task 2: Exact fog distance resolver and frame application

**Files:**
- Modify: `crates/assets/src/atmosphere.rs`
- Modify: `crates/render/src/atmosphere.rs`
- Modify: `crates/render/tests/atmosphere.rs`

**Interfaces:**
- Produces: `FogDistance::resolve(render_distance_blocks) -> ResolvedFog` and `AtmosphereFrame` methods that apply resolved fog and an optional exact sky RGB while preserving time/weather fields.
- Consumes: compiled fog records from Task 1.

- [ ] Add focused tests asserting fixed `10..96` is unchanged, render-relative `.92..1.0` at 256 blocks becomes `235.52..256`, invalid render distances fail safe, and RGB bytes are preserved.
- [ ] Run the focused resolver tests and confirm they fail because the resolver is absent.
- [ ] Implement the minimum finite bounded resolver and rerun the tests to green.
- [ ] Add render tests asserting exact sky/fog application changes only sky/fog fields and preserves sun direction, moon phase, day fraction, rain, thunder, and cloud offset.
- [ ] Run the render test and confirm it fails because profile application is absent.
- [ ] Implement standard sRGB-to-linear RGB application and rerun the render atmosphere suite to green.

### Task 3: Camera environment context and profile selection

**Files:**
- Modify: `crates/client-world/src/stream/polling.rs`
- Modify: `crates/client-world/src/stream/tests/cases_01.rs`
- Modify: `app/src/environment.rs`
- Modify: `app/src/runtime/world.rs`
- Modify: `app/src/app.rs`
- Modify: relevant app environment/core tests

**Interfaces:**
- Produces: fail-closed `WorldStream::camera_biome_id`, effective render-distance accessor, `EnvironmentContext`, and deterministic profile selection by biome then dimension fallback.
- Consumes: runtime atmosphere profile lookups, camera transform, current dimension, current medium, and unchanged `WorldClock`/`WeatherState`.

- [ ] Add a stream test that commits a known biome column and asserts the camera returns its palette-native biome ID, while missing/nonfinite positions return `None`.
- [ ] Run the focused stream test and confirm it fails because the accessor is absent.
- [ ] Implement the minimum store lookup and effective radius accessor; rerun to green.
- [ ] Add app tests for camera-biome precedence, Overworld/Nether/End fallback, unknown fallback, fixed/render-relative fog selection, exact End sky, and unchanged clock/weather values.
- [ ] Run the focused app tests and confirm they fail because context/profile routing is absent.
- [ ] Implement `EnvironmentContext`, update it after `drive_world_stream`, and apply selected profile data after existing frame derivation. Carry `lighting_identifier` as `ProvisionalLightingRoute` without changing WGSL.
- [ ] Run the app environment and ordering tests to green.

### Task 4: Verification, review, and local commit

**Files:**
- Review all changed files; no new production scope.

**Interfaces:**
- Produces: one locally committed, unpushed implementation SHA.

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run focused assets/compiler/render/client-world/app tests.
- [ ] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] Run `cargo run -p devtool -- verify-affected`.
- [ ] Run one independent diff review, fix every Critical or Important finding, and rerun affected verification if production behavior changes.
- [ ] Run `git diff --check` and confirm no Mojang/local artifacts are tracked.
- [ ] Commit intentionally with an environment-profile message; do not push or merge.

