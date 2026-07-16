# Dual-Stage Actor Interpolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Smooth remote players over three 20 Hz position ticks, interpolate adjacent tick poses per frame, preserve teleport/tick metadata, and cull actors before GPU upload.

**Architecture:** Protocol preserves wire truth, client-world owns received/previous/current actor poses, the app converts elapsed time into whole actor ticks plus a partial tick, and render consumes only adjacent poses and camera visibility. Packet smoothing and frame interpolation remain separate state machines.

**Tech Stack:** Rust 2024, Valentine Bedrock protocol types, Bevy 0.18, Cargo tests, repository devtool.

## Global Constraints

- Start from exact commit `1e507aa98203e64fa2bd0686352591e5f7f33aad` without rewriting history.
- Do not modify atmosphere or world-publication scheduler behavior.
- Ordinary remote players converge in exactly three 50 ms ticks using remaining-distance division.
- Teleports snap immediately; replacements and resets discard interpolation history.
- At most 128 visible actors reach the existing single instanced draw.

---

### Task 1: Preserve remote movement truth

**Files:**
- Modify: `crates/protocol/src/world.rs`
- Modify: `crates/protocol/src/actor.rs`
- Test: `crates/protocol/tests/world_packets.rs`
- Modify: `app/src/runtime/network/session.rs`
- Test: `app/src/runtime/network/session/tests.rs`

**Interfaces:**
- Produces: `MovePlayerEvent::{head_yaw,on_ground,teleported,source_tick}` and `ActorMoveEvent::source_tick: Option<i64>`.

- [ ] Add tests constructing teleport-mode `MovePlayerPacket` values and assert exact normalized and foreign-converted fields.
- [ ] Run the focused protocol and app tests and record their missing-field failures.
- [ ] Add the four normalized fields and map them without coercion; absolute/delta actor moves use `source_tick: None`.
- [ ] Run the focused tests and all protocol tests to green.

### Task 2: Add deterministic three-tick client-world poses

**Files:**
- Modify: `crates/client-world/src/actor_store.rs`
- Modify: `crates/client-world/src/actor_store/lifecycle.rs`
- Modify: `crates/client-world/src/actor_store/query.rs`
- Modify: `crates/client-world/src/stream/publication.rs`
- Test: `crates/client-world/src/actor_store/tests.rs`

**Interfaces:**
- Produces: public `ActorPose`, `ActorSnapshot::{previous_pose,current_pose}`, and `WorldStream::advance_actor_interpolation_ticks(u32)`.
- Consumes: normalized `ActorMoveEvent` from Task 1.

- [ ] Add tests for `0 -> 9` yielding `3,6,9`, retargeting from `3` to `12`, two packets before one tick yielding `4`, immediate teleport to `100`, and replacement/reset state initialization.
- [ ] Run each focused test and record behavior failures before store edits.
- [ ] Add received/previous/current poses and a three-tick player countdown; retain immediate non-player movement.
- [ ] Implement bounded multi-tick advancement and lifecycle clearing.
- [ ] Run client-world tests to green.

### Task 3: Replace packet-time render history with partial-tick sampling and culling

**Files:**
- Modify: `crates/render/src/actor.rs`
- Modify: `crates/render/src/lib.rs`
- Test: `crates/render/src/actor.rs`
- Modify: `app/src/runtime/network.rs`
- Test: `app/src/tests/core.rs`

**Interfaces:**
- Produces: `ActorCullView`, `ActorRenderSource::{previous_pose,current_pose}`, and `ActorRenderScene::update(partial_tick, view, sources)`.
- Consumes: adjacent client-world poses from Task 2 and a camera clip transform from the app.

- [ ] Add tests for midpoint position, same-source alpha changes, shortest angles, teleport-equal endpoints, frustum/distance rejection, edge inclusion, and post-cull truncation.
- [ ] Run render tests and record API/assertion failures before render edits.
- [ ] Remove timed pose tracks and the 100 ms delay; sample adjacent poses directly.
- [ ] Add conservative player-AABB clip tests and a 192-block distance cap before the 128-item truncation.
- [ ] Add a tested 50 ms app accumulator, advance client-world whole ticks, compute the camera clip matrix, and pass the remaining fraction to render.
- [ ] Run app and render tests to green.

### Task 4: Verify and commit

**Files:**
- Review all files changed since `1e507aa`.

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run focused protocol, client-world, render, and app tests.
- [ ] Run `cargo clippy -p protocol -p client-world -p render -p bedrock-client --all-targets -- -D warnings`.
- [ ] Run `cargo run -p devtool --locked -- verify-affected --base 1e507aa`.
- [ ] Run `git diff --check` and inspect `git diff --stat` plus the complete diff.
- [ ] Commit the reviewed tranche locally without pushing or merging.
