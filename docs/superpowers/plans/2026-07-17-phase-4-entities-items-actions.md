# Phase 4 Entities, Items, and Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish Phase 4.3 and Phase 4.5 by compiling bounded Bedrock entity
animation/controller data and canonical item visuals, evaluating shared runtime
rigs, rendering skinned actors plus held and dropped items, and presenting
first-person held-item actions from the `completion-phase5-authority`
checkpoint before closing the
binding Phase 4.4 LBSG witness.

**Architecture:** Extend the existing `MCBEENT3` envelope as version 4 rather
than creating a parallel asset format. Offline compilation owns JSON and Molang
complexity; runtime assets expose immutable, indexed rigs and item visuals.
`protocol` reduces vendored packets to bounded types, `client-world` stamps
them with session/dimension/actor-lifetime/FIFO identity and advances completed
20 Hz poses, and `render` consumes shared geometry, item, and compact dual-pose
bone arenas. Third-person and first-person presentations share item identity,
assets, rig selection, and action phase but keep separate transforms and render
passes. The immutable `completion-phase5-authority` checkpoint is the sole
authority for the local selected stack and action confirmation or rejection.

**Tech Stack:** Rust 1.93.1 edition 2024, serde/serde_json, SHA-256 asset
identity, Bevy 0.18.1 render extraction, WGPU 27/WGSL, protocol 1001 Bedrock
1.26.30 packets, PowerShell 5.1 acceptance tooling, local BDS, authenticated
`play.lbsg.net:19132`, and matched native Bedrock captures.

## Global Constraints

- Work only in a fresh `completion-phase4` worktree based on the current
  reviewed `completion-integration`/full-client-design plan tip. Before work,
  require both `b2940086fa2bc8d7089ae065da575086557cbd67` and canonical code
  parent `d8e469979a0ec6c4798bb2ffc1dc45d3a9891eeb` to be ancestors with
  `git merge-base --is-ancestor`; do not merge archival Phase 4 branches or
  worktrees. Before Task 1, pin the clean reviewed fork with
  `git branch completion-phase4-base HEAD`; never move that base ref.
- This lane owns entity/item leaf modules, actor/item state, Phase 4 render
  resources, and Phase 4 acceptance helpers. The integration lane owns root
  `Cargo.toml`, `Cargo.lock`, `plan.md`, shared `lib.rs` re-exports, packet
  dispatch in `protocol::world`, `assetc` command registration, app module and
  schedule assembly, architecture policy, acceptance entry-point registration,
  and the completion ledger. Each producer commit supplies an exact handoff;
  run its public GREEN gate after the integration owner applies that handoff.
- Preserve `ENTITY_BLOB_MAGIC == b"MCBEENT3"`; bump `ENTITY_BLOB_VERSION` from
  3 to 4 and reject other versions. Keep the existing 80-byte hashed envelope,
  extend the validated payload and header counts, and never check generated
  vanilla payloads into Git.
- Asset compilation is deterministic over normalized forward-slash paths,
  sorted symbols, and canonical JSON. Every source, expression, controller,
  clip, keyframe, item definition, texture, retained string, queue, history,
  palette, and GPU arena has an item and byte ceiling.
- Compile only the reviewed Molang subset listed in Task 3. An unsupported
  expression in an optional animation/render route records an attributed
  static fallback. A malformed or unsupported required geometry, material,
  texture, rig, controller, or animation reference rejects that rig.
- Keep geometry, texture, material, item mesh, and rig data shared. Per-actor
  state may contain handles, action state, and compact bone transforms; it may
  not own a mesh, material, texture, bind group, or unbounded string.
- Maintain two completed tick poses and interpolate only those adjacent poses
  during rendering. Reset histories on actor replacement, session or dimension
  replacement, incompatible metadata, and teleport.
- Normalize and retain AddPlayer held items, ItemRegistry, MobEquipment,
  Animate, the reviewed AnimateEntity subset, and AddItemEntity. Preserve
  session, dimension, actor lifetime, source-tick provenance, and ingress FIFO.
- Normalize MobEquipment exactly once as `WorldEvent::Equipment`; after
  StartGame establishes the local runtime ID, app integration routes the same
  sequenced event exactly once: a matching local actor goes only to the Phase
  5 selected-hotbar authority, while every nonlocal actor goes only to the
  Phase 4 actor-equipment store.
- Local viewmodel state is a consumer of `completion-phase5-authority`'s selected
  stack. Phase 4 must not add a second inventory, selected-slot, transaction,
  reach, target, or combat-authority store.
- Missing assets choose an explicit `EmptyHand`, `Missing`, static-fallback, or
  no-draw route and increment an attributed diagnostic. NaN, infinity, stale
  identities, out-of-range indices, and arena overflow are rejected before GPU
  upload.
- Evidence must traverse the normal packet, store, pose, extraction, draw, and
  presented-frame paths. Test-only entities may enter through bounded fixture
  packets, never direct render injection.
- Do not commit Mojang payloads, credentials, tokens, private player data,
  unsanitized packet captures, generated `.mcbeent` blobs, screenshots, or
  per-user settings. Commit only source, tests, sanitized manifests, and
  adjudication records.

## File and Interface Map

| Layer | Existing files to modify | New focused files |
|---|---|---|
| Asset carrier | `crates/assets/src/entity.rs`, `crates/assets/tests/entity.rs` | `crates/assets/src/item.rs`, `crates/assets/tests/item.rs`; integration exports from `crates/assets/src/lib.rs` |
| Offline compiler | `crates/asset-compiler/src/entity.rs`, `crates/asset-compiler/tests/entity.rs` | `crates/asset-compiler/src/entity/animation.rs`, `crates/asset-compiler/src/entity/molang.rs`, `crates/asset-compiler/src/entity/item.rs`, `crates/asset-compiler/tests/entity_animation.rs`, `crates/asset-compiler/tests/item_visuals.rs`; integration owns `lib.rs` and `assetc` dispatch |
| Packet normalization | `crates/protocol/src/actor.rs`, `crates/protocol/tests/actors.rs`, `crates/protocol/tests/world_packets.rs` | `crates/protocol/src/item.rs`, `crates/protocol/tests/items_actions.rs`; integration owns `lib.rs` exports and `world.rs` packet dispatch |
| Runtime world | `crates/client-world/src/actor_store.rs`, `crates/client-world/src/actor_store/lifecycle.rs`, `crates/client-world/src/actor_store/query.rs`, `crates/client-world/src/actor_store/tests.rs`, `crates/client-world/src/stream.rs`, `crates/client-world/src/stream/construction.rs`, `crates/client-world/src/stream/sequencing.rs` | `crates/client-world/src/actor_animation.rs`, `crates/client-world/src/item.rs`, `crates/client-world/src/action.rs`, `crates/client-world/src/dropped_item.rs`, `crates/client-world/tests/entity_runtime.rs`, `crates/client-world/tests/item_actions.rs`; integration owns `crates/client-world/src/lib.rs` exports |
| GPU presentation | `crates/render/src/actor.rs`, `crates/render/src/actor_render.rs`, `crates/render/src/actor.wgsl` | `crates/render/src/actor/rig.rs`, `crates/render/src/actor/gpu.rs`, `crates/render/src/item.rs`, `crates/render/src/item.wgsl`, `crates/render/src/viewmodel.rs`, `crates/render/src/viewmodel.wgsl`, `crates/render/tests/actor_rig.rs`, `crates/render/tests/item_presentation.rs`, `crates/render/tests/viewmodel.rs`; integration exports from render `lib.rs` |
| App publication | `app/src/asset_startup.rs`, `app/src/runtime/network.rs`, `app/src/runtime/network/session.rs`, `app/src/runtime/network/session/tests.rs`, `app/src/args.rs`, `app/src/metrics.rs` | `app/src/presentation.rs`, `app/src/presentation/actors.rs`, `app/src/presentation/viewmodel.rs`, `app/src/acceptance/actor_witness.rs`, `app/src/acceptance/item_witness.rs`, `app/src/tests/phase4_presentation.rs`; integration owns `app/src/lib.rs`, `app.rs`, and schedule registration |
| Acceptance | `scripts/acceptance.ps1`, `scripts/acceptance/Load.ps1`, `scripts/acceptance/Markers.ps1`, `scripts/acceptance/Metrics.ps1`, `scripts/acceptance/Orchestrator.ps1`, `scripts/tests/acceptance.Tests.ps1` | `scripts/acceptance/Actors.ps1`, `scripts/tests/acceptance.Actors.Tests.ps1`, `docs/evidence/phase-4/README.md`, `docs/evidence/phase-4/run-manifest.schema.json` |

The dependency checkpoints are binding:

1. **Tranche A:** publish `assets::ItemStackIdentity`/`ItemVisualRoute`, then
   compile clips, Molang/controllers, rig bindings, and canonical item visuals.
2. **Tranche B:** freeze entity-carrier, item-identity, and actor-pose
   interfaces; then land runtime rigs, skeletal GPU presentation, packet/world
   equipment/actions, and third-person held-item actions.
3. **Actor-facing 4.5 checkpoint:** remote held items/actions pass deterministic
   evidence. Only then may Phase 4.4's LBSG run become binding.
4. **Tranche C:** after the immutable `completion-phase5-authority` checkpoint
   publishes the exact selected-stack and action timeline contract from Phase
   5 Tasks 10-13, land AddItemEntity lifecycle/dropped rendering,
   the first-person viewmodel, and local action presentation.

---

### Task 1: Publish the canonical item identity and visual-route interface

**Files:**
- Create: `crates/assets/src/item.rs`
- Create: `crates/assets/tests/item.rs`
- Request integration edit: export the reviewed module from `crates/assets/src/lib.rs`

**Interfaces:**
- Produces the tranche-A interface consumed by Phase 5 inventory, hotbar/UI,
  remote equipment, dropped items, viewmodel, and outbound stack code.
- Does not own the live network-ID registry or selected slot.

- [ ] **Step 1: Write the failing public-contract tests**

Add tests that construct and compare these exact public types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemStackIdentity {
    pub network_id: i32,
    pub metadata: u32,
    pub stack_network_id: i32,
    pub count: u16,
    pub nbt_digest: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemVisualId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockVisualId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemVisualRoute {
    Compiled(ItemVisualId),
    BlockItem(BlockVisualId),
    EmptyHand,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemIconRef {
    pub asset_identity: [u8; 32],
    pub texture_page: u16,
    pub uv: [u16; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemActionPhase {
    Idle,
    Windup { elapsed_ticks: u16 },
    Active { elapsed_ticks: u16 },
    Recover { elapsed_ticks: u16 },
    UseHeld { elapsed_ticks: u16, duration_ticks: u16 },
    Cancelled,
}
```

Also test `ItemStackIdentity::empty()` and validation: count `0` canonicalizes
the complete identity to empty, nonempty stacks reject negative `network_id`,
all `u32` metadata values remain lossless, and an absent stack-network ID is
represented by `-1` rather than `Option`.

- [ ] **Step 2: Run RED**

```powershell
cargo test -p assets --locked --test item -- --nocapture
```

Expected: compile failure because `assets::ItemStackIdentity`,
`ItemVisualId`, `BlockVisualId`, `ItemVisualRoute`, `ItemIconRef`, and
`ItemActionPhase` do not exist.

- [ ] **Step 3: Implement and export the immutable identity types**

Keep NBT out of render-facing state: the normalizer hashes the validated NBT
bytes once, and downstream code compares the 32-byte digest. Implement
`ItemStackIdentity::validate`, `is_empty`, and `empty`; do not add identifier
strings or heap storage to this type. `ItemIconRef` is the only icon contract
consumed by hotbar/inventory UI; UI code may not compile or upload a second item
catalog. `ItemActionPhase` is the shared remote/local presentation phase, while
timeline authority remains with the owning world or interaction store.

- [ ] **Step 4: Run GREEN and the interface gate**

```powershell
cargo test -p assets --locked --test item -- --nocapture
cargo clippy -p assets --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: leaf tests pass before export; after the integration owner applies the
exact `mod item`/`pub use item::{...}` handoff, the public tests and all commands
pass.

- [ ] **Step 5: Commit**

```powershell
git add crates/assets/src/item.rs crates/assets/tests/item.rs
git commit -m "feat: define canonical item visual identity"
git branch completion-phase4-item-interface HEAD
```

Expected: the branch points at the producer commit and is immutable before any
Phase 5 consumer is merged.

### Task 2: Extend the entity carrier for clips, controllers, rigs, and items

**Files:**
- Modify: `crates/assets/src/entity.rs`
- Modify: `crates/assets/tests/entity.rs`
- Modify: `crates/assets/tests/item.rs`
- Request integration edit: extend reviewed exports in `crates/assets/src/lib.rs`

**Interfaces:**
- Consumes the Task 1 IDs.
- Produces deterministic version-4 carrier records used by the compiler and
  runtime resolver.

- [ ] **Step 1: Add failing version, bound, hash, and round-trip tests**

Cover exact-limit acceptance and limit-plus-one rejection for:

```rust
pub const MAX_ENTITY_ANIMATION_CLIPS: usize = 4_096;
pub const MAX_ENTITY_ANIMATION_CHANNELS: usize = 65_536;
pub const MAX_ENTITY_ANIMATION_KEYFRAMES: usize = 524_288;
pub const MAX_ENTITY_CONTROLLERS: usize = 2_048;
pub const MAX_ENTITY_CONTROLLER_STATES: usize = 16_384;
pub const MAX_ENTITY_CONTROLLER_TRANSITIONS: usize = 32_768;
pub const MAX_MOLANG_EXPRESSIONS: usize = 65_536;
pub const MAX_MOLANG_OPS_PER_EXPRESSION: usize = 256;
pub const MAX_MOLANG_STACK_DEPTH: u8 = 32;
pub const MAX_MOLANG_COLLECTION_ITEMS: usize = 32;
pub const MAX_ENTITY_RIG_BINDINGS: usize = 8_192;
pub const MAX_ITEM_VISUALS: usize = 16_384;
pub const MAX_ITEM_VISUAL_ALIASES: usize = 65_536;
pub const MAX_ITEM_IDENTIFIER_BYTES: usize = 256;
```

Tests must prove version 3 and version 5 are rejected, payload/header count
mismatches fail before allocation, re-encoding is byte-identical, all indices
are in range, all scalars are finite, and envelope SHA-256 covers the extended
payload.

- [ ] **Step 2: Run RED**

```powershell
cargo test -p assets --locked --test entity carrier_v4 -- --nocapture
cargo test -p assets --locked --test item carrier -- --nocapture
```

Expected: failure because version 4 and the extended records are absent.

- [ ] **Step 3: Add typed carrier records**

Use dense indices, not repeated strings, for the runtime records:

```rust
pub struct EntityAnimationClip {
    pub symbol: u32,
    pub length_seconds: EntityGeometryScalar,
    pub loop_mode: EntityAnimationLoop,
    pub first_channel: u32,
    pub channel_count: u32,
    pub source: u32,
}

pub struct EntityAnimationChannel {
    pub bone: u32,
    pub property: EntityAnimationProperty,
    pub first_keyframe: u32,
    pub keyframe_count: u32,
}

pub struct EntityAnimationKeyframe {
    pub time_seconds: EntityGeometryScalar,
    pub value: [EntityGeometryScalar; 3],
    pub interpolation: EntityAnimationInterpolation,
}

pub struct CompiledMolangExpression {
    pub first_op: u32,
    pub op_count: u16,
    pub max_stack: u8,
}

pub struct EntityRigBinding {
    pub entity_symbol: u32,
    pub geometry: u32,
    pub render_controller: u32,
    pub first_animation: u32,
    pub animation_count: u16,
    pub first_controller: u32,
    pub controller_count: u16,
    pub fallback: EntityRigFallback,
}
```

Add flattened controller/state/transition records, Molang ops, item visual
definitions, aliases, first-/third-person/drop display transforms, texture
source indices, and optional block-visual routes. Extend
`CompiledEntityAssets`, `RuntimeEntityAssets`, JSON carrier validation, startup
summary counts, and the 80-byte header's reserved count words. Keep all public
constructors validating before `Arc` conversion.

- [ ] **Step 4: Run GREEN**

```powershell
cargo test -p assets --locked --test entity -- --nocapture
cargo test -p assets --locked --test item -- --nocapture
cargo clippy -p assets --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all carrier tests pass with no warning or formatting diff.

- [ ] **Step 5: Commit**

```powershell
git add crates/assets/src/entity.rs crates/assets/src/item.rs crates/assets/tests/entity.rs crates/assets/tests/item.rs
git commit -m "feat: carry bounded entity rigs and item visuals"
```

### Task 3: Compile the reviewed Molang subset, clips, controllers, and item visuals

**Files:**
- Create: `crates/asset-compiler/src/entity/animation.rs`
- Create: `crates/asset-compiler/src/entity/molang.rs`
- Create: `crates/asset-compiler/src/entity/item.rs`
- Create: `crates/asset-compiler/tests/entity_animation.rs`
- Create: `crates/asset-compiler/tests/item_visuals.rs`
- Create: `crates/assets/data/block-item-routes-v1001.json`
- Modify: `tools/registrygen/main.go`, `tools/registrygen/main_test.go`
- Modify: `crates/asset-compiler/src/entity.rs`
- Modify: `crates/asset-compiler/src/entity/json.rs`
- Modify: `crates/asset-compiler/tests/entity.rs`
- Request integration edits: export the compiler leaf module from
  `crates/asset-compiler/src/lib.rs` and add the `entity-assets` match arm in
  `crates/asset-compiler/src/bin/assetc.rs`

**Interfaces:**
- Produces immutable rig bindings and `ItemVisualId` records without requiring
  inventory UI.
- Preserves source path, source SHA-256, symbol, inheritance, and every
  dependency/fallback decision in the existing compiler report.
- Consumes a generated reviewed block-item route table. `registrygen` derives
  it from the pinned Dragonfly `world.Items()` entries that also implement
  `world.Block`, resolves each exact item name/metadata default block
  name/properties against the canonical BREG1003 state set, and records the
  Dragonfly version/module sum plus BREG SHA-256. Duplicate, missing,
  ambiguous, noncanonical, or out-of-range routes fail generation. The entity
  compiler may not infer BlockItem routes from identifier equality or an
  optional pack sidecar.

- [ ] **Step 1: Write failing compiler fixtures**

Create synthetic packs covering a looping walk clip, nonlooping attack clip,
pre/post keyframes, a two-state controller, render-controller array selection,
required missing animation, optional unsupported expression, cyclic controller,
malformed keyframe, non-finite literal, item texture alias, block-item route,
empty hand, and missing item texture. Assert byte-identical output when input
directory enumeration order changes.

The accepted Molang grammar is exactly:

- finite numeric and boolean literals;
- parentheses; unary `-` and `!`; binary `+ - * / % < <= > >= == != && ||`;
- ternary `condition ? yes : no`;
- `math.abs`, `ceil`, `floor`, `round`, `sqrt`, `sin`, `cos`, `min`, `max`,
  `clamp`, and `lerp` with fixed arity;
- bounded `variable.*` and `temp.*` slots assigned at compile time;
- `query.anim_time`, `life_time`, `modified_move_speed`, `ground_speed`,
  `is_on_ground`, `is_moving`, `is_sprinting`, `is_sneaking`, `is_sleeping`,
  `body_y_rotation`, `head_y_rotation`, and `target_x_rotation`;
- render-controller collection selection with at most 32 compiled members and
  a clamped integer index.

Assignment, loops, `return`, arbitrary functions, dynamic property names,
strings as runtime values, and unlisted queries are unsupported.

- [ ] **Step 2: Run RED**

```powershell
cargo test -p asset-compiler --locked --test entity_animation -- --nocapture
cargo test -p asset-compiler --locked --test item_visuals -- --nocapture
```

Expected: compile failures because the parser/compiler modules are absent.

- [ ] **Step 3: Implement deterministic compilation and fallback attribution**

Parse `animations/*.json`, `animation_controllers/*.json`, client entity
definitions, render controllers, `textures/item_texture.json`, item texture
PNGs, and reviewed block-item aliases. Resolve geometry/render-controller/
material/texture/animation/controller inheritance into dense sorted indices.
Compile expressions with precedence-aware parsing, constant folding, explicit
division/modulo-by-zero behavior of `0.0`, a 32-level parse-depth ceiling, a
256-op ceiling, and calculated VM stack depth.

Use this exact outcome boundary:

```rust
pub enum CompileReferenceOutcome<T> {
    Resolved(T),
    OptionalStaticFallback { source: u32, symbol: u32, reason: FallbackReason },
    RequiredRigRejected { source: u32, symbol: u32, reason: RejectReason },
}
```

Compile item visuals in canonical identifier order. A block item maps to the
existing dense block visual, a textured item maps to one shared sprite mesh
plus texture region, air maps to `EmptyHand`, and an unresolved definition maps
to `Missing` with source attribution. First-person, third-person, and dropped
display transforms are finite fixed-size records, not free-form JSON.

- [ ] **Step 4: Verify the pinned real pack without committing output**

```powershell
make entity-assets
cargo run -p asset-compiler --locked --bin assetc -- entity-assets --pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack --out .local/assets/compiled/vanilla-v1.mcbeent --report .local/assets/reports/entity-assets.json
Get-FileHash .local/assets/compiled/vanilla-v1.mcbeent -Algorithm SHA256
git status --short
```

Expected: both compiles succeed; report counts are below every ceiling; every
rejection/fallback names source and symbol; the generated blob remains ignored;
tracked status contains only source and tests.

- [ ] **Step 5: Run GREEN**

```powershell
cargo test -p asset-compiler --locked --test entity_animation -- --nocapture
cargo test -p asset-compiler --locked --test item_visuals -- --nocapture
cargo test -p asset-compiler --locked --test entity -- --nocapture
cargo clippy -p asset-compiler --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all commands pass.

- [ ] **Step 6: Commit**

```powershell
git add crates/asset-compiler/src crates/asset-compiler/tests
git commit -m "feat: compile entity animation and item assets"
```

### Task 4: Resolve rigs and evaluate bounded adjacent tick poses

**Files:**
- Create: `crates/client-world/src/actor_animation.rs`
- Create: `crates/client-world/tests/entity_runtime.rs`
- Modify: `crates/client-world/src/actor_store.rs`
- Modify: `crates/client-world/src/actor_store/lifecycle.rs`
- Modify: `crates/client-world/src/actor_store/query.rs`
- Modify: `crates/client-world/src/actor_store/tests.rs`
- Modify: `crates/client-world/src/stream.rs`
- Modify: `crates/client-world/src/stream/construction.rs`
- Request integration export: `crates/client-world/src/lib.rs`

**Interfaces:**
- Consumes immutable `RuntimeEntityAssets`.
- Produces one `ActorRigSnapshot` per visible actor with adjacent completed bone
  palettes and reset provenance.

- [ ] **Step 1: Write failing rig resolution and VM tests**

Exercise symbol/inheritance resolution, metadata/attribute/pose/query inputs,
ground and velocity changes, controller FIFO transitions, loop/clamp behavior,
pre/post interpolation, optional static fallback, required rig rejection,
malformed indices, non-finite VM results, operation exhaustion, collection
clamping, actor replacement, dimension change, incompatible metadata, teleport,
and adjacent-pose interpolation.

Require these exact runtime ceilings:

```rust
pub const MAX_RUNTIME_BONES_PER_RIG: usize = 96;
pub const MAX_CONTROLLER_TRANSITIONS_PER_TICK: usize = 8;
pub const MAX_MOLANG_OPS_PER_ACTOR_TICK: usize = 4_096;
pub const MAX_MOLANG_OPS_PER_WORLD_TICK: usize = 262_144;
pub const MAX_MOLANG_OPS_PER_RENDER_FRAME: usize = 0;
pub const MAX_ACTOR_ACTION_HISTORY: usize = 32;
```

Molang evaluation is tick-owned, so the explicit render-frame VM budget is
zero; render interpolation reads only the two completed palettes.

- [ ] **Step 2: Run RED**

```powershell
cargo test -p client-world --locked --test entity_runtime -- --nocapture
```

Expected: compile failure because runtime rig types are absent.

- [ ] **Step 3: Implement resolver, VM, controller, and pose state**

Add this render-neutral publication surface:

```rust
pub struct ActorRigSnapshot<'a> {
    pub actor: ActorLifetimeId,
    pub rig: EntityRigId,
    pub previous: &'a [BoneTransform],
    pub current: &'a [BoneTransform],
    pub completed_tick: u64,
    pub reset_generation: u64,
    pub fallback: EntityRigFallback,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
pub struct BoneTransform {
    pub rotation: [f32; 4],
    pub translation_scale: [f32; 4],
}
```

Add `WorldStream::new_with_asset_sets(bootstrap, runtime_assets,
entity_assets, current_position, existing_anchor)` and keep
`new_with_assets` as the diagnostic-entity compatibility wrapper for existing
tests. Resolve entity definitions and render controllers once per actor
lifetime into validated geometry, bone, material, texture, animation-clip, and
controller indices.
At each 20 Hz completed tick, evaluate queries from bounded metadata,
attributes, pose flags, finite velocity, ground state, and retained history;
advance controllers in declared FIFO order; evaluate clips; compose parented
bone transforms; then swap previous/current palettes. Rendering reads but never
mutates this state.

Reset both palettes to the newly evaluated pose and increment
`reset_generation` for replacement, session/dimension change, incompatible
metadata, and teleport. Exhausted world budget freezes remaining actors at the
last completed pose in stable actor-lifetime order and increments an attributed
counter; it never partially commits a pose.

- [ ] **Step 4: Run GREEN and regression tests**

```powershell
cargo test -p client-world --locked --test entity_runtime -- --nocapture
cargo test -p client-world --locked actor_store -- --nocapture
cargo clippy -p client-world --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all tests pass; existing spawn/move interpolation behavior remains
unchanged when diagnostic entity assets are used.

- [ ] **Step 5: Commit**

```powershell
git add crates/client-world/src crates/client-world/tests/entity_runtime.rs
git commit -m "feat: evaluate bounded runtime entity rigs"
git branch completion-phase4-actor-interface HEAD
```

Expected: the immutable actor-interface branch freezes `ActorLifetimeId`,
`ActorRigSnapshot`, `BoneTransform`, reset generations, and evaluation budgets
before render or app consumers begin.

### Task 5: Normalize item registries, equipment, and remote actions once

**Files:**
- Modify: `crates/protocol/Cargo.toml`
- Create: `crates/protocol/src/item.rs`
- Create: `crates/protocol/tests/items_actions.rs`
- Modify: `crates/protocol/src/actor.rs`
- Modify: `crates/protocol/tests/actors.rs`
- Modify: `crates/protocol/tests/world_packets.rs`
- Request integration edits: export `protocol::item` from `lib.rs` and add the
  reviewed ItemRegistry/MobEquipment/Animate/AnimateEntity match arms to
  `protocol::world::into_world_event`

**Interfaces:**
- Produces vendor-independent packet records; actor lifetime and session are
  stamped by Task 6 when FIFO events enter `WorldStream`.

- [ ] **Step 1: Write failing packet normalization tests**

Construct generated protocol-1001 packets for AddPlayer held item,
ItemRegistry, MobEquipment, every reviewed Animate action, AnimateEntity with
one and maximum runtime IDs, authoritative left/right-hand metadata, and
malformed/oversized variants.
Assert `into_world_event` retains FIFO order and unrelated packets still return
`Ok(None)`.

Use these limits:

```rust
pub const MAX_ITEM_REGISTRY_ENTRIES: usize = 16_384;
pub const MAX_ITEM_EXTRA_BYTES: usize = 64 * 1024;
pub const MAX_ANIMATE_ENTITY_IDS: usize = 256;
pub const MAX_ACTION_IDENTIFIER_BYTES: usize = 256;
pub const MAX_ANIMATION_IDENTIFIER_BYTES: usize = 256;
```

- [ ] **Step 2: Run RED**

```powershell
cargo test -p protocol --locked --test items_actions -- --nocapture
```

Expected: failures because ItemRegistry, MobEquipment, Animate, AnimateEntity,
handedness, and AddPlayer held item are not normalized.

- [ ] **Step 3: Implement the bounded normalized surface**

Use these core records:

```rust
pub struct NetworkItemStack {
    pub network_id: i32,
    pub metadata: u32,
    pub stack_network_id: i32,
    pub count: u16,
    pub nbt_digest: [u8; 32],
    pub block_runtime_id: i32,
    pub extra_data: Arc<[u8]>,
}

pub struct EquipmentEvent {
    pub actor_runtime_id: u64,
    pub stack: NetworkItemStack,
    pub inventory_slot: i32,
    pub selected_slot: u8,
    pub window_id: u8,
    pub handedness: Option<ActorHandedness>,
}

pub enum ActorActionKind {
    SwingArm,
    Wake,
    CriticalHit,
    MagicCriticalHit,
    RowRight,
    RowLeft,
    Custom { animation: Arc<str>, controller: Arc<str> },
}

pub enum ItemActorEvent {
    Registry(ItemRegistryEvent),
    Action(ActorActionEvent),
}
```

Promote the existing `sha2` dev dependency to a normal crate dependency, hash
validated item extra/NBT bytes into `nbt_digest`, and retain at most 64 KiB of
exact wire bytes in `NetworkItemStack` for Phase 5-authoritative outbound use.
Remote actor conversion drops `extra_data` immediately after verifying the
digest. Keep `protocol` independent of `assets`; Tasks 6 and Phase 5 convert
`NetworkItemStack` to the one `ItemStackIdentity` outside protocol.
Convert `Item`, `ItemNew`, and reviewed item-stack wire variants through one
checked helper. Add the normalized held stack to
`ActorSpawnEvent` for AddPlayer; non-player spawns use the empty identity.
Map MobEquipment exactly once to `WorldEvent::Equipment(EquipmentEvent)` with
inventory slot, selected slot, window ID, and proven handedness without lossy
casts. After local identity is known, integration routes a matching local event
only to Phase 5 selection and a nonmatching event only to remote equipment;
pre-identity events remain in one bounded FIFO and cannot be double-applied. Map
known Animate discriminants explicitly; unknown discriminants become
attributed ignored actions. AnimateEntity accepts only bounded strings and
runtime-ID lists and is the sole source of `Custom`.

Reject invalid runtime IDs, oversized collections/strings/NBT, count conversion
overflow, invalid selected slots, and contradictory handedness. AddItemEntity
is deliberately deferred to tranche-C Task 10. Do not expose vendored packet or
NBT types from `protocol`.

- [ ] **Step 4: Run GREEN**

```powershell
cargo test -p protocol --locked --test items_actions -- --nocapture
cargo test -p protocol --locked --test actors -- --nocapture
cargo test -p protocol --locked --test world_packets -- --nocapture
cargo clippy -p protocol --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all commands pass and all generated/vendor types remain behind the
normalization boundary.

- [ ] **Step 5: Commit**

```powershell
git add crates/protocol/Cargo.toml crates/protocol/src/item.rs crates/protocol/src/actor.rs crates/protocol/tests
git commit -m "feat: normalize actor item and action packets"
```

### Task 6: Retain canonical remote equipment and action timelines

**Files:**
- Create: `crates/client-world/src/item.rs`
- Create: `crates/client-world/src/action.rs`
- Create: `crates/client-world/tests/item_actions.rs`
- Modify: `crates/client-world/src/actor_store.rs`
- Modify: `crates/client-world/src/actor_store/lifecycle.rs`
- Modify: `crates/client-world/src/actor_store/query.rs`
- Modify: `crates/client-world/src/actor_store/tests.rs`
- Modify: `crates/client-world/src/stream.rs`
- Modify: `crates/client-world/src/stream/construction.rs`
- Modify: `crates/client-world/src/stream/sequencing.rs`
- Request integration edit: export reviewed item/action snapshots from
  `crates/client-world/src/lib.rs`

**Interfaces:**
- Consumes protocol item/action events plus runtime item assets.
- Produces canonical remote render snapshots and stable action identities. Phase
  5 owns the separate local authoritative wire stack and local action timeline.

- [ ] **Step 1: Write failing state-machine tests**

Test AddPlayer initial equipment, MobEquipment replacement, registry arriving
before and after equipment, left/right/default-hand resolution, actor
replacement, RemoveEntity, dimension/session reset, duplicate/past sequence
rejection, unknown item fallback, custom action FIFO, identical action
deduplication, later-source-tick restart, overlapping different-action
replacement, history overflow, teleport reset, and stale lifetime rejection.

Require these retained bounds:

```rust
pub const MAX_ITEM_REGISTRY_RECORDS: usize = 16_384;
pub const MAX_PENDING_ITEM_RESOLUTIONS: usize = 1_024;
pub const MAX_ACTIONS_PER_ACTOR: usize = 32;
pub const MAX_ACTION_EVENTS_PER_TICK: usize = 4_096;
```

- [ ] **Step 2: Run RED**

```powershell
cargo test -p client-world --locked --test item_actions -- --nocapture
```

Expected: compile failure because canonical remote item/action state is absent.

- [ ] **Step 3: Implement exact identity stamping and resolution**

Stamp every applied event as:

```rust
pub enum ActorSourceTick {
    Packet(i64),
    IngressSequence(u64),
}

pub struct ActorEventIdentity {
    pub session_id: u64,
    pub dimension: i32,
    pub actor_lifetime: u64,
    pub ingress_sequence: u64,
    pub source_tick: ActorSourceTick,
}

pub struct CanonicalItemStack {
    pub identity: ItemStackIdentity,
    pub identifier: Option<Arc<str>>,
    pub visual: ItemVisualRoute,
}
```

Use packet tick where present and `IngressSequence` otherwise. Resolve live
network IDs through the latest bounded ItemRegistry and then immutable runtime
aliases. Never mutate `ItemStackIdentity` when a visual changes from `Missing`
to resolved. Before conversion, recompute SHA-256 over
`NetworkItemStack::extra_data`, require equality with `nbt_digest`, then discard
the bytes from remote state. Store equipment and action state on
`ActorLifetimeId`, not raw runtime ID.

Advance one bounded **remote timeline per actor** using the frozen
`assets::ItemActionPhase`. Its dedupe key is `(actor_lifetime, action_kind,
source_tick, ingress_sequence)`: an identical key is ignored, the same action
with a later source tick restarts from Windup, and a different accepted action
replaces according to FIFO order. Remote Animate/AnimateEntity events select
phases; missing clips use the same bounded phase with static item/arm transforms
and an attributed fallback. Local attack/break/place/use has exactly one Phase
5 timeline and is not instantiated here.

- [ ] **Step 4: Run GREEN and world regressions**

```powershell
cargo test -p client-world --locked --test item_actions -- --nocapture
cargo test -p client-world --locked actor_store -- --nocapture
cargo test -p client-world --locked stream -- --nocapture
cargo clippy -p client-world --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all commands pass; dimension/session changes leave no stale remote
equipment, actions, or pending resolutions.

- [ ] **Step 5: Commit**

```powershell
git add crates/client-world/src crates/client-world/tests/item_actions.rs
git commit -m "feat: retain remote actor equipment and actions"
```

### Task 7: Replace the static biped path with shared skeletal GPU presentation

**Files:**
- Create: `crates/render/src/actor/rig.rs`
- Create: `crates/render/src/actor/gpu.rs`
- Create: `crates/render/tests/actor_rig.rs`
- Modify: `crates/render/src/actor.rs`
- Modify: `crates/render/src/actor_render.rs`
- Modify: `crates/render/src/actor.wgsl`
- Request integration edit: export reviewed render PODs/plugin from
  `crates/render/src/lib.rs`

**Interfaces:**
- Consumes only render-owned PODs plus `assets` handles. App conversion copies
  validated fields from `client_world::ActorRigSnapshot`; `render` never
  depends on `client-world`.
- Produces a bounded render frame with packet/store/pose/draw identity and dual
  palette offsets.

- [ ] **Step 1: Write failing CPU/GPU layout and extraction tests**

Test parented bone transforms, previous/current palette offsets, `partial_tick`
clamping, finite conversion, culling before arena reservation, shared geometry
reuse, deterministic overflow, static fallback, no-draw route, actor
replacement generation, and exact `#[repr(C)]` shader layout sizes.

Use this instance contract:

```rust
pub struct ActorRigRenderInput {
    pub identity: ActorRenderIdentity,
    pub rig: EntityRigId,
    pub previous_bones: Arc<[RenderBoneTransform]>,
    pub current_bones: Arc<[RenderBoneTransform]>,
    pub completed_tick: u64,
    pub reset_generation: u64,
}

#[repr(C)]
pub struct ActorGpuInstance {
    pub world_from_actor: [[f32; 4]; 3],
    pub previous_bone_base: u32,
    pub current_bone_base: u32,
    pub geometry_id: u32,
    pub texture_layer: u32,
    pub partial_tick: f32,
    pub reset_generation: u32,
}
```

- [ ] **Step 2: Run RED**

```powershell
cargo test -p render --locked --test actor_rig -- --nocapture
```

Expected: compile failure because skeletal rig extraction and palette arenas do
not exist.

- [ ] **Step 3: Implement shared geometry and compact dual-pose arenas**

Upload each validated geometry/skin/material family once. The app converts each
client-world transform to finite `render::RenderBoneTransform`; render converts
that POD to one 3x4 `f32` affine matrix during bounded extraction and
reserve at most `128 actors * 96 bones * 2 poses * 48 bytes = 1,179,648 bytes`
per presented actor frame. Keep the existing `MAX_RENDERED_PLAYERS == 128` and
distance culling before upload. Vertices carry one validated bone index for
rigid Bedrock cubes; the shader reads adjacent matrices and interpolates their
transformed position and normal by clamped `partial_tick`.

The app-owned adapter consumes Phase 3 `LocalAvatarPresentation`, combines a
visible local pose with the canonical Phase 4 rig/skin/equipment/action state,
and emits the same render-owned actor POD used by remotes. Selection removes
the local runtime ID from the remote candidates. `HiddenFirstPerson` submits
zero local world actors and permits at most 128 remotes; either visible
third-person mode reserves the local slot first and permits at most 127
remotes. Tests require world-avatar counts `0/1/1` for first/rear/front and an
absolute 128-total arena ceiling.

Replace fixed part-number head rotation with compiled bone indices. Preserve
the shared standard-biped diagnostic geometry for rejected/missing rigs. A rig
with invalid indices, non-finite transforms, or arena overflow is excluded from
the draw and counted by exact reason; never resize the arena mid-frame.

- [ ] **Step 4: Run GREEN and shader checks**

```powershell
cargo test -p render --locked --test actor_rig -- --nocapture
cargo test -p render --locked actor -- --nocapture
cargo clippy -p render --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all commands pass and tests prove multiple actors share geometry,
materials, textures, and pipeline resources.

- [ ] **Step 5: Commit**

```powershell
git add crates/render/src/actor.rs crates/render/src/actor crates/render/src/actor_render.rs crates/render/src/actor.wgsl crates/render/tests/actor_rig.rs
git commit -m "feat: render shared skeletal actor rigs"
```

### Task 8: Render remote equipment and deduplicated action poses

**Files:**
- Create: `crates/render/src/item.rs`
- Create: `crates/render/src/item.wgsl`
- Create: `crates/render/tests/item_presentation.rs`
- Create: `app/src/presentation.rs`
- Create: `app/src/presentation/actors.rs`
- Create: `app/src/tests/phase4_presentation.rs`
- Modify: `crates/render/src/actor.rs`
- Modify: `crates/render/src/actor_render.rs`
- Modify: `crates/render/src/actor.wgsl`
- Request integration call-site edits: `app/src/asset_startup.rs`, `app/src/runtime/network.rs`
- Request integration edits: shared render/app `lib.rs` exports,
  `Phase4PresentationPlugin` registration, and schedule edges

**Interfaces:**
- Completes the actor-facing portion of Phase 4.5 required before binding 4.4.
- Exports `Phase4PresentationPlugin`; the integration owner inserts it in
  `ClientFrameSet::ActorPublication` and retains ownership of `app/src/app.rs`.

- [ ] **Step 1: Write failing render-list and publication tests**

Test right-hand and left-hand bone attachment when protocol metadata proves the
hand, reviewed right-hand default with an attributed counter when it does not,
first-/third-person transform separation, swing/use phase sampling, identical
action deduplication, later-source-tick restart, equipment replacement, empty
hand, missing visual, block item, sprite item, culling, dimension cleanup, and
correlation identity through extraction. Feed Phase 3
`LocalAvatarPresentation` in first/rear/front modes and assert local world draw
counts `0/1/1`, local runtime-ID deduplication, 128
remotes when local is hidden, and at most 127 remotes plus one local when
visible. Assert that cycling equipment changes
only instance/item handles and allocates no new texture, mesh, material, or bind
group.

- [ ] **Step 2: Run RED**

```powershell
cargo test -p render --locked --test item_presentation -- --nocapture
cargo test -p bedrock-client --locked phase4_presentation -- --nocapture
```

Expected: compile failures because item render lists and app publication are
absent.

- [ ] **Step 3: Implement canonical item GPU assets and third-person attachment**

At asset upload, build one shared sprite mesh and immutable texture-array
regions for compiled items; reuse block visuals for `BlockItem`, and expose the
frozen `ItemIconRef` without copying texture data. Publish:

```rust
pub struct ActorPresentationIdentity {
    pub session_id: u64,
    pub dimension: i32,
    pub actor_lifetime: u64,
    pub ingress_sequence: u64,
    pub completed_tick: u64,
    pub pose_generation: u64,
    pub draw_generation: u64,
    pub item_visual: ItemVisualRoute,
    pub action_phase: ItemActionPhase,
}
```

Attach remote items to the compiled left/right hand selected by proven actor
metadata; use the reviewed right-hand default only when handedness is absent.
Sample attack/use clips from the shared action phase; if a clip is missing,
retain the item and apply the finite static fallback transform. Cap tranche-B
item instances at 128 held items; Task 10 expands the preallocated arena to
4,224 when dropped items land.

- [ ] **Step 4: Publish through the normal app path**

Retain `LoadedEntityAssets.runtime()` in `ClientWorld`, construct
`WorldStream::new_with_asset_sets`, advance actor rigs on completed actor ticks,
and publish actor/item frames through `Phase4PresentationPlugin`. Do not add an
alternate acceptance render source. Add an integration handoff note requiring:

```rust
app.add_plugins(Phase4PresentationPlugin);
// publish_phase4_actor_frame belongs to ClientFrameSet::ActorPublication.
```

- [ ] **Step 5: Run GREEN and affected verification**

```powershell
cargo test -p render --locked --test item_presentation -- --nocapture
cargo test -p bedrock-client --locked phase4_presentation -- --nocapture
cargo test -p bedrock-client --locked network -- --nocapture
cargo run -p devtool --locked -- verify-affected --base completion-phase4-base
cargo fmt --all -- --check
git diff --check
```

Expected: render/leaf-module tests pass in the producer lane. The two
`bedrock-client` public gates pass after integration applies the reviewed
asset-startup/network call-site hunks; the devtool then reports no failing
affected crate, strict Clippy, format, architecture, or carrier checks.

- [ ] **Step 6: Commit**

```powershell
git add crates/render app/src/presentation.rs app/src/presentation app/src/tests/phase4_presentation.rs
git commit -m "feat: present remote held item actions"
```

### Task 9: Add actor/item evidence contracts and run the non-binding LBSG diagnostic

**Files:**
- Create: `app/src/acceptance/actor_witness.rs`
- Create: `app/src/acceptance/item_witness.rs`
- Create: `scripts/acceptance/Actors.ps1`
- Create: `scripts/tests/acceptance.Actors.Tests.ps1`
- Create: `docs/evidence/phase-4/README.md`
- Create: `docs/evidence/phase-4/run-manifest.schema.json`
- Modify: `app/src/args.rs`
- Modify: `app/src/metrics.rs`
- Modify: `scripts/tests/acceptance.Tests.ps1`
- Request integration edits: app acceptance module/marker registration and
  `scripts/acceptance.ps1`/Load/Markers/Metrics/Orchestrator registration of
  the reviewed `Actors.ps1` leaf functions

**Interfaces:**
- Produces parsed, identity-correlated evidence. The first LBSG run is
  diagnostic only because local Phase 4.5 and the Phase 5 authority checkpoint
  are not complete.

- [ ] **Step 1: Write failing marker/parser/reducer tests**

Define request files under ignored `.local/acceptance/` and require two
consecutive presented frames for one stable identity. The completion marker is:

```text
RUST_MCBE_ACTOR_WITNESS_COMPLETE request_sha256=0000000000000000000000000000000000000000000000000000000000000000 session=1 dimension=0 lifetime=1 ingress=1 source_tick_kind=packet source_tick=1 completed_tick=1 pose_generation=1 draw_generation=1 item_visual=empty action_phase=idle feet_error_micros=0 consecutive=2
```

Add parser failures for malformed hashes, zero identities, generation
regression, mismatched consecutive frames, nonzero missing/stale/wrong-session/
wrong-dimension/wrong-lifetime/no-draw counters, and feet-plane error above
10,000 micrometres.

- [ ] **Step 2: Run RED**

```powershell
cargo test -p bedrock-client --locked actor_witness -- --nocapture
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/acceptance.Actors.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
```

Expected: compile/script failures because marker ownership and parsers are
absent.

- [ ] **Step 3: Implement bounded witness capture**

Add `--actor-witness-request PATH` and `--item-witness-request PATH`.
Requests carry expected session/dimension/lifetime or a bounded actor selector,
action phase, item visual, maximum feet error, and required consecutive count
of exactly two. Poll once per frame, cap a request at 16 actors and 64 KiB,
hash canonical request bytes, and complete only from the WGPU presented-frame
acknowledgement. Metrics record packet/store/pose/extraction/draw generations,
fallback counters, bone/item arena high-water bytes, resource creation counts,
and action resource-allocation deltas.

- [ ] **Step 4: Run deterministic GREEN**

```powershell
cargo test -p bedrock-client --locked actor_witness -- --nocapture
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/acceptance.Actors.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$r = Invoke-Pester -Script "scripts/tests/acceptance.Tests.ps1" -PassThru; if ($r.FailedCount -ne 0) { exit 1 }'
cargo fmt --all -- --check
git diff --check
```

Expected: all commands pass and every new marker appears exactly once in the
app ownership table and exactly once in the PowerShell parser registry.

- [ ] **Step 5: Run the early diagnostic on LBSG**

Build release, connect the normal authenticated core to
`play.lbsg.net:19132`, and capture spawn, ordinary movement, rotation, one
teleport, equipment, and any observed Animate event. Record ground-contact
feet origin, packet source tick or ingress fallback, three completed ticks,
adjacent render frames, `STANDING_PLAYER_EYE_HEIGHT == 1.62`, and
`PLAYER_NETWORK_OFFSET == 1.62001` separately.

Expected: the run identifies server-specific packet/asset gaps and never marks
`P4.4-LIVE-ACTOR` complete. Missing authentication, a missing native reference,
or an unobserved required event is recorded as an open blocker, not a pass.

- [ ] **Step 6: Commit deterministic tooling and sanitized diagnostic adjudication**

```powershell
git add app/src/acceptance/actor_witness.rs app/src/acceptance/item_witness.rs app/src/args.rs app/src/metrics.rs scripts/acceptance/Actors.ps1 scripts/tests docs/evidence/phase-4
git commit -m "test: add actor item presentation witnesses"
```

### Task 10: Consume `completion-phase5-authority`, add dropped items, and render the viewmodel

**Dependency gate:** Do not begin this task until the integration branch has
merged the reviewed `completion-phase5-authority` producer with the exact
contract below. Rebase the unpublished Phase 4 tranche-C continuation onto
that integration tip; do not duplicate or adapt around a missing field.

**Files:**
- Modify: `crates/protocol/src/item.rs`
- Modify: `crates/protocol/tests/items_actions.rs`
- Create: `crates/client-world/src/dropped_item.rs`
- Modify: `crates/client-world/src/actor_store.rs`
- Modify: `crates/client-world/src/stream/sequencing.rs`
- Modify: `crates/client-world/tests/item_actions.rs`
- Modify: `crates/render/src/item.rs`
- Modify: `crates/render/tests/item_presentation.rs`
- Create: `crates/render/src/viewmodel.rs`
- Create: `crates/render/src/viewmodel.wgsl`
- Create: `crates/render/tests/viewmodel.rs`
- Create: `app/src/presentation/viewmodel.rs`
- Modify: `app/src/presentation.rs`
- Modify: `app/src/tests/phase4_presentation.rs`
- Request integration edits: AddItemEntity dispatch; client-world/render shared
  exports; app plugin/schedule registration

**Interfaces:**
- Consumes `completion-phase5-authority` plus Phase 3 `PerspectiveMode`; produces
  AddItemEntity lifecycle/dropped rendering and visual-only local presentation.
  The Phase 5 interaction/inventory lane owns the sole local action timeline.

- [ ] **Step 1: Verify the Phase 5 authority producer contract before editing**

```powershell
git fetch --all --prune
git rebase completion-integration
git merge-base --is-ancestor completion-phase5-authority HEAD
```

The last command must exit 0. Published Phase 4 interface branches remain
immutable; only the unpublished tranche-C continuation is rebased.

The merged public surface must be semantically and type-identical to:

```rust
pub struct SelectedItemSnapshot {
    pub session_id: u64,
    pub inventory_revision: u64,
    pub selected_slot: u8,
    pub stack: Option<ItemStackIdentity>,
    pub visual: ItemVisualRoute,
}

pub enum LocalActionReconciliation {
    Predicted { action_id: u64, kind: LocalItemAction, source_tick: u64 },
    Confirmed { action_id: u64, authoritative_tick: u64 },
    Rejected { action_id: u64, authoritative_tick: u64 },
    Cancelled { action_id: u64, reason: ActionCancelReason },
}

pub struct LocalActionTimelineSnapshot {
    pub session_id: u64,
    pub action_id: u64,
    pub revision: u64,
    pub phase: ItemActionPhase,
    pub reconciliation: LocalActionReconciliation,
}
```

`selected_slot` is restricted to `0..=8`; revisions and action IDs are
monotonic within a session; the stack is server-authoritative; attack reach,
target validity, block validity, use duration, and outbound transaction results
come from `completion-phase5-authority`. If the producer differs, resolve the interface in the
integration plan before continuing this task.

- [ ] **Step 2: Write failing viewmodel and reconciliation tests**

Cover AddItemEntity spawn/move/remove and bounds, empty hand, melee item,
placeable block, consumable with duration, missing visual, all nine slots,
`PerspectiveMode::FirstPerson` enablement and both third-person disablements,
exact first/rear/front viewmodel draw counts `1/0/0`,
first/third transform distinction, camera FOV changes, depth isolation,
swing/use phase, duplicate reconciliation revision, later revision restart,
confirm, reject, cancel, selected-stack rollback, session replacement,
dimension change, UI-focus release, and no resource allocation after warmup.

- [ ] **Step 3: Run RED**

```powershell
cargo test -p render --locked --test viewmodel -- --nocapture
cargo test -p bedrock-client --locked phase4_viewmodel -- --nocapture
```

Expected: compile failures because the viewmodel pass and Phase 5 authority consumer
are absent.

- [ ] **Step 4: Implement the separate paper-doll/held-item render pass**

Use a dedicated viewmodel camera/projection and depth target so world geometry
cannot clip the arm/item. Resolve native-matched arm and item transforms from
the compiled first-person display transform, current camera FOV/aspect, and the
Phase 5-owned `LocalActionTimelineSnapshot`. Reuse the selected
`ItemVisualRoute`, `ItemIconRef`, texture array, item mesh, rig, and action clip;
never clone resources for a slot or action. Render the viewmodel only for
`PerspectiveMode::FirstPerson`; Phase 3 owns local-avatar visibility in both
third-person modes.

Render the phase already resolved by the one Phase 5 timeline; do not advance a
second local state machine. `Predicted` displays that phase, `Confirmed` keeps
the producer's completion phase, `Rejected` displays its bounded rollback
phase, and `Cancelled` displays its producer-authored return to idle. A newer
timeline or inventory revision replaces older visual state; duplicate or stale
session/revision/action IDs are ignored with attributed counters. Empty hand
still renders the local arm. Missing visual renders the arm plus the explicit
diagnostic fallback and never guesses another item.

Normalize AddItemEntity in the same `protocol::item` leaf during this tranche,
then store it as a bounded actor lifetime with canonical item identity and
standard adjacent movement history. Render dropped items through the same
`ItemVisualRoute`, using deterministic 20 Hz bob/spin derived from lifetime and
completed tick. Expand the already allocated item arena to its final fixed cap
of 4,224 instances: 128 held plus 4,096 dropped.

- [ ] **Step 5: Run GREEN and integration schedule checks**

```powershell
cargo test -p render --locked --test viewmodel -- --nocapture
cargo test -p bedrock-client --locked phase4_viewmodel -- --nocapture
cargo test -p bedrock-client --locked schedule_order -- --nocapture
cargo clippy -p render -p bedrock-client --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all commands pass. The integration schedule publishes viewmodel state
after Phase 5 authority interaction resolution and before render extraction,
while network send remains owned by `completion-phase5-authority`.

- [ ] **Step 6: Commit**

```powershell
git add crates/protocol/src/item.rs crates/protocol/tests/items_actions.rs crates/client-world/src crates/client-world/tests/item_actions.rs crates/render/src/item.rs crates/render/src/viewmodel.rs crates/render/src/viewmodel.wgsl crates/render/tests/item_presentation.rs crates/render/tests/viewmodel.rs app/src/presentation.rs app/src/presentation/viewmodel.rs app/src/tests/phase4_presentation.rs
git commit -m "feat: present dropped and first person item actions"
git branch completion-phase4-presentation HEAD
```

Expected: `completion-phase4-presentation` is the immutable tranche-C producer
consumed by the Phase 5 witness branch.

### Task 11: Prove deterministic, local two-client, native, and performance gates

**Files:**
- Modify: `scripts/acceptance/Actors.ps1`
- Modify: `scripts/tests/acceptance.Actors.Tests.ps1`
- Modify: `docs/evidence/phase-4/README.md`
- Create from the sanitized schema only: `docs/evidence/phase-4/local-two-client-manifest.json`
- Create from the sanitized schema only: `docs/evidence/phase-4/native-adjudication.json`
- Create from the sanitized schema only: `docs/evidence/phase-4/performance-manifest.json`

**Interfaces:**
- Consumes complete Phase 4.3 and Phase 4.5 implementation plus the immutable
  `completion-phase5-authority` checkpoint.
- Produces evidence references for `P4.3-RIGS` and `P4.5-ITEM-ACTIONS`; the
  integration owner updates the central ledger.

- [ ] **Step 1: Run the clean deterministic gate**

```powershell
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml
Push-Location core; try { go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'core go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'core go vet failed' } } finally { Pop-Location }
cargo run -p devtool --locked -- verify-affected --base completion-phase4-base
git diff --check
git status --short
```

Expected: zero failures, warnings, formatting changes, architecture violations,
carrier drift, or unexplained tracked files.

- [ ] **Step 2: Run deterministic galleries through normal packet ingress**

Use local BDS commands/fixtures to cover one compiled animated non-player rig,
one standard player, rejected required rig, optional static fallback, all
reviewed action phases, proven left/right hand plus absent-hand default,
identical action deduplication and later-tick restart, empty hand, block item,
sprite item, missing item, dropped item spawn/move/remove, teleport,
replacement, dimension reset, and resource-arena saturation. Require two
consecutive exact presented frames for each stable request and zero stale/
wrong-session/wrong-dimension/wrong-lifetime/no-draw mismatches.

- [ ] **Step 3: Run the controlled two-client matrix**

With RustMCBE and a native Bedrock client on one local BDS world, cycle all nine
hotbar slots, empty hand, melee item, placeable block, and duration-use item;
perform one confirmed attack, one miss, one place, one use, one server-rejected
stack request, one drop/pickup, one teleport, one respawn, and one dimension
change. Capture both directions: RustMCBE viewmodel versus native remote held
item/action, and native viewmodel versus RustMCBE remote held item/action.

Expected: selected-stack identity, remote equipment, action phase, dropped-item
identity, hotbar/UI identity, packet/store/pose/draw generations, and presented
frames correlate. No stale item, duplicated action, phantom dropped item, or
second inventory authority remains.

- [ ] **Step 4: Capture matched native references**

Match resource pack/version, world state, item/metadata/count, selected slot,
camera mode, FOV, aspect, UI scale, action source tick, action phase, actor pose,
ground plane, time, and lighting. Adjudicate player walk/idle/turn/teleport,
at least one matched animated non-player mob's idle/move/turn clip and rig,
third-person left/right/default-hand hold/swing/use, dropped bob/spin, and
first-person empty/melee/block/consumable transforms. Every adjudication names
both capture identities and records pass, fail, or open blocker; visual
resemblance without matched state is insufficient.

- [ ] **Step 5: Enforce release performance and resource ceilings**

Warm all actor/item/viewmodel resources, then run the active scene with hotbar,
viewmodel, equipped remote actor, dropped items, and repeated combat/use actions.
The reducer fails unless:

```json
{
  "full_view_remesh_ms_max": 2000,
  "join_settle_ms_max": 2000,
  "teleport_settle_ms_max": 2000,
  "combined_steady_rss_mb_max": 650,
  "combined_steady_cpu_percent_max": 15,
  "frame_time_p95_ms_max": 16.6666666667,
  "frame_time_p99_ms_max": 16.6666666667,
  "frame_time_max_ms_max": 50.0,
  "rendered_actor_count_max": 128,
  "bone_palette_bytes_max": 1179648,
  "item_instance_count_max": 4224,
  "per_action_mesh_allocations": 0,
  "per_action_texture_allocations": 0,
  "per_action_material_allocations": 0,
  "per_action_bind_group_allocations": 0,
  "required_gpu_consecutive_exact_frames": 2,
  "unexplained_diagnostics": 0
}
```

Discard exactly the first 30 warmup seconds, sample one uninterrupted
120-second steady interval, and fail if p95, p99, or maximum frame time exceeds
the explicit ceilings above. Also record p50, 120 one-second resource samples,
the reference machine/GPU/display identity, and actor VM operation high-water
marks.

- [ ] **Step 6: Commit sanitized evidence and hand off ledger references**

```powershell
git add scripts/acceptance/Actors.ps1 scripts/tests/acceptance.Actors.Tests.ps1 docs/evidence/phase-4
git commit -m "test: record phase 4 item action parity"
```

Expected: committed manifests contain hashes, public fixture coordinates,
numeric metrics, and adjudication results only. The integration owner replaces
the `P4.3-RIGS` and `P4.5-ITEM-ACTIONS` ledger cells with these exact manifest
paths and commit hashes.

### Task 12: Run the binding Phase 4.4 LBSG witness and final review

**Dependency gate:** Start only after Task 11 proves the actor-facing Phase 4.5
path and the integration branch contains `completion-phase5-authority` plus the reconciled local
viewmodel. The diagnostic run from Task 9 cannot be promoted.

**Files:**
- Create from the sanitized schema only: `docs/evidence/phase-4/lbsg-binding-manifest.json`
- Create from the sanitized schema only: `docs/evidence/phase-4/lbsg-native-adjudication.json`
- Modify: `docs/evidence/phase-4/README.md`

**Interfaces:**
- Produces the binding `P4.4-LIVE-ACTOR` evidence and final Phase 4 review
  range. The integration owner alone closes roadmap/ledger status.

- [ ] **Step 1: Build and identify one clean release candidate**

```powershell
cargo build --workspace --release --locked
git rev-parse HEAD
git status --short
```

Expected: release build succeeds, the exact commit is written into the ignored
run request before launch, and tracked status is empty.

- [ ] **Step 2: Capture the authenticated LBSG witness**

Connect the normal authenticated core to `play.lbsg.net:19132`. Capture at least
one player spawn, ordinary movement, body/head rotation, server teleport,
equipment replacement, and observed action pose. For each actor, correlate
packet type, runtime/unique ID, session, dimension, actor lifetime, source tick
or ingress fallback, three completed 20 Hz poses, adjacent render frames, rig
and item asset identities, pose/draw generations, and two consecutive WGPU
presented-frame acknowledgements.

Require both feet on the same captured ground plane with at most 0.01 block
error, no 1.6-block spawn or teleport jump, three-tick convergence after
ordinary movement, reset rather than interpolation across teleport, and
separate evidence that eye height `1.62` was not substituted for network offset
`1.62001`. Any absent required event, authentication failure, unexplained
fallback, stale identity, no-draw route, or missing consecutive frame keeps the
gate open.

- [ ] **Step 3: Capture the matching native LBSG reference**

Use the same server, resource-pack version, observed actor, equipment, action
phase, camera/FOV/aspect, and ground reference. Redact usernames, UUIDs, chat,
tokens, and server transfer secrets. The adjudication records exact capture
hashes and pass/fail/open status for origin, interpolation, teleport reset,
limb pose, held-item transform, and action timing.

- [ ] **Step 4: Re-run final deterministic and performance gates**

```powershell
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml
cargo run -p devtool --locked -- verify-affected --base completion-phase4-base
git diff --check
git status --short
```

Expected: every command passes, performance remains within Task 11 ceilings,
and tracked status contains only the sanitized LBSG evidence files before
commit.

- [ ] **Step 5: Request independent code and evidence review**

Invoke `superpowers:requesting-code-review` for
`completion-phase4-base..HEAD`. Require review
of carrier compatibility, Molang grammar and budgets, rig resets, GPU bounds,
packet normalization, lifetime/FIFO identity, inventory authority, prediction
rollback, missing-asset routes, evidence privacy, native match, and release
performance. Every actionable finding is fixed and the affected deterministic
and live gates are rerun before approval.

- [ ] **Step 6: Commit binding evidence and hand off final status**

```powershell
git add docs/evidence/phase-4
git commit -m "test: record binding LBSG actor witness"
git status --short
```

Expected: clean Phase 4 branch. Give the integration owner the reviewed commit
range and exact `P4.3-RIGS`, `P4.4-LIVE-ACTOR`, and `P4.5-ITEM-ACTIONS`
manifest paths. The integration owner marks a requirement complete only when
all deterministic, live, native, performance, privacy, and review cells are
authoritative; any missing cell remains open.

## Self-Review

- Spec coverage: Tasks 2-4 compile and evaluate clips, the bounded Molang and
  controller subset, resolved runtime rigs, adjacent completed poses, resets,
  fallbacks, and operation budgets. Task 7 supplies shared skeletal GPU data.
- Item/action coverage: Tasks 1, 3, 5, 6, and 8 provide one canonical identity
  from AddPlayer/ItemRegistry/MobEquipment/Animate/AnimateEntity through remote
  equipment and third-person action poses; tranche-C Task 10 alone adds
  AddItemEntity and dropped items.
- Cross-phase authority: Task 10 names the exact `completion-phase5-authority` selected-stack and
  reconciliation dependency and prohibits a Phase 4 inventory or combat store.
- Presentation coverage: third-person equipment/actions land before the
  actor-facing checkpoint; dropped items and the first-person arm/held-item viewmodel
  has distinct transforms/depth/FOV but shared identity/assets/action phase.
- Evidence order: Task 9's LBSG run is explicitly diagnostic. Task 12 is binding
  only after actor-facing Phase 4.5, the Phase 5 authority dependency, local two-client
  evidence, native adjudication, and performance gates.
- Bounds and identity: every new retained collection and arena has a numeric
  ceiling; packet/store/pose/draw/presented-frame identity is explicit; all
  scalar and index inputs are validated before evaluation or GPU upload.
- Ownership: lane commits avoid root manifests, `plan.md`, central ledger,
  architecture allowlists, and `app/src/app.rs`; integration handoffs state the
  exact required plugin and schedule edge.
- Completion semantics: unavailable authentication, server events, native
  references, performance measurements, or exact presented frames remain open
  blockers and never become inferred passes.
