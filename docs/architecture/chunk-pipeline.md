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
[Lifeboat investigation](../evidence/lifeboat-offline.md#vanilla-rules).

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

## Between-frames servicing

`WorldStreamService` owns one `stream-service` thread. The production app lends it
`ClientWorld.stream` in a schedule after `Last` and its network flush, and reclaims it
after frame timing starts but before `First`, so frame systems from `First` onward
find the stream present and pacing includes any reclaim wait. While
lent, the thread runs the unchanged `WorldStream::poll` in 250 µs slices: decode
acceptance, ordered commits, light and mesh acceptance and dispatch. That work overlaps
render extraction and frame pacing instead of the next frame. A reclaim raises a yield
flag that `poll_budget_exhausted` and the heavy-commit budget honour, so it waits for
at most one slice plus one work item. A run sleeps on the decode, light and mesh result
channels and a wake signal only after a slice commits, accepts and dispatches nothing.
A panic on the thread resumes on the reclaiming thread.

Ownership stays exclusive: exactly one thread holds the stream, every input it reads is
part of it, and `poll` keeps its ordered commit frontier, stale-result checks and
bounds. Once lent, the stream changes two policies:

- Retention requests mark retention due instead of evicting; the next commit or poll,
  normally the service's, re-evaluates it first. Frame systems after the request still
  see the retiring out-of-view columns until then. Chunk requests the new grid drops
  retire at the request, so the frame never flushes them; a column that holds only
  requests is evicted then.
- After a service window at least as long as the frame's allocation, the frame's poll
  keeps the 1 ms floor allocation and leaves heavy chunk data to the service. Chunk
  data that a ready block change, retention change or barrier waits behind still
  commits, so local authority sees those this frame. A shorter window restores the full
  allocation, so terrain cannot stall when frames leave no gap.

Per-poll budgets bound each service slice like a frame poll, so a lent stream can
dispatch more light and mesh work per frame. Publication permits and worker caps still
bound throughput, and the scheduler's ranking (distance, quadrupled behind the view)
orders the larger committed backlog. A view-wide join therefore meshes the faced
surface sooner and the deep or behind sub-chunks of adjacent columns later.

Without the service resource (tests, `RUST_MCBE_WORLD_SERVICE=0`) the stream never
leaves the frame thread and both policies stay off. The vanilla rule that rebuild setup
and completion run on the main thread is met in observable order, not thread identity:
the stream's single owner performs them in the same sequence.

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
The original behavior comments move with their owners.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Rebuild ownership | Setup and completion run on the main thread, with exclusive in-progress geometry and ordered completion prerequisites. |
| Dirty work | Loaded subchunks and dirty work are coordinated together. |
| Retained view | The subscribed view moves its grid when its center changes. |

Pack inputs are recorded in [the pack migration](pack-compiler.md#references-and-parity-scope).

The numeric queue and work budgets remain Cinnabar's existing bounds; this work
does not claim they are vanilla constants. Moved tests cover ordering, admission,
publication ownership, predictions, session and dimension changes, decode
leniency, actor and audio ingress, residency and lighting. New lower-layer tests
exercise FIFO gaps, partial batches, asynchronous fences and biome overflow.
Optional installed-asset fixtures skip when their inputs are absent. No client or
live server was run. [Build timing evidence](../evidence/chunk-pipeline-build-timings.md)
records the local before/after executable rebuilds.
