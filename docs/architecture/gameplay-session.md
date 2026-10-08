# Gameplay and client session: restructuring step 4

`gameplay` owns local prediction, movement modes, correction and retry state,
mining, combat, block use, and accepted item use. `client-session` owns the network
worker, bounded transport queues, connection progress, and pack preparation.
Neither crate depends on Bevy, app, UI, the renderer, or the other new crate.
The app retains resource wrappers and installs the existing system order.

## Ownership and synchronous boundaries

Gameplay reads player authority from `player-state` and `inventory`, and world
facts through a synchronous `GameplayWorld` borrow. The app implements that view
over its existing stream. The trait does not own terrain, actor stores, publication
permits, or a second inventory. Collision queries continue through `sim`.

Transport command identity and cancellation guards live in `protocol`, shared by
gameplay and session. The app couples the gameplay ticker to the session's existing
watch publisher. It does not create another queue or sequence counter. A prepared
presentation payload is opaque to client-session; the app supplies its compilation
callback and specializes the handle's payload type.

The app projects drained or requested physics faults into its existing evidence
record, keeping pending fault state solely in gameplay. The evidence module and
step 5's actor publication, presentation, camera and audio files remain unchanged.
Fast-transfer actions are shared through `protocol`, with the existing client-UI
re-export retained; pack texture source limits now have one owner in `resource-pack`.

Session preparation owns validated pack admission, generation fencing, StartGame
block and item inputs, stack fingerprints, language overlays, and texture catalog
and decode preparation. Renderer, UI, entity and audio subscriber compilation and
publication remain app adapters, alongside the work owned by step 5. Join polling
owns generation cancellation, launcher fallback, endpoint readiness and deadlines;
the app retains process ownership, menu projection and Bevy resource insertion.

## Ordering retained

The app's schedule retains its existing slots and chain:

1. Commit and reconcile world controls before local physics.
2. Advance fixed physics ticks from the current input and movement modifiers.
3. At network send, flush inventory, emit observations, resolve melee, mining,
   block use and item use, then send player inputs and resolve pick block.

Accepted item use updates the same unsent tick synchronously before its movement
packet is handed off. A rejected transaction does not commit candidate use or
swing state. A full queue keeps the deferred press/release semantics, including a
release preceding a later press. Existing sprint and modifier behavior is retained;
this migration does not recompute a completed physics tick after accepting use.

The session keeps the separate bounded world/control channels, command FIFO,
readiness counters, sequencer, transfer barrier, and latency acknowledgement fence.
A movement send remains pending until socket acknowledgement or cancellation;
correction epochs invalidate stale sends through the original watch channel.
Confirmed writes publish their retained evidence at the same boundary. Shutdown
and final control flush retain their original limits and ordering.

Bootstrap still compiles pack subscribers, rejects an invalid required stack,
then builds item components and actor artwork before publishing StartGame. A
failed core retains its directory guard until the app has stopped the process.
A retired generation cannot poll or start a replacement transport.

## Enforcement

The architecture policy rejects direct, aliased, target-specific and transitive
app, Bevy, UI, renderer, chunk-pipeline and peer-domain dependencies. Session's
existing latency benchmark may use `sim` only as a development dependency.
Module rules reject rooted app adapter imports and duplicate inventory/world
owners. The gameplay physics controller may own `sim::PlayerState`, its predicted
position and velocity; the narrow ownership exception does not permit another
inventory or authoritative world store.

Fixtures use non-default `test-support` features enabled only by app development
dependencies. Policy regression tests reject production activation, including
feature aliases. Pure behavior tests move with their owner; tests that exercise
Bevy, app evidence or transport composition stay in app. No live server is needed.

## Vanilla rules and migration scope

This is an ownership migration, not a new vanilla parity claim. Existing behavior
and provisional limitations remain attached to the moved implementation.

| Area | Retained rule |
| --- | --- |
| Item use | Rearm build actions after air use and preserve use, release and completion transactions. |
| Movement | Preserve MovePlayer handling and player-input construction, including the existing teleport behavior. |
| Bow charge | Derive charge from remaining use ticks and preserve those values for the presentation adapter. |

The installed vanilla pack is read through the worktree's `.local` symlink.
`assets/bedrock-samples/v1.26.50.4/full/resource_pack/attachables/bow.json:28`
defines the bow charge rule. Artwork and presentation are not moved here.

See [local rebuild measurements](../evidence/gameplay-session-build-timings.md).

## Local verification

All Cargo commands ran through the required `scratchpad/cslot` limiter with
`RUSTC_WRAPPER=`. The final affected test run passed 3,145 tests across 76 test
targets, including doctests; 33 existing manual or opt-in tests remained ignored.
The app suite passed 1,176 tests, gameplay passed 349, and client-session passed 67.
The same-tick transaction/PlayerAuthInput regression and all 65 architecture tests
passed. No original app test names were lost during relocation.

Checks completed:

- `cargo test --offline --locked --workspace --no-run` compiled all workspace test targets.
- `cargo test --offline --locked --no-fail-fast -p bedrock-client -p client-session -p gameplay -p protocol -p resource-pack -p client-ui -p launcher -p architecture` ran the affected tests and doctests.
- `cargo clippy --offline --locked --all-targets` for the same eight packages passed with `-- -D warnings`.
- `cargo fmt --all -- --check`, `git diff --check`, and `architecture check --root . --policy tools/architecture/policy.toml` passed.

The `verify-affected --base origin/dev --dry-run` plan selects the full workspace
because members and the lockfile changed. Local execution used the requested
workspace compilation and affected tests; the full test matrix remains for CI.
Three loopback fixture tests initially hit sandbox permission errors and passed
when rerun with local socket access. No live game server was used. Tests in the
new domains require no `.local` installation; existing optional carrier fixtures
retain their absence checks.

Local logs are `/private/tmp/cinnabar-step4-workspace-test-build.log`,
`/private/tmp/cinnabar-step4-affected-tests-final.log`, and
`/private/tmp/cinnabar-step4-clippy.log`.
