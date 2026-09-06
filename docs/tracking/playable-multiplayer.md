# Playable multiplayer execution track

The master plan remains the long-term scope. This track orders implementation by
working player workflows rather than counting isolated infrastructure as complete.
No percentage or native parity gate is implied by an implemented helper or test.

## Baseline stabilization (in progress)

- Restore current-target acceptance checks without weakening rejection contracts.
- Validate current world, collision, entity, HUD, font, icon, and language assets.
- Exercise Windows launch, menu navigation, join, movement, chat, disconnect, rejoin.
- Record exact build and scenario for fresh release frame-time/memory measurements.
- Fix observed blockers before expanding the gameplay surface.

Initial Windows checks on 2026-09-06 found two production blockers: the chat input
system consumed menu clicks before menu handling, and an unavailable optional pack
cache prevented core startup. Both fixes passed regression tests and a live
Lifeboat join. Asset failures now precede connection startup, with an
executable-level regression test.

### Local checkpoint, 2026-09-06

Implementation checkpoint on `dev/ox-alpha`; publication and review status are
tracked separately from the implementation and native acceptance evidence below.

- Current protocol/count gallery checks and registry-sensitive dry-run snapshots;
  PowerShell diagnostic matching preserves rejection tests across error views.
- Windows executable physics-recovery test uses native paths for its COPY recipe,
  separately from Make's forward-slash target paths.
- Required-asset validation before network startup, menu click ownership, and
  consuming the pause-opening Escape without replaying it on the next frame.
- Optional pack-cache failure leaves joining available without using unsafe paths.
- Session teardown removes old render entities, queued uploads/removals, and
  resident diagnostic attribution; GPU allocations retain fence-based retirement.
- Full-cohort publication evidence is collected only for explicitly requested
  acceptance/metrics runs, not every ordinary gameplay frame.
- Untimed server motion replaces velocity immediately and remains replayable at
  the next simulation boundary. Late, nonzero-tick motion is still incomplete.

Verification: the full Rust workspace suite passed (3,370 passed, 13 ignored),
including 821 client and 225 render unit tests. Strict client/render
lint and architecture checks passed. The Windows acceptance dry-run and
PowerShell asset-acquisition harness passed. Platform-specific shell extraction
legs were not rerun, and no hosted CI result is implied.

Native Windows/DX12 checks used the optimized debug executable based on
`0d182c105d7860685f6944cc8ad883c1c7ed96fe` plus local changes, SHA-256
`70eb16c739d3032a9b9e869563e21bf927b4050e156951d3982398d8c6462aad`.
At 1280x720 client size, scale 1, GUI scale 2: launcher clicks, inventory open/close,
chat focus/dismissal (no public message sent), one-press pause, disconnect to zero
rendered chunks, and fresh rejoin were observed on `mco.lbsg.net:19132`.
The latest session rendered a limited 25-column cohort; this is not full-view
acceptance. Server-reported version was not captured. Brief automated movement
taps did not establish a movement pass: the native automation helper released
mouse capture when sending gameplay keys. This is not evidence of a movement
regression. Debug FPS is not release performance
evidence. Long launcher text/toast clipping also remains open.

The synthetic visual-gallery harness still reads MCBEAS05 while the production
compiler emits MCBEAS07. Passing its synthetic tests is not current-carrier visual
acceptance. Its registry-sensitive snapshots must also follow protocol 2168.

## First complete interaction

Join with authoritative physics, select a tool, target a block, mine it, and
reconcile the server-confirmed result. Implement the actual app action producer
and its shared immutable pose/target/selected-stack authority. Include delayed
motion handling, transport pressure, corrections, and session replacement tests.

Then extend the same workflow to placement, block/item use, and chest interaction.
Complete server-form presentation and response delivery for multiplayer navigation.

## Subsequent complete workflows

1. Melee, damage, death, and respawn.
2. Broader inventory operations and crafting.
3. Audible gameplay/UI feedback and basic non-player rendering.
4. Movement variants and measured performance corrections.
5. Runtime pack application, followed by expanded pack/UI compatibility.

Preserve joining servers that offer packs while runtime loading is incomplete.
Cache unavailability must not disable all joining, and unsafe cache paths must not
be read or written as a fallback. Full pack compatibility remains incomplete until
content actually reaches the running renderer/UI/audio consumers.

Use focused tests plus rendered/live evidence at each milestone. Re-review when
changes materially affect the contract, not as a replacement for integration.
