# Architecture Decomposition Design

## Goal

Replace Cinnabar's giant source files with durable ownership boundaries that keep production
modules understandable, independently testable, and resistant to renewed growth. Preserve all
current behavior, evidence contracts, performance properties, executable paths, and native
acceptance semantics while reorganizing the implementation. Backward compatibility for internal
module paths, crate names, type aliases, or test helpers is not required; consumers in this
workspace move atomically to the canonical API.

This design covers the current structural hotspots:

- `crates/render/src/plugin.rs`
- `app/src/world_stream.rs`
- `app/src/main.rs`
- `crates/assets/src/compiler.rs`
- their large unit and integration tests
- `scripts/acceptance.ps1` and its large Pester suite

The work is structural, not cosmetic. Files split by owned state machine or domain behavior, and
crate extraction occurs only where the existing dependency graph demonstrates an independent
consumer boundary.

## Architecture principles

Cinnabar separates shared runtime contracts from client-only world processing and presentation:

- `world`, `protocol`, and `assets` own shared runtime contracts.
- `client-world` owns client-only world ingestion and publication.
- `render` owns client-only GPU presentation.
- `app` owns executable composition and acceptance control.

Cinnabar does not introduce broad `common` or `util` buckets. Every module must have a named
domain owner, and reusable code moves only when at least two real consumers share the same
contract.

## Selected dependency architecture

The target runtime dependency graph is acyclic:

```text
bridge -> protocol

world
assets                         runtime format, codec, and validated runtime data
  ^
  `-- asset-compiler           offline compiler and assetc

assets + world -> meshing      pure CPU geometry; no Bevy or WGPU

protocol + world + assets + meshing
  `-- client-world             event normalization, residency, and scheduling

assets + world + meshing + Bevy/WGPU
  `-- render                   GPU resources, extraction, queuing, and presentation

client-world + render + Bevy
  `-- app                      executable composition and acceptance control
```

`bridge` remains consumed by `protocol`. `sim` remains a deliberately parked standalone
workspace crate until a separate feature integrates or removes it. `tools/visualcoverage` stays
an explicit tooling consumer of runtime asset contracts. These edges appear in the repository's
architecture allowlist rather than being omitted as accidental exceptions.

The following alternatives are rejected:

1. In-place file splitting without crate-boundary corrections would leave CPU meshing owned by
   `render`, world streaming owned by the Bevy executable, and offline compilation on the runtime
   dependency path. Those are the causes of continued hotspot growth.
2. Splitting the Bevy renderer into several crates would force private ECS render-world state
   through public APIs and encourage a renderer-wide prelude.
3. Moving meshing into `world` would add an `assets` dependency to the foundational world-model
   crate.
4. Feature-gating the compiler inside `assets` would remain vulnerable to workspace feature
   unification and would not create a clear ownership boundary.
5. Turning acceptance libraries into `.psm1` modules would change Windows PowerShell 5.1 scope
   and closure behavior.

## Runtime assets and offline compilation

### `assets` ownership

`crates/assets` becomes the runtime format and validation crate. It retains:

- `TextureArray`, `TextureMip`, `TexturePage`, and `TextureRef`
- `CompiledAssets`, `BlockVisual`, `Material`, model and animation runtime records
- flags, limits, format magic, versions, and cross-reference validation
- blob encoding and decoding as one versioned format contract
- `RuntimeAssets` and runtime resolution APIs
- shared registry/model value types used by runtime consumers

Blob encoding remains beside decoding. Tests and tools can create synthetic runtime blobs without
depending on the offline compiler, and format version changes cannot diverge across two crates.

### `asset-compiler` ownership

Create `crates/asset-compiler` as an offline leaf depending on `assets`. It owns:

```text
crates/asset-compiler/src/
|-- lib.rs
|-- compiler.rs
|-- materials.rs
|-- image_decode.rs
|-- pack/
|-- geometry/
|-- visuals/
|   |-- mod.rs
|   `-- families/
|       |-- bee_housing.rs
|       |-- cake.rs
|       |-- cactus.rs
|       |-- farmland.rs
|       |-- selector_alias.rs
|       |-- resin_clump.rs
|       |-- bookshelf.rs
|       |-- transparent_cube.rs
|       |-- liquid.rs
|       |-- flowerbed.rs
|       |-- signs.rs
|       |-- vine.rs
|       |-- multiface.rs
|       |-- doors.rs
|       |-- walls.rs
|       |-- pressure_plates.rs
|       |-- buttons.rs
|       |-- carpets.rs
|       |-- gates.rs
|       |-- panes.rs
|       |-- fences.rs
|       |-- slabs.rs
|       |-- stairs.rs
|       |-- kelp.rs
|       |-- cross.rs
|       `-- fallback_cube.rs
`-- bin/assetc.rs
```

It owns pack traversal, PNG/TGA source decoding, material compilation, family compilation, and
the `assetc` binary. It produces runtime structures and calls `assets::encode_blob`.

Each visual-family module keeps its recognizer, typed state parsing, complete-inventory admission,
material descriptors, and quad construction together. Shared geometry contains only proven
primitives such as cuboids and rotation math. It must not become a generic family prelude.

The initial crate extraction moves the compiler unchanged. A later, separately tested semantic
tranche introduces an explicit ordered dispatcher with:

```text
NoMatch   the family does not own this record
Reject    the family owns the name/state space but exact admission failed
Compiled  the family produced the complete visual
```

That distinction preserves the current fail-closed branches and prevents malformed exact-family
records from reaching generic fallbacks. The dispatcher remains an explicit ordered list; there
is no dynamic registry, trait-object graph, or plugin mechanism.

The package continues producing a binary named `assetc`. All `cargo run -p assets --bin assetc`
references in the Makefile, README, application startup copy, and tests change atomically to the
new package. The migration checklist searches the complete repository for `-p assets` and
`--bin assetc` before the commit closes.

## Pure CPU meshing

Create `crates/meshing` for Bevy-free geometry and sampling:

```text
crates/meshing/src/
|-- lib.rs
|-- types.rs
|-- classifier.rs
|-- contributors.rs
|-- connectivity.rs
|-- biome.rs
|-- lighting.rs
|-- liquid.rs
|-- cloud.rs
`-- chunk/
    |-- mod.rs
    |-- build.rs
    |-- opaque.rs
    |-- models.rs
    `-- liquids.rs
```

Move the current pure `mesh`, `lighting`, `biome`, `liquid`, `cloud_mesh`, and `color` behavior
from `render`. `CameraMedium` moves from the Bevy-owning atmosphere module into `meshing::liquid`,
beside `sample_camera_medium`. `ChunkBiomeTintIdentity` moves from the GPU plugin into
`meshing::biome`, because client-world creates that identity while render only consumes it.

The meshing API exposes immutable input contracts and packed output products. It does not expose
worker queues, Bevy components, GPU allocation identities, or renderer state. The extraction and
all workspace import changes land atomically on the canonical `meshing` paths. `render` does not
retain compatibility re-exports.

## Client-world pipeline

Create `crates/client-world` for the client-only authoritative world pipeline:

```text
crates/client-world/src/
|-- lib.rs
|-- stream.rs
|-- sequencing.rs
|-- admission.rs
|-- decode.rs
|-- residency.rs
|-- requests.rs
|-- cohort.rs
|-- meshing.rs
|-- lighting/
|   |-- mod.rs
|   |-- snapshot.rs
|   `-- scheduler.rs
|-- actors.rs
|-- block_entities.rs
|-- server_position.rs
`-- stats.rs
```

Move `WorldStream`, `actor_store`, `block_entity_visuals`, and `server_position` into this crate.
The production crate has no Bevy or WGPU dependency. The current `Resource` derive is removed;
the app wraps `WorldStream` in an application-owned resource.

`stream.rs` owns the `WorldStream` state and its narrow public facade. Child modules add coherent
`impl WorldStream` blocks and can access parent-owned private fields without publishing them.
Sequencing and admission own FIFO ordering and bounded heavy-event admission. Requests owns the
sub-chunk request, retry, acknowledgement, and deadline state machine. Lighting owns snapshots,
provenance, readiness, worker dispatch, completion validation, and dependent dirtiness. Meshing
owns snapshot creation, scheduling, revision validation, and publication output. Residency owns
world application, eviction, dimension changes, and known-air state. Cohort owns publisher scope
and exact-view status. Actors and block entities own their respective lifecycle state.

The first pass is a mechanical crate move followed by method relocation. Regrouping fields into
owned sub-states such as `RequestTracker` or `LightEngine` is not part of the mechanical split.
It may occur only as a later, separately designed and benchmarked change because crossbeam/Rayon
timing, atomic completion, and backpressure behavior are sensitive to state ownership changes.

## Chunk rendering

Keep Bevy/WGPU rendering in one `render` crate and replace `plugin.rs` with:

```text
crates/render/src/chunk/
|-- mod.rs
|-- plugin.rs
|-- api.rs
|-- queue.rs
|-- extract.rs
|-- draw.rs
|-- textures.rs
|-- biome_tints.rs
|-- gpu/
|   |-- types.rs
|   |-- arena.rs
|   |-- layout.rs
|   |-- upload.rs
|   `-- bind_groups.rs
|-- pipeline/
|   |-- layouts.rs
|   |-- opaque.rs
|   |-- model.rs
|   |-- liquid.rs
|   `-- commands.rs
|-- transparent/
|   |-- sort.rs
|   |-- model.rs
|   |-- liquid.rs
|   `-- retirement.rs
`-- presentation/
    |-- frame_probe.rs
    |-- model_witness.rs
    |-- transparent_witness.rs
    `-- metrics.rs
```

`api` owns the application-facing queue, instance, upload acknowledgement, and view-expectation
contracts. `gpu::types` owns shared private allocation identities used by upload, drawing,
transparency, and presentation. Arena modules own allocation math and retirement; upload owns the
per-frame mutation driver; pipeline owns layouts and specializers; draw owns phase commands and
submission; transparent owns sorting and retirement state machines; presentation owns exact
frame and witness evidence.

`chunk::mod` composes the subsystem and explicitly re-exports only the stable public API. No glob
re-exports or crate-wide prelude are allowed. Internal types use `pub(crate)` only where a sibling
subsystem has a demonstrated dependency.

Rename `DebugWorldPlugin` to `ChunkRenderPlugin` as part of the workspace-wide move. Because
backward compatibility is not required, consumers change atomically and no permanent alias is
retained.

Moving `load_internal_asset!` calls changes their source-relative WGSL paths. Every shader path is
updated in the same mechanical move, and the existing shader/noop-WGPU tests must pass before
further renderer decomposition.

## Application composition and acceptance

Add `app/src/lib.rs`; reduce `main.rs` to CLI parsing, error reporting, and calling the library
entry point. Organize application-owned behavior as:

```text
app/src/
|-- main.rs
|-- lib.rs
|-- app.rs
|-- runtime/
|   |-- endpoint.rs
|   |-- shutdown.rs
|   |-- network.rs
|   |-- world.rs
|   |-- visibility.rs
|   `-- telemetry.rs
`-- acceptance/
    |-- mod.rs
    |-- world_ready.rs
    |-- teleport.rs
    |-- remesh.rs
    |-- mutation.rs
    |-- proofs.rs
    `-- markers.rs
```

Runtime modules own Bevy system wiring and normal client behavior. Acceptance modules own every
acceptance-only tracker, deterministic proof constructor, completion decision, and
`RUST_MCBE_*` marker string. The existing `metrics.rs` and `network.rs` split along these owners
rather than becoming generic `helpers` modules.

Rust `acceptance::markers` and PowerShell `Markers.ps1` form one cross-language protocol. A
structural test checks that every marker literal has exactly one Rust producer and the expected
PowerShell parser/consumer.

## PowerShell acceptance harness

Keep `scripts/acceptance.ps1` as the stable entry path containing the parameter block, pinned
constants, top-level validation, fixed library load order, and main orchestration. It dot-sources:

```text
scripts/acceptance/
|-- Common.ps1
|-- RuntimePaths.ps1
|-- Process.ps1
|-- Bds.ps1
|-- Markers.ps1
|-- Proofs.ps1
|-- Resources.ps1
|-- Metrics.ps1
`-- Galleries/
    |-- Common.ps1
    |-- Leaves.ps1
    |-- CrossCrop.ps1
    |-- Aquatic.ps1
    |-- Water.ps1
    |-- FlowerBed.ps1
    |-- SlabStair.ps1
    `-- Vine.ps1
```

The fixed order is explicit and tested. Libraries do not execute main-flow behavior when loaded.
The existing `RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY` seam remains functional.

Before splitting the monolith, add `Get-AcceptanceCompositeSource`, which returns entry-script and
library text in canonical load order. Raw source-order tests migrate to that helper while the
source is still monolithic. This prevents test weakening during the physical split.

Split `scripts/tests/acceptance.Tests.ps1` by runtime/process behavior, fixtures and galleries,
markers and proofs, resources and metrics, and orchestration. Existing runtime-safety tests remain
separate. Pester 3.4 and Windows PowerShell 5.1 are required compatibility targets. Dot-sourcing,
captured helper scriptblocks, and exact scalar-token validation remain unchanged.

## Test architecture

Tests follow production ownership:

- Private pure tests live in child `tests` modules beside the subsystem they exercise.
- Large child suites use `tests/mod.rs` with domain files, preserving access to private parent
  items without public test helpers.
- Integration roots include per-family or per-subsystem modules and shared fixtures so expensive
  suites remain one linked test binary where WGPU or large fixture construction dominates cost.
- Compiler tests mirror `visuals/families` and share pack/registry fixture builders.
- Meshing tests move out of `render`, so ordinary CPU geometry tests do not link WGPU.
- `*_for_test` production exports are removed. Tests either exercise the public behavior contract
  or live beside private implementation.

The current mixed world-stream/Bevy publication test is handled explicitly. During extraction it
may temporarily remain a client-world unit test with Bevy/render dev-dependencies so it retains
private access. Before the final dependency guardrail lands, it splits into pure private
client-world scheduler tests and an app-owned public-contract publication test. No permanent
test-support production API or Bevy normal dependency is introduced.

Every mechanical move records `cargo test -- --list` before and after and compares exact test names
and counts. Test behavior may be rewritten only in a separate semantic commit with an explicit
reason.

## Migration sequence

Each tranche receives one focused independent review. Fix all Critical and Important findings,
but do not start repeated review loops unless a correction materially changes production behavior.

1. Record the clean baseline: workspace test lists and counts, Rust/Go/Pester/Bash results,
   acceptance dry-run output, relevant renderer benchmarks, and repository status.
2. Add `Get-AcceptanceCompositeSource` and migrate raw PowerShell source assertions without moving
   production functions.
3. Split PowerShell libraries, gallery families, and Pester suites one ownership group at a time.
4. Create `asset-compiler` by moving the compiler and `assetc` unchanged. Move all command strings
   atomically and verify the complete `-p assets`/`--bin assetc` search.
5. Introduce the explicit fail-closed family result contract test-first, then split compiler
   families and their tests.
6. Extract `meshing` and atomically flip every workspace consumer to its canonical APIs.
7. Split `render::chunk`, updating WGSL source-relative paths in the same mechanical move.
8. Move `WorldStream` and its private collaborators into `client-world`, then separate method and
   test modules without regrouping scheduler state.
9. Add the app library and split runtime composition, telemetry, and acceptance evidence.
10. Remove migration scaffolding, rewrite the mixed publication test across its final owners, run
    benchmarks, and execute the full native acceptance gate.
11. Enable final architecture enforcement only after the tree complies.

No step changes the stable Windows BDS or Rust client executable paths used by live tests. No
Mojang asset, screenshot, runtime, or evidence payload enters git.

## Verification

Every tranche runs the narrow affected tests and the full relevant workspace gate. The final gate
includes:

```text
cargo fmt --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
go test ./core/...
go vet ./core/...
PowerShell 5.1 / Pester acceptance suites
Bash acceptance suites
repository architecture policy check
```

Renderer and world-publication tranches rerun their existing deterministic benchmarks. Final live
acceptance uses the approved stable Windows executable paths and existing evidence discipline; it
does not recapture equivalent native views merely because files moved.

## Structural guardrails

Create a cross-platform `tools/architecture` Rust workspace tool with a checked-in
`tools/architecture/policy.toml`. Land strict enforcement after the migration. The policy contains
the allowed crate dependency edges and source-size budgets:

- production Rust hard maximum: 1,000 lines; normal target: 300-800
- `main.rs`, `lib.rs`, and module roots: 300 lines
- PowerShell library maximum: 800 lines
- test source maximum: 1,200 lines
- gallery code split by family before reaching the PowerShell limit

During migration the checker operates as a ratchet against the recorded baseline. Final strict
enforcement begins only when the repository complies. Exceptions are allowed only for generated or
versioned data and must include an owner, reason, and expiry. Handwritten production and test code
cannot use the exception manifest.

The checker also rejects:

- runtime dependencies on `asset-compiler`
- Bevy or WGPU normal dependencies in `meshing` or `client-world`
- dependency edges absent from the explicit workspace allowlist
- glob re-exports and project-wide preludes
- public APIs that exist only to expose private behavior to tests
- duplicate or unpaired Rust/PowerShell acceptance marker contracts
- tracked Mojang assets, screenshots, or local runtime artifacts

Line count is a backstop rather than the decomposition rule. Modules must still have one named
owner, and splitting one state machine across arbitrary files does not satisfy the design.

## Completion criteria

The restructure is complete when:

- no targeted production or test file violates the final source-size policy
- the dependency graph matches the selected architecture with no compatibility shims
- CPU meshing and client-world production builds contain no Bevy/WGPU dependency
- the runtime `assets` dependency graph excludes offline compiler dependencies such as Clap and
  source image decoding
- renderer subsystems communicate through named private contracts rather than a god module
- compiler family ownership and fail-closed ordering are explicit and tested
- normal client runtime and acceptance evidence are separate app-owned modules
- all original test names/counts are accounted for or changed in an independently reviewed
  semantic commit
- full Rust, Go, PowerShell, Bash, benchmark, and live acceptance gates pass
