# Architecture Decomposition Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Cinnabar's giant Rust and PowerShell files with durable crate and module ownership boundaries while preserving behavior, performance contracts, test coverage, and acceptance evidence.

**Architecture:** Extract offline asset compilation, pure CPU meshing, and the headless client-world pipeline into acyclic crates. Keep Bevy/WGPU chunk rendering in one crate but split it by ECS-owned state machines, make the app a thin composition library/binary, split the PowerShell harness by domain, and enforce the resulting structure with a cross-platform architecture policy tool.

**Tech Stack:** Rust 1.93.1 edition 2024, Cargo workspace, Bevy 0.18.1/WGPU 27, Rayon/crossbeam, Go 1.26, Windows PowerShell 5.1, Bash, GitHub Actions.

## Global Constraints

- Preserve all current runtime behavior, evidence contracts, performance properties, stable Windows executable paths, and native acceptance semantics.
- Backward compatibility for internal crate/module paths, aliases, and test helpers is not required; all workspace consumers move atomically.
- Keep Mojang assets, screenshots, BDS runtimes, and generated local evidence out of git.
- `assets` owns runtime types plus blob encoding and decoding; runtime crates never depend on `asset-compiler`.
- `meshing` and `client-world` have no normal Bevy or WGPU dependency.
- Keep the renderer in one crate and use explicit re-exports; never add a glob re-export or project prelude.
- Each tranche receives one focused independent review. Fix all Critical and Important findings before the next dependent tranche.
- Mechanical moves and semantic changes land in separate commits.
- On macOS, run Go tests with `TMPDIR=/private/tmp` so auth-cache parent validation and Unix-socket length constraints both remain valid.
- Keep this worktree's default Cargo `target` directory isolated from every other worktree.

---

## Target file ownership

```text
crates/assets/                    runtime contracts, MCBEAS codec, RuntimeAssets
crates/asset-compiler/            pack/image input, compiler, assetc
crates/meshing/                   CPU biome/light/liquid/cloud/chunk geometry
crates/client-world/              world event ingestion, residency, scheduling
crates/render/src/chunk/          Bevy/WGPU chunk renderer
app/src/runtime/                  normal application systems
app/src/acceptance/               deterministic acceptance state and markers
scripts/acceptance/               dot-sourced PowerShell libraries
scripts/tests/acceptance/         split PowerShell test libraries/cases
tools/architecture/               dependency and source-layout policy checker
```

## Task 1: Record baselines and commit the execution plan

**Files:**
- Create: `docs/superpowers/plans/2026-07-16-architecture-decomposition.md`

**Interfaces:**
- Consumes: approved `docs/superpowers/specs/2026-07-16-architecture-decomposition-design.md`.
- Produces: the ordered task contract used by every later tranche.

- [x] **Step 1: Verify the isolated worktree build**

Run: `cargo build --workspace --locked`

Expected: exit 0 using this worktree's own `target` directory.

- [x] **Step 2: Record the behavioral baseline**

Run:

```bash
cargo test --workspace --locked
TMPDIR=/private/tmp go test ./core/...
bash scripts/tests/acceptance_test.sh
```

Expected: all available tests pass. Ignored real-pack/release tests remain ignored for their documented reasons.

- [x] **Step 3: Self-review this plan**

Run:

```bash
rg -n 'TBD|TODO|implement later|Similar to Task|appropriate error handling' docs/superpowers/plans/2026-07-16-architecture-decomposition.md
git diff --check
```

Expected: the placeholder search prints nothing and diff check exits 0.

- [x] **Step 4: Commit the plan**

```bash
git add docs/superpowers/plans/2026-07-16-architecture-decomposition.md
git commit -m "docs: plan architecture decomposition"
```

## Task 2: Split the PowerShell acceptance harness without changing behavior

**Files:**
- Modify: `scripts/acceptance.ps1`
- Create: `scripts/acceptance/Load.ps1`
- Create: `scripts/acceptance/Common.ps1`
- Create: `scripts/acceptance/RuntimePaths.ps1`
- Create: `scripts/acceptance/Process.ps1`
- Create: `scripts/acceptance/Bds.ps1`
- Create: `scripts/acceptance/Markers.ps1`
- Create: `scripts/acceptance/Proofs.ps1`
- Create: `scripts/acceptance/Resources.ps1`
- Create: `scripts/acceptance/Metrics.ps1`
- Create: `scripts/acceptance/Galleries/Common.ps1`
- Create: `scripts/acceptance/Galleries/Leaves.ps1`
- Create: `scripts/acceptance/Galleries/CrossCrop.ps1`
- Create: `scripts/acceptance/Galleries/Aquatic.ps1`
- Create: `scripts/acceptance/Galleries/Water.ps1`
- Create: `scripts/acceptance/Galleries/FlowerBed.ps1`
- Create: `scripts/acceptance/Galleries/SlabStair.ps1`
- Create: `scripts/acceptance/Galleries/Vine.ps1`
- Create: `scripts/acceptance/Orchestrator.ps1`
- Modify: `scripts/tests/acceptance.Tests.ps1`
- Create: `scripts/tests/acceptance/Assertions.ps1`
- Create: `scripts/tests/acceptance/Fixtures.ps1`
- Create: `scripts/tests/acceptance/Paths.Tests.ps1`
- Create: `scripts/tests/acceptance/Galleries.Tests.ps1`
- Create: `scripts/tests/acceptance/Markers.Tests.ps1`
- Create: `scripts/tests/acceptance/Metrics.Tests.ps1`
- Create: `scripts/tests/acceptance/Orchestration.Tests.ps1`

**Interfaces:**
- Consumes: the existing parameter block, `RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY`, and all current function names.
- Produces: `Get-AcceptanceCompositeSource -EntryPath [string]` returning entry plus libraries in canonical load order; `Invoke-CinnabarAcceptance` containing the former top-level runtime flow.

- [x] **Step 1: Add a failing composite-source contract test**

Add a test case to `scripts/tests/acceptance.Tests.ps1` that calls:

```powershell
$source = Get-AcceptanceCompositeSource -EntryPath $AcceptanceScript
Assert-True ($source.IndexOf('function ConvertTo-CommandArgument') -lt $source.IndexOf('function Start-LoggedProcess')) 'composite source order changed'
Assert-True ($source.IndexOf('function Start-LoggedProcess') -lt $source.IndexOf('function Assert-AcceptanceMetrics')) 'composite source order changed'
```

Run: `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/acceptance.Tests.ps1` on Windows, or use the existing source-contract checks when PowerShell is unavailable locally.

Expected before implementation: failure because `Get-AcceptanceCompositeSource` is undefined.

- [x] **Step 2: Implement the deterministic loader seam**

`scripts/acceptance/Load.ps1` defines:

```powershell
function Get-AcceptanceLibraryPaths {
    param([Parameter(Mandatory = $true)][string]$EntryPath)
    $root = Join-Path (Split-Path -Parent $EntryPath) 'acceptance'
    return @(
        'Common.ps1', 'RuntimePaths.ps1', 'Process.ps1', 'Bds.ps1',
        'Markers.ps1', 'Galleries\Common.ps1', 'Galleries\Leaves.ps1',
        'Galleries\CrossCrop.ps1', 'Galleries\Aquatic.ps1', 'Galleries\Water.ps1',
        'Galleries\FlowerBed.ps1', 'Galleries\SlabStair.ps1', 'Galleries\Vine.ps1',
        'Proofs.ps1', 'Resources.ps1', 'Metrics.ps1', 'Orchestrator.ps1'
    ) | ForEach-Object { Join-Path $root $_ }
}

function Get-AcceptanceCompositeSource {
    param([Parameter(Mandatory = $true)][string]$EntryPath)
    $parts = @([IO.File]::ReadAllText($EntryPath))
    $parts += @(Get-AcceptanceLibraryPaths -EntryPath $EntryPath | ForEach-Object { [IO.File]::ReadAllText($_) })
    return $parts -join "`n"
}
```

- [x] **Step 3: Move functions by owner and keep fixed load order**

Use `git mv`/mechanical extraction so function bodies remain byte-for-byte except for indentation required by `Invoke-CinnabarAcceptance`. `acceptance.ps1` dot-sources `Load.ps1`, then every path returned by `Get-AcceptanceLibraryPaths`, returns for library-only mode, and invokes `Invoke-CinnabarAcceptance` otherwise.

- [x] **Step 4: Split the custom PowerShell test script**

Keep `scripts/tests/acceptance.Tests.ps1` as a loader that dot-sources `Assertions.ps1`, fixtures, and each `*.Tests.ps1` in fixed order. Preserve the existing direct-script exit behavior and exact assertions.

- [x] **Step 5: Verify and commit the PowerShell split**

Run:

```bash
bash scripts/tests/acceptance_test.sh
git diff --check
```

On Windows additionally run both acceptance PowerShell scripts. Expected: identical passes and no new output files.

Commit: `refactor: split acceptance harness by domain`

## Task 3: Extract the offline asset compiler

**Files:**
- Create: `crates/asset-compiler/Cargo.toml`
- Create: `crates/asset-compiler/src/lib.rs`
- Move: `crates/assets/src/compiler.rs` to `crates/asset-compiler/src/compiler.rs`
- Move: `crates/assets/src/bin/assetc.rs` to `crates/asset-compiler/src/bin/assetc.rs`
- Move build-only pack/image/animation/atmosphere compilation code into `crates/asset-compiler/src/`
- Modify: `crates/assets/src/lib.rs`
- Modify: `crates/assets/Cargo.toml`
- Modify: `Cargo.toml`
- Modify: `Makefile`
- Modify: `README.md`
- Modify: `app/src/asset_startup.rs`
- Modify: affected Rust tests and Cargo dev-dependencies

**Interfaces:**
- `assets` continues producing `CompiledAssets`, `Material`, `BlockVisual`, texture/model records, `encode_blob`, `RuntimeAssets`, and all format validation.
- `asset_compiler` produces `compile_pack`, `compile_pack_with_biomes`, `inspect_animation_inventory`, and offline atmosphere compilation APIs.
- Binary name remains `assetc`; package selector becomes `-p asset-compiler`.

- [x] **Step 1: Add dependency-boundary RED checks**

Add assertions to the later architecture policy fixture, initially as shell checks:

```bash
cargo tree -p bedrock-client --locked | rg 'asset-compiler' && exit 1 || true
cargo tree -p assets --locked | rg 'clap|image' && exit 1 || true
```

Expected before extraction: the second check fails because offline dependencies remain in `assets`.

- [x] **Step 2: Move runtime data types out of compiler-owned source**

Create focused `assets` modules for runtime material/visual/compiler-output records and their validation. Keep exact public names and binary layouts:

```rust
pub struct Material { pub texture: TextureRef, pub flags: u32, pub animation: u32 }
pub struct BlockVisual { pub faces: [u32; 6], pub flags: BlockFlags, pub kind: VisualKind, pub contributor_role: ContributorRole, pub model_template: u32, pub animation: u32, pub variant: u32 }
pub struct CompiledAssets { /* existing exact fields */ }
```

Run `cargo test -p assets --locked`. Expected: all existing asset tests pass unchanged.

- [x] **Step 3: Create `asset-compiler` and move compiler code mechanically**

The new manifest depends on `assets`, `image`, `clap`, `serde`, `serde_json`, `sha2`, `same-file`, and `thiserror` as required by moved offline code. Update test imports from `assets::compile_pack` to `asset_compiler::compile_pack` and add dev-dependencies only where compiler-backed fixtures are genuinely required.

- [x] **Step 4: Update every assetc invocation atomically**

Run:

```bash
rg -n -- '-p assets|--bin assetc' . -g '!target'
```

Change package selectors to `-p asset-compiler`; retain `--bin assetc`. Update source-string tests in the same commit.

- [x] **Step 5: Verify and commit the crate extraction**

Run:

```bash
cargo fmt --check
cargo test -p assets -p asset-compiler --locked
cargo test -p bedrock-client -p render --locked
cargo tree -p bedrock-client --locked
```

Expected: all pass; normal app dependency tree contains no `asset-compiler`; `assets` no longer has Clap or source-image decoding normal dependencies.

Commit: `refactor: extract offline asset compiler`

## Task 4: Split asset compilation by explicit family ownership

**Files:**
- Create: `crates/asset-compiler/src/materials.rs`
- Create: `crates/asset-compiler/src/geometry/`
- Create: `crates/asset-compiler/src/visuals/mod.rs`
- Create the complete family module set named in the design spec
- Split: `crates/asset-compiler/tests/compiler.rs` into one harness plus `tests/compiler/*.rs`

**Interfaces:**
- `CompileRuleResult = NoMatch | Reject | Compiled(BlockVisual)` preserves exact fail-closed ordering.
- `VisualCompiler` owns material lookup and template interning; family modules receive `&RegistryRecord` plus `&mut VisualCompiler`.

- [x] **Step 1: Add fail-closed dispatcher characterization tests**

For selector aliases, bee housing, cake, and resin, assert malformed owned names produce `Reject` and never reach the generic cube fallback. Assert unrelated names produce `NoMatch`.

Expected before the dispatcher: compile failure because `CompileRuleResult` is absent.

- [x] **Step 2: Introduce the explicit ordered result contract**

```rust
pub(super) enum CompileRuleResult {
    NoMatch,
    Reject,
    Compiled(BlockVisual),
}
```

The dispatcher lists family functions explicitly and stops on `Reject` or `Compiled`.

- [x] **Step 3: Move each family as a cohesive unit**

Move recognizer, exact state parser, complete inventory admission, descriptors, and quad construction together. Keep shared cuboid/rotation operations in `geometry`; keep family-specific constants with their family.

- [x] **Step 4: Split the single compiler integration harness without multiplying binaries**

Retain `tests/compiler.rs` as the Cargo integration target. It declares shared support plus the
exact family files `bee_housing.rs`, `cake.rs`, `cactus.rs`, `farmland.rs`,
`selector_alias.rs`, `resin_clump.rs`, `bookshelf.rs`, `transparent_cube.rs`, `liquid.rs`,
`flowerbed.rs`, `signs.rs`, `vine.rs`, `multiface.rs`, `doors.rs`, `walls.rs`,
`pressure_plates.rs`, `buttons.rs`, `carpets.rs`, `gates.rs`, `panes.rs`, `fences.rs`,
`slabs.rs`, `stairs.rs`, `kelp.rs`, `cross.rs`, and `fallback_cube.rs` via explicit
`#[path = "compiler/family.rs"] mod family;` declarations. Shared fixture construction lives in
`tests/compiler/support.rs`.

- [x] **Step 5: Verify deterministic compiler parity and commit**

Run:

```bash
cargo fmt --check
cargo test -p asset-compiler --locked
cargo test -p assets --locked
```

Expected: exact existing counts, golden bytes, and shuffle determinism pass.

Commit: `refactor: organize asset compiler by visual family`

## Task 5: Extract pure CPU meshing

**Files:**
- Create: `crates/meshing/Cargo.toml`
- Create: `crates/meshing/src/lib.rs`
- Move/split: `crates/render/src/mesh.rs`, `biome.rs`, `lighting.rs`, `liquid.rs`, `cloud_mesh.rs`, `color.rs`
- Move/split corresponding render tests to `crates/meshing/tests/`
- Modify: `crates/render/Cargo.toml`, `crates/render/src/lib.rs`
- Modify: `app/Cargo.toml`, `app/src/world_stream.rs`, and all consumers

**Interfaces:**
- `meshing` exports existing packed mesh types and functions under canonical names.
- Move `CameraMedium` to `meshing::liquid` and `ChunkBiomeTintIdentity` to `meshing::biome`.
- `render` consumes `ChunkMesh` and CPU identity contracts but does not re-export them when the tranche closes.

- [ ] **Step 1: Add dependency-boundary RED checks**

Run after scaffolding an empty crate:

```bash
cargo tree -p meshing --locked | rg 'bevy|wgpu'
```

Expected: no output; compilation remains red until moved APIs exist.

- [ ] **Step 2: Move pure modules and tests mechanically**

Preserve public signatures and packed layout assertions. Split the former `mesh.rs` into `types`, `classifier`, `contributors`, `connectivity`, and `chunk::{build,opaque,models,liquids}` only after the crate move is green.

- [ ] **Step 3: Flip every consumer atomically**

Replace CPU imports from `render` with `meshing` in app, render, and tests. `render::lib` exports only renderer-owned APIs when complete.

- [ ] **Step 4: Verify and commit**

Run:

```bash
cargo fmt --check
cargo test -p meshing -p render -p bedrock-client --locked
cargo tree -p meshing --locked | rg 'bevy|wgpu' && exit 1 || true
```

Commit: `refactor: extract pure cpu meshing`

## Task 6: Split the Bevy chunk renderer by owned state machine

**Files:**
- Replace: `crates/render/src/plugin.rs`
- Create: all `crates/render/src/chunk/**` files specified by the design
- Modify: `crates/render/src/lib.rs`
- Split: `crates/render/tests/plugin.rs` into a single harness plus subsystem modules

**Interfaces:**
- `ChunkRenderPlugin` replaces `DebugWorldPlugin` across the workspace without an alias.
- `chunk::api` owns application-facing queue/instance/acknowledgement/view contracts.
- `gpu::types` owns private allocation identities shared by upload, drawing, transparency, and presentation.

- [ ] **Step 1: Rename the plugin workspace-wide**

Change `DebugWorldPlugin` to `ChunkRenderPlugin`, run render and app tests, and commit the semantic-free rename with no alias.

- [ ] **Step 2: Move plugin wiring and public contracts**

Create `chunk::mod` and `chunk::plugin`; move queue/API types first. Update all `load_internal_asset!` paths relative to the new source file, using `../` as required.

- [ ] **Step 3: Move GPU preparation and allocation**

Move arena math, stream layout, buffer growth, bind groups, and upload driver into `gpu`. Use `pub(super)` or `pub(crate)` only for demonstrated sibling consumers.

- [ ] **Step 4: Move pipelines, draw submission, transparency, and presentation**

Keep sort state, model sort state, retirement, frame probes, and witness evidence with their owning resources. Do not introduce a shared prelude.

- [ ] **Step 5: Move inline private tests to subsystem child modules**

Tests that exercise private implementation live in `chunk/gpu/tests.rs`,
`chunk/transparent/tests.rs`, and `chunk/presentation/tests.rs`; public shader, pipeline, and
queue contracts remain in the one integration harness.

- [ ] **Step 6: Verify and commit**

Run:

```bash
cargo fmt --check
cargo test -p render --locked
cargo test -p bedrock-client --locked
```

Expected: all shader parsing, noop-WGPU, direct/MDI, queue, witness, and presentation tests pass.

Commit: `refactor: decompose chunk renderer`

## Task 7: Extract and decompose the headless client-world pipeline

**Files:**
- Create: `crates/client-world/Cargo.toml`
- Create: all `crates/client-world/src/**` modules specified by the design
- Move: `app/src/world_stream.rs`, `actor_store.rs`, `block_entity_visuals.rs`, `server_position.rs`
- Modify: `app/Cargo.toml`, `app/src/main.rs`, and app tests

**Interfaces:**
- `client_world::WorldStream` retains the current behavioral public methods.
- The app defines `#[derive(Resource)] struct ClientWorld` and stores `Option<WorldStream>`; client-world itself has no Bevy normal dependency.
- Scheduler fields remain one `WorldStream` state during this refactor; no `RequestTracker`/`LightEngine` semantic regrouping occurs.
- State-machine modules live beneath `stream/` so their `impl WorldStream` blocks retain access to
  private fields without widening the entire scheduler state to `pub(crate)`.

- [ ] **Step 1: Create the Bevy-free crate and move private collaborators**

Move actor, block-entity, and server-position behavior with their tests. Split the oversized actor
store by actor lifecycle/query ownership during the move. Export only types consumed by the app.

- [ ] **Step 2: Move `WorldStream` mechanically**

Remove the `Resource` derive/import. Add dependencies on protocol, world, assets, meshing, crossbeam-channel, rayon, and thiserror. Flip app imports atomically.

- [ ] **Step 3: Split methods by state-machine ownership**

Keep `WorldStream` field definitions in `stream.rs`. Move coherent `impl WorldStream` blocks to sequencing/admission, decode, residency, requests, cohort, meshing, lighting, actors/block entities, and stats.

- [ ] **Step 4: Partition the mixed Bevy publication test**

Retain private scheduler tests in client-world. Move the cross-layer render/publication contract to the app test harness and drive it through public submission/poll/change/acknowledgement methods. Remove any temporary render/Bevy dev-dependency from client-world before commit.

- [ ] **Step 5: Verify dependency and behavior boundaries**

Run:

```bash
cargo fmt --check
cargo test -p client-world -p bedrock-client -p render --locked
cargo tree -p client-world --locked | rg 'bevy|wgpu' && exit 1 || true
```

Commit: `refactor: extract client world pipeline`

## Task 8: Make the app a thin composition library and isolate acceptance state

**Files:**
- Create: `app/src/lib.rs`
- Create: `app/src/app.rs`
- Create: `app/src/runtime/{endpoint,shutdown,network,world,visibility,telemetry}.rs`
- Create: `app/src/acceptance/{mod,world_ready,teleport,remesh,mutation,proofs,markers}.rs`
- Move: `app/src/{model_witness,transparent_witness}.rs` into app acceptance ownership
- Retain and map: `app/src/{args,asset_startup,camera,movement,environment}.rs`
- Modify: `app/src/main.rs`
- Split: `app/src/metrics.rs`, `app/src/network.rs`, and inline tests

**Interfaces:**
- `bedrock_client::run(ClientArgs) -> anyhow::Result<()>` is the library entry point.
- `main()` parses args, calls `run`, reports fatal error, and returns the existing exit semantics.
- All `RUST_MCBE_*` producer strings live in `acceptance::markers`.
- A checked-in marker expectation table distinguishes parsed evidence from log-only diagnostics.

- [ ] **Step 1: Add the library entry and remove path-based test includes**

Move module declarations to `lib.rs`; make `main.rs` call `bedrock_client::run`. Update integration tests to import the library normally.

- [ ] **Step 2: Move normal runtime systems**

Group endpoint/shutdown, network ingress, world driving, cave visibility, and telemetry/title systems by owner. Keep Bevy schedule ordering explicit in `app.rs`.

- [ ] **Step 3: Move acceptance state and marker construction**

Move trackers, proof functions, completion decisions, and marker serialization into acceptance modules. Normal runtime modules may emit typed observations but do not format acceptance markers.

- [ ] **Step 4: Split app tests beside their owners**

Keep private state-machine tests under each acceptance/runtime child module. Cross-module application graph tests remain integration tests.

- [ ] **Step 5: Verify and commit**

Run:

```bash
cargo fmt --check
cargo test -p bedrock-client --locked
```

Commit: `refactor: separate app runtime and acceptance`

## Task 9: Enforce the architecture and source-size policy

**Files:**
- Create: `tools/architecture/Cargo.toml`
- Create: `tools/architecture/src/main.rs`
- Create: `tools/architecture/policy.toml`
- Create: `tools/architecture/tests/policy.rs`
- Modify: `Cargo.toml`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- `cargo run -p architecture -- check --root . --policy tools/architecture/policy.toml` validates dependency edges, forbidden dependencies, file budgets, re-export patterns, test-only public names, marker ownership, and tracked forbidden artifact patterns.

- [ ] **Step 0: Close every remaining baseline size violation**

Split `crates/assets/src/runtime.rs`, `crates/asset-compiler/src/pack.rs`,
`crates/asset-compiler/tests/pack.rs`, `tools/visualcoverage/src/lib.rs`,
`tools/visualcoverage/tests/ratchet.rs`, and the actor store moved by Task 7. Confirm the complete
first-party handwritten tree meets the final budgets before implementing the strict checker.

- [ ] **Step 1: Write failing policy fixtures**

Create temporary fixture trees proving rejection of an oversized handwritten Rust file, forbidden `client-world -> bevy`, `render -> asset-compiler`, `pub use module::*`, `_for_test` public function, and duplicate `RUST_MCBE_` producer.

- [ ] **Step 2: Implement deterministic policy parsing and checks**

The tool reads only checked-in TOML and repository files, sorts every diagnostic, and exits nonzero on violations. It never shells out to Cargo for source-size checks; dependency validation reads workspace manifests and resolved local path dependencies.

- [ ] **Step 3: Encode final budgets and explicit edges**

Policy values:

```toml
production_rust_max = 1000
module_root_max = 300
powershell_max = 800
test_max = 1200
```

Include explicit edges for bridge, protocol, world, assets, asset-compiler, meshing, client-world,
render, app, sim, visualcoverage, and architecture. Treat `crates/protocol/vendor/` as a first-class
third-party snapshot scope tied to its upstream ownership record, not as a handwritten-code
exception. Marker policy reads the expectation table so log-only diagnostics are not falsely
reported as unpaired.

- [ ] **Step 4: Add the CI gate and verify**

Run:

```bash
cargo test -p architecture --locked
cargo run -p architecture -- check --root . --policy tools/architecture/policy.toml
```

Commit: `ci: enforce architecture boundaries`

## Task 10: Full verification, independent review, and PR publication

**Files:**
- Modify only files required by Critical/Important review findings.

**Interfaces:**
- Produces a clean feature branch and ready-for-review PR targeting `phase2-textures`.

- [ ] **Step 1: Run formatting, tests, lint, and static checks**

```bash
cargo fmt --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
TMPDIR=/private/tmp go test ./core/...
TMPDIR=/private/tmp go vet ./core/...
bash scripts/tests/acceptance_test.sh
git diff --check
```

Expected: all commands pass. Windows-only PowerShell/native acceptance is reported precisely if not runnable from this host.

- [ ] **Step 2: Run one focused independent review cycle**

Review the complete branch against the design spec, dependency policy, test preservation, public API cleanliness, and file ownership. Fix every Critical and Important finding; rerun affected tests. Do not begin a second review loop unless a fix changes production behavior.

- [ ] **Step 3: Rebase onto current remote base without force-pushing**

```bash
git fetch origin phase2-textures
git rebase origin/phase2-textures
```

Resolve only branch-relevant conflicts and rerun the full verification gate.

- [ ] **Step 4: Push and open a ready PR**

```bash
git push -u origin refactor/architecture-decomposition
gh pr create --base phase2-textures --head refactor/architecture-decomposition --title "refactor: decompose client architecture" --body-file /private/tmp/cinnabar-architecture-decomposition-pr.md
```

The PR body summarizes crate boundaries, module decomposition, behavior preservation, verification commands, review results, and any Windows-only checks left to CI/native hardware. The PR is not a draft.
