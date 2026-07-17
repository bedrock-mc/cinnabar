# Phases 2.5 Through 5 Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate the independently implemented Phase 2, Phase 3, Phase 4,
and Phase 5 lanes from canonical `d8e4699` without duplicating stale work and
close every gate in the approved completion specification.

**Architecture:** One clean integration branch owns shared manifests,
workspace files, app scheduling, requirement evidence, and cross-lane merges.
Four fresh lane worktrees own isolated feature modules and publish small
reviewed commits at three dependency checkpoints. Shared interfaces merge
before their consumers; shared dispatch/export roots are serialized by the
integration lane. No lane merges a stale archival branch or edits another
lane's worktree.

**Tech Stack:** Rust 1.93.1 edition 2024, Cargo resolver 3, Bevy 0.18.1/WGPU
27, Go 1.26.1, PowerShell 5.1 acceptance scripts, Git worktrees, protocol 1001
Bedrock 1.26.30, local BDS, and authenticated Lunar/Zeqa/LBSG sessions.

## Global Constraints

- Start from the reviewed `full-client-design` tip containing these plans;
  require approved spec commit `b2940086fa2bc8d7089ae065da575086557cbd67`
  and canonical functional base
  `d8e469979a0ec6c4798bb2ffc1dc45d3a9891eeb` as ancestors.
- Treat every pre-existing Phase 2/3/4 worktree as archival; never merge,
  rebase, reset, clean, or delete it.
- Run at most three implementation agents plus one integration/review agent.
- Combat is strictly vanilla; never add extended reach, randomized reach,
  target enlargement, auto-clicking, or Lunar module integration.
- Freecam remains network-silent and physics authority remains disabled until
  its deterministic and live gates pass.
- UI focus releases gameplay actions before the next fixed tick.
- Local viewmodel, remote equipment, inventory UI, dropped items, and outbound
  transactions share one canonical item identity.
- No Mojang payload, generated production carrier, credential, token, live
  capture containing secrets, or per-user settings file is committed.
- Every retained collection, queue, history, arena, text, NBT value, and async
  job has explicit item and byte ceilings.
- Release evidence uses normal FIFO/presentation paths, combined steady-state
  RSS at most 650 MB, combined CPU at most 15 percent, and full-view remesh at
  most two seconds.

## Plan Set and Ownership

| Plan | Lane ownership | Shared interfaces published |
|---|---|---|
| `2026-07-17-phase-2-completion.md` | biome, streaming, publication, lighting, atmosphere | publication metrics and immutable world identities |
| `2026-07-17-phase-3-movement-controls-camera.md` | collision, simulation, prediction, semantic input, camera | `ActionSnapshot`, local interpolated pose, camera mode |
| `2026-07-17-phase-4-entities-items-actions.md` | entity assets, rigs, equipment, item visuals, viewmodel | canonical item visual identity and actor pose/action publication |
| `2026-07-17-phase-5-ui-interaction-settings.md` | UI, HUD, inventory, combat, forms, settings | UI actions, inventory authority, selected stack, settings |

The integration lane exclusively owns root `Cargo.toml`, `Cargo.lock`,
`plan.md`, app plugin/schedule assembly, architecture allowlists, and the final
requirement ledger. A lane that needs one of these files emits a small patch or
documents the required edge in its commit message; the integration owner
applies it after reviewing the producer interface.

The integration lane also serializes central exports and dispatch changes in
`crates/assets/src/lib.rs`, `crates/asset-compiler/src/lib.rs`,
`crates/asset-compiler/src/bin/assetc.rs`, `crates/protocol/src/lib.rs`,
`crates/protocol/src/world.rs`, `crates/render/src/lib.rs`, `app/src/lib.rs`,
`app/src/runtime/network.rs`, `app/src/runtime/world.rs`,
`app/src/acceptance/markers.rs`, and shared
acceptance entry points. Lane producer commits add focused child modules and
tests; their handoff records the exact export, subcommand, packet-dispatch,
runtime, marker, and parser edges required. The integration commit applies
those shared edits once in dependency order. Phase 3 Tasks 7-11 are likewise
integration-serialized because they touch shared app/render assembly; their
feature modules may be prepared in the lane, but the owning integration commit
applies `app/src/app.rs`, shared module-root exports, architecture policy, and
roadmap changes.

---

### Task 1: Create the clean integration and lane worktrees

**Files:**
- Verify only: `.git`, `.worktrees/*`
- Verify only: `docs/superpowers/specs/2026-07-16-phases-2-5-through-5-completion-design.md`

**Interfaces:**
- Consumes: reviewed `full-client-design`, canonical `d8e4699`, and approved
  documentation commit `b294008`.
- Produces: branches `completion-integration`, `completion-phase2`,
  `completion-phase3`, `completion-phase4`, and `completion-phase5` in fresh
  worktrees.

- [ ] **Step 1: Read and invoke the worktree safety workflow**

Run:

```powershell
Get-Content -Raw C:\Users\Hashim\.codex\plugins\cache\openai-curated-remote\superpowers\6.1.1\skills\using-git-worktrees\SKILL.md
```

Expected: the complete worktree workflow prints before any branch or worktree
mutation.

- [ ] **Step 2: Verify the immutable base and archival state**

Run:

```powershell
git rev-parse full-client-design^{commit}
git merge-base --is-ancestor b294008 full-client-design
git merge-base --is-ancestor d8e4699 full-client-design
git worktree list --porcelain
git status --short
```

Expected: one reviewed plan-set commit hash, both merge-base commands exit
zero, all existing worktrees are listed, and only previously recorded
user-owned changes appear in the root worktree.

- [ ] **Step 3: Create the integration worktree**

Run from the repository root after confirming the target path does not exist:

```powershell
git branch completion-integration full-client-design
git worktree add .worktrees/completion-integration completion-integration
```

Expected: a new clean worktree at `.worktrees/completion-integration` whose
`HEAD` matches the reviewed `full-client-design` tip.

- [ ] **Step 4: Create four lane worktrees from the integration tip**

Run:

```powershell
git branch completion-phase2 completion-integration
git branch completion-phase3 completion-integration
git branch completion-phase4 completion-integration
git branch completion-phase5 completion-integration
git worktree add .worktrees/completion-phase2 completion-phase2
git worktree add .worktrees/completion-phase3 completion-phase3
git worktree add .worktrees/completion-phase4 completion-phase4
git worktree add .worktrees/completion-phase5 completion-phase5
```

Expected: all five new worktrees are clean and have the same initial tree.

- [ ] **Step 5: Record the baseline identities without modifying archives**

Run:

```powershell
git worktree list --porcelain
git -C .worktrees/completion-integration status --short
git -C .worktrees/completion-phase2 status --short
git -C .worktrees/completion-phase3 status --short
git -C .worktrees/completion-phase4 status --short
git -C .worktrees/completion-phase5 status --short
```

Expected: empty status for every new worktree; archival worktree paths and
branches remain byte-for-byte untouched.

- [ ] **Step 6: Reserve the diagnostic-only checkpoint-0 boundary**

Do not dispatch behavior tasks yet. Checkpoint 0 runs in Task 3 only after the
reviewed behavior-neutral Phase 2 diagnostics and runners are integrated. No
biome, publication, protocol, scheduling, lighting, or atmosphere behavior
change is allowed before that boundary completes.

### Task 2: Add the amendment roadmap and requirement ledger

Before any Task 2 edit, move from the repository root into the new integration
worktree and prove the active branch. Every remaining command in this plan is
relative to that worktree unless a command explicitly uses `git -C`:

```powershell
Set-Location .worktrees/completion-integration
if ((git branch --show-current) -ne 'completion-integration') {
    throw 'Task 2 must run in the completion-integration worktree'
}
```

**Files:**
- Modify: `plan.md`
- Create: `docs/evidence/phases-2-5-completion-ledger.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `tools/architecture/policy.toml`
- Create: `crates/input/Cargo.toml`
- Create: `crates/input/src/lib.rs`
- Create: `crates/ui/Cargo.toml`
- Create: `crates/ui/src/lib.rs`

**Interfaces:**
- Consumes: approved spec headings 3.4, 4.5, expanded 5.5, and 5.8.
- Produces: stable requirement IDs consumed by lane commits and final
  verification.

- [ ] **Step 1: Write the failing roadmap contract test**

Create `tools/architecture/src/completion_plan.rs` and register it from the
existing architecture tool with this exact required-ID surface:

```rust
pub const REQUIRED_COMPLETION_IDS: &[&str] = &[
    "P2.5-NATIVE-BIOME",
    "P2-CHUNK-PUBLICATION",
    "P2.7-ATMOSPHERE",
    "P3-MOVEMENT",
    "P3.4-INPUT-CAMERA",
    "P4.3-RIGS",
    "P4.4-LIVE-ACTOR",
    "P4.5-ITEM-ACTIONS",
    "P5.1-UI",
    "P5.2-HUD",
    "P5.3-CHAT",
    "P5.4-SCOREBOARD",
    "P5.5-INTERACTION-COMBAT-INVENTORY",
    "P5.6-FORMS",
    "P5.7-PARITY-PERF",
    "P5.8-SETTINGS",
];
```

The test reads `plan.md` and the ledger and fails unless each ID occurs exactly
once in the roadmap and exactly once as a ledger heading.

- [ ] **Step 2: Run the test and verify it fails**

Run:

```powershell
cargo test -p architecture --locked completion_plan -- --nocapture
```

Expected: FAIL listing the absent requirement IDs.

- [ ] **Step 3: Add the exact roadmap entries**

Add unchecked entries under their owning phases in `plan.md` with these titles:

```markdown
- [ ] **3.4 Semantic controls and camera perspectives.** `P3.4-INPUT-CAMERA`
- [ ] **4.5 Held items, actions, dropped items, and viewmodel.** `P4.5-ITEM-ACTIONS`
- [ ] **5.8 In-game menu, controls, video settings, and persistence.** `P5.8-SETTINGS`
```

Append the corresponding IDs to the existing 2.5, chunk publication, 2.7,
Phase 3, 4.3, 4.4, and 5.1-5.7 headings without changing their meaning. Expand
5.5 with the approved strictly vanilla entity-combat paragraph verbatim from
the spec.

- [ ] **Step 4: Create the ledger skeleton with no unowned requirements**

Use one heading per required ID and this exact table beneath every heading:

```markdown
| Field | Evidence |
|---|---|
| Owning plan/task | Not started |
| Deterministic tests | Not started |
| Review commit | Not started |
| Live/native witness | Not started |
| Performance/resource witness | Not started |
| Final status | Open |
```

`Not started` and `Open` are explicit initial states, not completion evidence.

- [ ] **Step 5: Run the contract test and commit**

Before running the gate, add minimal compiling package shells for
`semantic-input` at `crates/input` and `ui` at `crates/ui`, register both
workspace members, and add their empty dependency allowlists. These shells
contain only crate documentation and no public gameplay/UI contract. They are
integration-owned bootstrap files so all parallel lanes begin from identical
workspace metadata.

Run:

```powershell
cargo test -p architecture --locked completion_plan -- --nocapture
git diff --check
git add Cargo.toml Cargo.lock tools/architecture plan.md docs/evidence/phases-2-5-completion-ledger.md crates/input crates/ui
git commit -m "docs: track amended client completion gates"
git branch completion-bootstrap HEAD
```

Expected: PASS, no whitespace errors, and one focused integration commit.

- [ ] **Step 6: Fast-forward every untouched lane to the bootstrap commit**

Run before dispatching any lane work:

```powershell
git -C ..\completion-phase2 merge --ff-only completion-integration
git -C ..\completion-phase3 merge --ff-only completion-integration
git -C ..\completion-phase4 merge --ff-only completion-integration
git -C ..\completion-phase5 merge --ff-only completion-integration
```

Expected: all four lanes share the workspace/package bootstrap and remain
clean. If any lane has started or cannot fast-forward, stop and reconcile it
before parallel dispatch.

- [ ] **Step 7: Verify the clean bootstrapped baseline before dispatch**

Run from `completion-integration`:

```powershell
cargo build --workspace --locked
cargo test --workspace --all-targets --all-features --locked
cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml
Push-Location core; try { go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'core go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'core go vet failed' } } finally { Pop-Location }
Push-Location tools/fixturegen; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go vet failed' } } finally { Pop-Location }
git status --short
```

Expected: all commands pass and status is empty. If the clean canonical
baseline fails, stop implementation and report the exact pre-existing failure
for user direction, as required by the worktree safety workflow.

### Task 3: Publish and freeze the tranche-A shared interfaces

**Files:**
- Modify through reviewed lane commits only: Phase 2 publication/settings carrier modules
- Modify through reviewed lane commits only: `crates/input/src/action.rs`
- Modify through reviewed lane commits only: `crates/input/src/router.rs`
- Modify through reviewed lane commits only: `crates/assets/src/item.rs`
- Modify through reviewed lane commits only: Phase 4 actor-pose and item-icon carrier modules
- Modify through reviewed lane commits only: `crates/ui/src/action.rs`
- Modify through reviewed lane commits only: Phase 5 draw-list/settings carrier modules
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `tools/architecture/policy.toml`

**Interfaces:**
- Consumes: producer tasks from the Phase 2, Phase 3, Phase 4, and Phase 5 plans.
- Produces: frozen semantic input, item identity, UI action, settings, actor
  pose, and render-list types for tranche B.

- [ ] **Step 0: Dispatch the checkpoint-1 producers with three-agent concurrency**

Start Phase 2 Tasks 1-3, Phase 3 Task 6 only, and Phase 4 Tasks 1-4 in their
fresh worktrees. Phase 3 Task 6 must start from exact `completion-bootstrap`
before Tasks 1-5 add any other Phase 3 history; it pins
`completion-phase3-semantic-interface`. After Step 1 merges that dependency,
fast-forward the untouched Phase 5 lane and dispatch its Tasks 1-5 while the
Phase 3 lane continues Tasks 1-5. Phase 2 stops after its behavior-neutral
interface/runner handoff; Phase 4 stops after
`completion-phase4-actor-interface`. Phase 3 runs Task 7 only after the exact
Phase 4 item/actor and Phase 5 UI refs exist, then stops at
`completion-phase3-interface`. Keep one root integration/review owner and at
most three implementation agents active. Every task uses RED/GREEN, a focused
commit, a generated review package, and independent spec/quality approval
before its immutable checkpoint branch is accepted.

Expected: all six interface refs named below exist and remain immutable; no
lane has edited an integration-owned shared root.

- [ ] **Step 1: Review the Phase 3 semantic input producer commit**

The accepted public surface is exactly the Phase 3 freeze:
`semantic_input::{Action, ActionPhase, ActionSnapshot, InputContext,
ReleaseReason, InputMode, SemanticInputRouter}`. `ActionSnapshot` carries a
monotonic frame sequence, nonzero authority generation, finite unit-circle
movement, bounded finite look delta, an `ActionPhase { pressed, held, released
}` for every catalog action, and `InputMode::{KeyboardMouse, GamePad, Touch}`.
The catalog includes movement/look, jump, sneak, sprint, attack, use,
perspective, distinct menu/back, hotbar 1-9, hotbar previous/next, and UI
navigation/accept/cancel/tab actions.
No integration task may introduce `DigitalAction`, `ActionBits`, or
`SemanticInputFrame` aliases.

Before accepting the producer, also require the exact typed binding catalog,
`replace_bindings`, `ReleaseReason::BindingChanged`, the router-owned UI
context/finalize barrier, and neutral `PerspectiveMode`. `CameraPose` and the
immutable `InteractionOriginSnapshot` remain part of the later full Phase 3
interface freeze; Phase 5 does not depend on them.

Verify the semantic ref contains only Task 6 after `completion-bootstrap`,
merge it through the integration owner, then advance the untouched Phase 5
worktree before its first edit:

```powershell
git show-ref --verify refs/heads/completion-phase3-semantic-interface
$semanticCommits = @(git rev-list --reverse completion-bootstrap..completion-phase3-semantic-interface)
if ($semanticCommits.Count -ne 1) { throw 'semantic interface ref must contain exactly reviewed Task 6' }
git merge --no-ff completion-phase3-semantic-interface -m "merge: freeze semantic input interfaces"
git -C ..\completion-phase5 merge --ff-only completion-integration
```

Expected: Phase 5 now imports the real `semantic_input::ControlSettings` and
`PerspectiveMode`; no UI mirror type or dependency cycle is possible.

- [ ] **Step 2: Review the Phase 4 item identity producer commit**

The accepted public surface must distinguish network identity from visual
fallback without copying NBT into render state:

```rust
pub struct ItemStackIdentity {
    pub network_id: i32,
    pub metadata: u32,
    pub stack_network_id: i32,
    pub count: u16,
    pub nbt_digest: [u8; 32],
}

pub enum ItemVisualRoute {
    Compiled(ItemVisualId),
    BlockItem(BlockVisualId),
    EmptyHand,
    Missing,
}
```

The same producer surface includes a render-owned actor POD boundary and an
immutable item-icon reference containing asset-set identity, page/layer, and
finite UV bounds. `render` does not depend on `client-world`; the app converts
world/actor snapshots into render-owned POD. One normalized protocol equipment
record fans out to remote actor equipment and, only after local runtime identity
is known, local inventory authority. Protocol carries `NetworkItemStack`, never
an assets-owned identity.

- [ ] **Step 3: Review the Phase 5 UI/settings producer commit**

The accepted surface separates user settings from acceptance overrides and
separates UI navigation from raw device keys:

```rust
pub struct UserSettings {
    pub schema_version: u32,
    pub controls: semantic_input::ControlSettings,
    pub video: VideoSettings,
    pub gameplay: GameplaySettings,
}

pub struct GameplaySettings {
    pub default_perspective: semantic_input::PerspectiveMode,
}

pub enum UiAction {
    Navigate([i8; 2]), Accept, Cancel, TabNext, TabPrevious,
    PointerMove { position: UiPoint },
    PointerPrimary { position: UiPoint, phase: PointerPhase },
    PointerSecondary { position: UiPoint, phase: PointerPhase },
    Scroll { delta: UiPoint },
}

pub enum PointerPhase { Pressed, Held, Released }
```

The same checkpoint includes the Phase 2 publication/renderer settings
carriers, the Phase 4 actor-pose and immutable item-icon reference, and the
Phase 5 `UiDrawList`/UI render carrier. Phase 5 video settings reuse the Phase
2-owned cloud and precipitation quality enums; numeric duplicates are rejected.
It also freezes one local action timeline: Phase 5 owns prediction,
confirmation/rejection/cancellation, revision authority, and the canonical
`ItemActionPhase` value in each snapshot. Phase 4 renders that phase verbatim;
it never remaps or advances it. No second clock or provisional-action store is
allowed.

- [ ] **Step 4: Pin and merge producer commits in dependency order**

Each producer lane creates these immutable branches at the exact reviewed
commit. The integration owner verifies they already exist and never recreates
or force-moves them:

```powershell
git show-ref --verify refs/heads/completion-phase2-interface
git show-ref --verify refs/heads/completion-phase3-semantic-interface
git show-ref --verify refs/heads/completion-phase3-interface
git show-ref --verify refs/heads/completion-phase4-item-interface
git show-ref --verify refs/heads/completion-phase4-actor-interface
git show-ref --verify refs/heads/completion-phase5-ui-interface
```

Merge the reviewed interface histories from their common bootstrap base:

```powershell
git merge --no-ff completion-phase2-interface -m "merge: freeze phase 2 publication interfaces"
git merge --no-ff completion-phase4-item-interface -m "merge: freeze phase 4 item interfaces"
git merge --no-ff completion-phase4-actor-interface -m "merge: freeze phase 4 actor interfaces"
git merge --no-ff completion-phase5-ui-interface -m "merge: freeze phase 5 UI interfaces"
git merge --no-ff completion-phase3-interface -m "merge: freeze phase 3 gameplay interfaces"
```

Expected: each merge is conflict-free because producers do not own root
workspace, module-root exports, packet dispatch, or app assembly files. A
conflict means the ownership contract was violated and the producer commit is
reworked rather than hand-merged.

- [ ] **Step 5: Apply workspace edges and run the interface gate**

The bootstrap already registered `crates/input` and `crates/ui`. Add only the
reviewed path dependencies and architecture edges required by the frozen
interfaces, then run:

```powershell
cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml
cargo test -p bedrock-client --locked input
cargo test -p assets --locked item
cargo test -p ui --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: all commands pass with the frozen interfaces and no lane consumer
merged yet.

- [ ] **Step 6: Commit the integration-only workspace wiring**

```powershell
git add Cargo.toml Cargo.lock app tools/architecture
git commit -m "build: freeze client completion interfaces"
```

- [ ] **Step 7: Integrate the behavior-neutral Phase 2 runner handoff**

Apply the exact module exports, app diagnostic adapters, and acceptance-runner
hunks recorded by Phase 2 Task 3. They may observe and report state but may not
change request ordering, retry policy, publication budgets, render constants,
protocol/core wire behavior, or asset schema. Run the focused Phase 2 runner
tests and commit these integration-owned edges separately.

- [ ] **Step 8: Capture immutable canonical checkpoint 0 before dispatch**

Fast-forward `completion-phase2` to this integration commit. Execute Phase 2
Task 3 Steps 6-8 exactly: release/FIFO Lunar diagnostic at
`pvp.lunarbedrock.com:19134`, then diagnostic-complete Zeqa at
`zeqa.net:19132`, each with its unique create-new run ID. Merge only the
sanitized non-final evidence commit back into integration.

Expected: both manifests have complete attributable stage identities and may
record the reproduced defect; neither is binding completion evidence. Fix
selection uses Lunar's first stalled stage, and no behavior lane begins until
this step is reviewed and committed.

### Task 4: Integrate parallel tranche A

**Files:**
- Update: `docs/evidence/phases-2-5-completion-ledger.md`
- Merge reviewed lane commits; do not manually reproduce their feature edits.

**Interfaces:**
- Consumes: Phase 2 diagnostic/2.5 producer, Phase 3 collision/movement
  producer, Phase 4 clip/Molang producer, and Phase 5 UI foundation producer.
- Produces: deterministic-green checkpoint 1.

- [ ] **Step 1: Verify every lane commit is based on the frozen interface tip**

First merge the reviewed integration interface checkpoint into each lane, then
verify ancestry:

```powershell
git -C ..\completion-phase2 merge --no-edit completion-integration
git -C ..\completion-phase3 merge --no-edit completion-integration
git -C ..\completion-phase4 merge --no-edit completion-integration
git -C ..\completion-phase5 merge --no-edit completion-integration
git merge-base --is-ancestor completion-integration completion-phase2
git merge-base --is-ancestor completion-integration completion-phase3
git merge-base --is-ancestor completion-integration completion-phase4
git merge-base --is-ancestor completion-integration completion-phase5
```

Expected: all merges and ancestry checks exit zero. If any feature lane has an
unreviewed shared-file conflict, stop and rework its producer commit; never
rebase or modify an archival branch.

- [ ] **Step 2: Run lane-specific acceptance commands before integration**

Run the exact tranche-A commands recorded in each lane plan. Expected: every
command exits zero and every producer commit has an independent APPROVE review.

- [ ] **Step 3: Merge in lowest-consumer order**

```powershell
git merge --no-ff completion-phase2 -m "merge: phase 2 tranche A"
git merge --no-ff completion-phase3 -m "merge: phase 3 tranche A"
git merge --no-ff completion-phase4 -m "merge: phase 4 tranche A"
git merge --no-ff completion-phase5 -m "merge: phase 5 tranche A"
```

Expected: protocol/assets/input producers merge before app consumers. Resolve
only the explicitly integration-owned manifests, shared dispatch/export roots,
app assembly, acceptance registries, roadmap, and ledger in the integration
lane. Any conflict in a lane-owned feature module returns to its producer.

- [ ] **Step 4: Run checkpoint 1**

```powershell
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml
Push-Location core; try { go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'core go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'core go vet failed' } } finally { Pop-Location }
Push-Location tools/fixturegen; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go vet failed' } } finally { Pop-Location }
git diff --check
```

Expected: zero failures, warnings, formatting changes, architecture violations,
or dirty generated payloads.

- [ ] **Step 5: Update checkpoint evidence and commit**

Replace only tranche-A `Not started` cells with command names, reviewed commit
hashes, and `Deterministic green`; leave live and final status open.

```powershell
git add docs/evidence/phases-2-5-completion-ledger.md
git commit -m "docs: record completion checkpoint one"
```

### Task 5: Integrate parallel tranche B

**Files:**
- Modify: `app/src/app.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `docs/evidence/phases-2-5-completion-ledger.md`

**Interfaces:**
- Consumes: Phase 2 publication/lighting, Phase 3 prediction/network/camera,
  Phase 4 rigs/remote equipment, and Phase 5 HUD/chat/scoreboard/settings UI.
- Produces: one normal app schedule with explicit input/UI/network/render
  authority order.

- [ ] **Step 0: Fast-forward lanes and dispatch tranche B**

Fast-forward every clean lane to checkpoint 1. Run Phase 2 Tasks 4-8, Phase 3
Tasks 8-11, and Phase 4 Tasks 5-9 as the first three task streams; after the
first reviewed stream frees a slot, run Phase 5 Tasks 6-9 and 15-16. The root
integration owner applies each shared call-site/export handoff and returns a
clean integration tip to the affected lane before its next dependent task.
Task 11 Phase 3 remains candidate-only and production-disabled. Phase 4 Task 9
LBSG remains explicitly non-binding.

- [ ] **Step 1: Write the failing schedule-order test**

Add an app test that requires this order:

```rust
const REQUIRED_ORDER: &[&str] = &[
    "collect_raw_input",
    "sample_semantic_input",
    "adjudicate_ui_authority",
    "finalize_semantic_input_and_releases",
    "advance_local_physics",
    "resolve_camera_pose",
    "resolve_interactions",
    "publish_world_frame",
    "publish_actor_render_frame",
    "build_ui_draw_list",
    "send_player_auth_inputs",
    "send_interaction_packets",
];
```

The test inspects registered system sets or a checked schedule descriptor; it
must not rely on source-string ordering.

- [ ] **Step 2: Run the test and verify it fails before app wiring**

```powershell
cargo test -p bedrock-client --locked schedule_order -- --nocapture
```

Expected: FAIL listing unregistered sets or missing edges.

- [ ] **Step 3: Merge reviewed tranche-B producer commits**

Merge the four lane branches using `--no-ff` in Phase 2, Phase 3, Phase 4,
Phase 5 order. Do not accept a lane commit that edits app assembly or a root
manifest; transplant that hunk into the integration-owned wiring commit.

- [ ] **Step 4: Wire explicit system sets**

The app integration must expose and order these sets:

```rust
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientFrameSet {
    RawInput,
    SemanticSample,
    UiAuthority,
    SemanticFinalize,
    Physics,
    Camera,
    Interaction,
    WorldPublication,
    ActorPublication,
    UiPublication,
    NetworkSend,
}
```

Configure them with `.chain()` only where ordering is a correctness contract;
keep independent publication/render preparation parallel inside its set.

- [ ] **Step 5: Run checkpoint 2**

```powershell
cargo test -p bedrock-client --locked schedule_order -- --nocapture
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml
Push-Location core; try { go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'core go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'core go vet failed' } } finally { Pop-Location }
Push-Location tools/fixturegen; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go vet failed' } } finally { Pop-Location }
```

Expected: all commands pass. UI focus tests prove semantic releases reach
physics/network before the next tick; camera tests prove perspective never
changes movement or interaction origin.

- [ ] **Step 6: Commit integration wiring and evidence**

```powershell
git add app Cargo.toml Cargo.lock tools/architecture docs/evidence/phases-2-5-completion-ledger.md
git commit -m "feat: integrate client gameplay and UI authority"
```

### Task 6: Integrate tranche C and binding live features

**Files:**
- Modify: `app/src/app.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `docs/evidence/phases-2-5-completion-ledger.md`

**Interfaces:**
- Consumes: final Phase 2 parity/publication, Phase 4.5 viewmodel/dropped item,
  Phase 5.5 interaction/combat/inventory, Phase 5.6 forms, and Phase 5.8 live
  settings/persistence commits.
- Produces: feature-complete release candidate before binding evidence.

- [ ] **Step 1: Execute and merge the authority/presentation/consumer chain without a cycle**

Fast-forward every clean lane to checkpoint 2. Phase 5 runs Tasks 10-13 and
pins `completion-phase5-authority` after inventory, selected-stack,
combat/block/item/container authority and the shared local action timeline.
Merge it first. Phase 4 then fast-forwards, runs Task 10, and pins
`completion-phase4-presentation` after the viewmodel and dropped-item
presentation; merge it second. Phase 5 fast-forwards again, runs Tasks 14,
17, and 18 against that presentation, and pins `completion-phase5-consumer`.
In the remaining task slots, Phase 2 runs Tasks 9-12, Phase 3 runs Tasks 12-14
including candidate evidence and the separate normal-path enable, and Phase 4
runs Tasks 11-12. Merge in this dependency order:

```powershell
git merge --no-ff completion-phase5-authority -m "merge: phase 5 inventory and interaction authority"
git merge --no-ff completion-phase4-presentation -m "merge: phase 4 held-item and dropped-item presentation"
git merge --no-ff completion-phase5-consumer -m "merge: phase 5 presentation witnesses and live settings"
git merge --no-ff completion-phase2 -m "merge: phase 2 visual and publication completion"
git merge --no-ff completion-phase3 -m "merge: phase 3 movement controls and camera completion"
git merge --no-ff completion-phase4 -m "merge: phase 4 entity item and actor completion"
```

Expected: the Phase 4 viewmodel consumes the already-integrated authoritative
selected-stack interface; it does not invent a second local inventory store.
Phase 3 normal physics authority is merged only after its candidate/live gates,
and every lane's live evidence remains a lane-specific prerequisite rather
than a substitute for Task 7's one-build cross-system binding rerun. Phase 2
Task 13 is deliberately deferred until the assembled release candidate: its
full integration matrix runs as part of Task 7 against this single build, and
its evidence is recorded with the binding ledger rather than being claimed by
the isolated Phase 2 lane.

- [ ] **Step 2: Prove vanilla-only combat statically**

Add an architecture test whose forbidden production identifiers are:

```rust
const FORBIDDEN_COMBAT_IDENTIFIERS: &[&str] = &[
    "ReachModule", "ExtendedReach", "ReachFluctuation", "AutoClicker",
    "KillAura", "TargetEnlargement", "LunarCombat",
];
```

The allowed test/fixture prose exception is limited to the approved spec and
architecture test itself. Expected: production source contains none of these
symbols and attack reach comes only from the reviewed game-mode policy.

- [ ] **Step 3: Run the integrated deterministic gate**

```powershell
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml
Push-Location core; try { go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'core go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'core go vet failed' } } finally { Pop-Location }
Push-Location tools/fixturegen; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go vet failed' } } finally { Pop-Location }
Push-Location tools/chunkfix; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'chunkfix go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'chunkfix go vet failed' } } finally { Pop-Location }
Push-Location tools/bedsimtrace; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'bedsimtrace go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'bedsimtrace go vet failed' } } finally { Pop-Location }
git diff --check
```

Expected: every command passes and the worktree is clean after committing only
source, tests, plans, and reviewed evidence manifests.

- [ ] **Step 4: Commit final integration wiring**

```powershell
git add app Cargo.toml Cargo.lock tools/architecture docs/evidence/phases-2-5-completion-ledger.md
git commit -m "feat: assemble full client completion candidate"
```

### Task 7: Run the binding server and native-client matrix

**Files:**
- Modify: `docs/evidence/phases-2-5-completion-ledger.md`
- Create only from sanitized templates:
  `docs/evidence/runs/phase-completion-binding/manifest.json`

**Interfaces:**
- Consumes: clean release candidate and exact lane acceptance commands.
- Produces: authoritative live/native evidence for every ledger entry.

- [ ] **Step 1: Build one release candidate and record its identity**

```powershell
cargo build --workspace --release --locked
git rev-parse HEAD
git status --short
```

Expected: release build succeeds, commit hash is recorded in the run manifest,
and status is empty.

- [ ] **Step 2: Run Lunar first**

Use `pvp.lunarbedrock.com:19134` with the normal authenticated core. Require:

```json
{
  "current_position_holes": 0,
  "persistent_visible_stalls": 0,
  "full_view_remesh_ms_max": 2000,
  "combat_mode": "vanilla",
  "extended_reach_attacks": 0,
  "unexplained_diagnostics": 0
}
```

Capture spawn/current publication, lighting, movement, HUD, perspectives,
held item, one permitted ordinary attack/use interaction, RSS, CPU, and frame
time. Redact tokens and player-identifying chat before retaining evidence.

- [ ] **Step 3: Run Zeqa after Lunar passes**

Use `zeqa.net:19132`. Require authenticated transfer, current chunks,
movement, chat/forms where available, UI focus, settings changes, perspective,
held item, vanilla combat, reconnect, and clean disconnect with the same zero
hole/stall/diagnostic requirements.

- [ ] **Step 4: Run the LBSG binding actor witness**

Use `play.lbsg.net:19132` after Phase 4.5 actor-facing rendering is stable.
Capture spawn, ordinary movement, rotation, teleport, equipment/action pose,
three-tick convergence, adjacent-frame interpolation, and both feet on the
same ground plane without a 1.6-block jump. The binding artifact also includes
one native-matched animated non-player mob; player-only evidence cannot close
the 4.3 native gate.

- [ ] **Step 5: Run controlled local BDS matrices**

Run deterministic biome/lighting/movement galleries plus a two-client matrix
that cycles nine hotbar slots, attacks one valid entity, misses once, places
and uses items, forces one rejected stack request, drops/picks up an item,
opens every container/form type, cycles all camera modes, holds jump through
multiple landings, rebinds active controls, persists settings across restart,
respawns, and changes dimension.

Repeat the focus-transition script through keyboard/mouse, controller, and
touch. During open chat, inventory, scoreboard, and settings overlays, force a
radius-16 join/teleport publication burst and one full-view remesh. Require
publication budgets never fall below their configured minima, current-position
holes stay zero, UI remains responsive, and the frame/resource manifest passes.

Expected: every provisional state converges to server authority; no stale item,
duplicate attack, phantom jump, stuck key, inventory duplication, or local
avatar visibility error remains.

- [ ] **Step 6: Capture matching native references**

Match resource pack, world state, camera, FOV, time, weather, UI scale, aspect
ratio, held item, and action phase. Retain adjudicated results for biome tint,
lighting, sky/celestial/cloud/fog, UI, first-/third-person camera, viewmodel,
swing/use, combat target/reach, controls, and settings.

- [ ] **Step 7: Fill evidence by identity, not prose assertion**

Every ledger cell names the exact run manifest, capture ID, metric key,
reviewed commit, and native comparison. Do not mark a gate complete from visual
inspection without its matching state identities.

### Task 8: Final verification and completion review

**Files:**
- Modify: `docs/evidence/phases-2-5-completion-ledger.md`
- Modify: `plan.md`

**Interfaces:**
- Consumes: all deterministic, live, native, performance, and resource proof.
- Produces: a clean review-ready completion branch or an explicit open blocker.

- [ ] **Step 1: Re-run the entire clean-worktree gate**

```powershell
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml
Push-Location core; try { go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'core go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'core go vet failed' } } finally { Pop-Location }
Push-Location tools/fixturegen; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'fixturegen go vet failed' } } finally { Pop-Location }
Push-Location tools/chunkfix; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'chunkfix go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'chunkfix go vet failed' } } finally { Pop-Location }
Push-Location tools/bedsimtrace; try { $env:GOWORK='off'; go test ./... -count=1; if ($LASTEXITCODE -ne 0) { throw 'bedsimtrace go test failed' }; go vet ./...; if ($LASTEXITCODE -ne 0) { throw 'bedsimtrace go vet failed' } } finally { Pop-Location }
git diff --check
git status --short
```

Expected: all commands pass and status is empty.

- [ ] **Step 2: Validate final numeric gates**

Create and review
`scripts/acceptance/config/phases-2-5-performance.json`; every run manifest
records its hash, the reference machine/GPU/display, release binary hash, a
30-second warm-up, and a 120-second steady sample. The final evidence reducer
must assert:

```json
{
  "full_view_remesh_ms_max": 2000,
  "join_settle_ms_max": 2000,
  "teleport_settle_ms_max": 2000,
  "p95_frame_ms_max": 16.6666666667,
  "p99_frame_ms_max": 16.6666666667,
  "max_frame_ms_max": 50,
  "combined_steady_rss_mb_max": 650,
  "combined_steady_cpu_percent_max": 15,
  "persistent_current_position_holes": 0,
  "extended_reach_or_automated_attacks": 0,
  "unexplained_diagnostics": 0,
  "required_gpu_consecutive_exact_frames": 2
}
```

Expected: nonzero exit if any value is absent, unattributed, or above its
ceiling.

- [ ] **Step 3: Request independent code and evidence review**

Use `superpowers:requesting-code-review` against the full behavior range from
`b294008` to `completion-integration`. Require review of correctness, bounds,
authority, vanilla combat, privacy, native adjudication, and evidence identity.

- [ ] **Step 4: Close only proven roadmap entries**

Change a checkbox to `[x]` and ledger status to `Complete` only when every
deterministic/live/native/performance cell is authoritative. Any missing gate
stays open and prevents the final completion claim.

- [ ] **Step 5: Commit final evidence**

```powershell
git add plan.md docs/evidence
git commit -m "docs: record verified client completion"
git status --short
```

Expected: a clean integration worktree and no unexplained deferrals.

## Self-Review

- Spec coverage: all Phase 2, Phase 3, Phase 4, Phase 5, gameplay amendment,
  worktree preservation, parallelization, and final evidence requirements map
  to a lane plan and an integration task.
- Dependency order: semantic input/item/settings interfaces precede consumers;
  selected-stack authority precedes the local viewmodel; actor-facing 4.5
  precedes the binding 4.4 witness; Lunar precedes Zeqa chunk acceptance.
- Ownership: root manifests, app assembly, architecture policy, roadmap, and
  ledger have exactly one integration owner.
- Safety: archival worktrees and user changes are read-only; credentials and
  private live payloads are excluded from evidence.
- Completion: missing live/native/performance evidence remains an open gate,
  never a deferral or inferred pass.
