# Phase 4.2 Actor Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make remote players visible through bounded actor snapshots, time-based interpolation, classic Bedrock biped geometry, and usable `PlayerList` skin pixels.

**Architecture:** Normalize bounded skin data in `protocol`, retain it in the app roster, translate player actors into render-only sources, and publish an interpolated frame. Render all players in one compact custom `Opaque3d` draw backed by one instance buffer and one texture array.

**Tech Stack:** Rust 1.93.1, Bevy 0.18.1 custom render phases, WGSL, wgpu 27.

## Global Constraints

- Work only in the `actor-rendering` linked worktree and commit locally without push or merge.
- Add focused failing tests and observe RED before production changes.
- Do not add persona/Molang support, `StandardMaterial`, per-actor meshes, Mojang assets, or debug textures.
- Keep all actor, history, skin-byte, and GPU publication counts explicitly bounded.

---

### Task 1: Normalize classic roster skins

**Files:** Modify `crates/protocol/src/actor.rs`; test `crates/protocol/tests/actors.rs`.

**Interfaces:** `PlayerListEntry::Add` produces a `PlayerSkin` containing bounded pixels or an explicit `PlayerSkinUnavailable` reason.

- [x] Add a fixture assertion for valid 64x64 RGBA retention and explicit persona/malformed outcomes.
- [x] Run `cargo test -p protocol --test actors` and record the missing-field/behavior failure.
- [x] Implement exact-dimension/byte validation and the 64 MiB per-packet retained-byte limit.
- [x] Re-run the focused protocol test to GREEN.

### Task 2: Join player actors to roster skin state

**Files:** Modify `app/src/actor_store.rs`; test its unit module.

**Interfaces:** `ActorStore::render_players(excluded_runtime_id)` yields deterministic remote player actor/profile pairs carrying `(unique_id, spawn_revision)` lifetime identity without exposing local movement state, while roster skins remain within a cumulative retained-byte cap.

- [x] Add tests for roster skin retention and its cumulative cap, UUID joining, local-player exclusion, stable runtime-ID order, accepted-spawn revision identity, replacement, and reset/removal.
- [x] Run `cargo test -p bedrock-client actor_store` and record RED.
- [x] Implement the minimal iterator/profile changes.
- [x] Re-run the focused app tests to GREEN.

### Task 3: Build bounded interpolated render frames

**Files:** Create `crates/render/src/actor.rs`; modify `crates/render/src/lib.rs`; test `actor.rs`.

**Interfaces:** `ActorRenderScene::update(now_seconds, sources)` produces `ActorRenderFrame` with at most 128 instances, two poses per actor, 100 ms delayed interpolation, actor-lifetime identity, movement-event revisions, and deterministic skin layers.

- [x] Add tests for interpolation, shortest angles, same-lifetime republication, same-runtime replacement, remove/re-add generation, same-event teleport republication, consecutive teleport events, ordinary post-teleport movement, truncation, skin resampling, and the locally generated default.
- [x] Run `cargo test -p render actor::tests` and record RED.
- [x] Implement only the bounded scene/frame logic required by those tests.
- [x] Re-run the focused render tests to GREEN.

### Task 4: Render one instanced standard biped draw

**Files:** Create `crates/render/src/actor_render.rs`, `crates/render/src/actor.wgsl`; modify `crates/render/src/lib.rs`; test `actor_render.rs`.

**Interfaces:** `ActorRenderPlugin` extracts `ActorRenderFrame`; WGSL expands six fixed base-layer cuboids and selects the instance skin layer.

- [x] Add geometry/UV contract, shader parse, descriptor specialization, no-op-backend binding-layout, and plugin idempotence tests.
- [x] Run `cargo test -p render actor_render::tests` and record RED.
- [x] Implement the custom pipeline, bounded GPU uploads, bind group, queue, and draw command.
- [x] Re-run the focused renderer tests to GREEN.

### Task 5: Publish actor frames from the live app

**Files:** Modify `app/src/main.rs`, `app/src/world_stream.rs`, `plan.md`.

**Interfaces:** An Update system after world-stream application translates only stored player actors and roster skins into `ActorRenderScene`; `ActorRenderPlugin` consumes its current frame.

- [x] Add a focused mapping test proving actor render input consumes only the remote actor/profile and no camera or movement state.
- [x] Run the focused test and record RED.
- [x] Wire the scene resource/plugin/system and document only the landed Phase 4.2 slice.
- [x] Re-run focused tests, `cargo fmt --all -- --check`, and strict workspace Clippy.
- [x] Inspect `git diff --check`, commit locally, and report the hash and remaining gaps.
