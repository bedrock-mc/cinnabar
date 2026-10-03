# Chunk scheduling and world authority: restructuring step 6

`chunk-pipeline` owns `WorldStream`'s terrain residency, request scheduling,
lighting and mesh workers, visibility, staged completions and bounded publication.
`client-world` owns packed terrain, actor state, session identities, registry and
biome state, committed consumer queues, decode preparation and the ordered commit
frontier. `world` remains the packed terrain model. Reusable pack compilation is
covered in [pack-compiler.md](pack-compiler.md).

`WorldAuthority` exposes read-only terrain borrows and synchronous mutation
commands. Its fields are private; the coordinator cannot directly change session
identities, biome revisions, actors or committed queues. `OrderedCommitState`
retains admission, heavy-event membership, decode completion ordering, partial
batch progress and the asynchronous block-mutation fence. Prepared records cross
the seam as owned values. No second world store, frontier or channel was added.

Block-crack presentation and deferred block predictions remain terrain adapters
in the coordinator because their reconciliation follows residency and pending
block mutation work. Map images, sign requests and block-event cues are retained
by the lower authority.

The session's block palette diagnostics also belong to `client-world`.
`WorldAuthority` retains one shared sample budget and passes it, together with
the captured session and registry identities, to every decode job. Registry
provenance logging stays beside remap updates. The pipeline dispatches those jobs
without keeping a second registry or diagnostic budget. The bounded sampling and
unknown-ID air fallback retain the behavior and references documented in the
[Lifeboat investigation](../evidence/lifeboat-offline.md#vanilla-references).

The coordinator still decides relevance and mutation order. It applies each
accepted mutation synchronously before invalidating lighting, mesh or block-entity
presentation. Worker jobs retain the existing captured inputs and bounded result
channels. The lower crate runs decode preparation; the upper crate owns dispatch,
worker priority and completion polling.

## Ordering and bounds

- Admission retains the same event and heavy-event limits. Retained controls, UI,
  audio and camera events still count against admission. Particle retention keeps
  its separate bound and previous overflow behavior.
- Partial subchunk batches hold the committed frontier until their final entry.
  Installing a batch preserves the existing deadline checkpoint before applying
  its first entry. The frontier visible inside synchronous application keeps its
  original value.
- A block-update batch snapshots terrain and starts prediction handling before
  installing the asynchronous commit fence. Only its matching decode completion
  releases that fence; later events cannot pass it. Reservations release at the
  same completion points.
- Session counters remain process-wide atomics in `client-world`. Dimension
  changes, correction ticks, actor lifetimes, biome revisions and committed
  consumer drains keep their existing reset and ordering behavior.
- Publication allowance, permit ownership, mesh-memory reservations and their
  drop paths stay together in `chunk-pipeline`. Staging bounds, stale-completion
  checks, eviction order and renderer handoff are unchanged. Shared publication
  contracts still come from `render-api`.

The app temporarily names its `chunk-pipeline` dependency `client-world`.
An explicit facade reexports the previously consumed domain types and stream API,
so this migration does not edit the menu or UI runtime being extracted separately.
The manifest identifies the real package; the architecture checker resolves that
identity through the alias. Future consumers can depend on `client-world` directly
when they only need authority. This adapter owns no duplicate state.

## Enforced dependency boundaries

- `client-world` may depend on `assets`, `protocol` and `world`; it cannot depend
  directly on `render-api`, `meshing` or `chunk-pipeline`. Existing protocol skin
  contracts still reach `render-api` transitively.
- `chunk-pipeline` may depend on `client-world`, `meshing`, `assets`, `world` and
  `render-api`. Its production code consumes explicit ingress contracts exposed
  by `client-world`; direct `protocol` use is confined to fixtures.
- `pack-compiler` may depend internally only on `assets`. `asset-compiler` is the
  CLI and may depend on `pack-compiler` and `assets`; the app cannot regain a
  direct or transitive dependency on that CLI.
- Transitive rules keep Bevy, wgpu, the app, UI and renderer out of the lower
  layers and prevent renderer back-edges into world authority or chunk scheduling.
- Syntax rules prevent `client-world` from importing scheduling/render modules
  or owning chunk meshes, publication allowances, publication permits or mesh
  memory permits. The coordinator cannot own a separate `ChunkStore` or
  `ActorStore`; it composes the lower authority and borrows terrain. Regression
  fixtures exercise direct, aliased and transitive
  dependencies, ownership through aliases and test exclusions.

## References and verification scope

This is an ownership migration, with no new vanilla behavior or parity claim.
The original behavior and reference comments move with their owners. Scheduling
and completion roles are consistent with these identified references:

- Lens reconstructed client `1.26.50.26`, artifact 6: source search followed by
  source-backed raw reads of `RenderChunkShared::startRebuild` (RVA `0x1ef8770`)
  and `RenderChunkShared::endRebuild` (RVA `0x1ef96f0`). They identify main-thread
  rebuild setup, exclusive in-progress geometry and completion prerequisites.
- **R:RenderChunkShared:383**, **R:RenderChunkShared:390** and
  **R:RenderChunkShared:628** identify main-thread setup and completion.
  **R:RenderChunkShared:637–657** describes completion prerequisites/order.
- **R:RenderChunkCoordinator:878–907** handles loaded subchunks and dirty work;
  **R:NetworkChunkSubscriber:75–99** moves the retained view as its center changes;
  **R:ChunkViewSource:1147** invokes grid movement. Reference root:
  `~/coding/go/lunar/refs/mcsrc-1.26.50/reference/26.30/src/by-owner`.
- Pack inputs and their vanilla source references are recorded in
  [the pack migration](pack-compiler.md#references-and-parity-scope).

The numeric queue and work budgets remain Cinnabar's existing bounds; this work
does not claim they are vanilla constants. Moved tests cover ordering, admission,
publication ownership, predictions, session and dimension changes, decode
leniency, actor and audio ingress, residency and lighting. New lower-layer tests
exercise FIFO gaps, partial batches, asynchronous fences and biome overflow.
Optional installed-asset fixtures skip when their inputs are absent. No client or
live server was run. [Build timing evidence](../evidence/chunk-pipeline-build-timings.md)
records the local before/after executable rebuilds.
