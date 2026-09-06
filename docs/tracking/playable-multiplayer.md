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

Publication review on 2026-09-06: independent review approved the complete range
`0d182c105d7860685f6944cc8ad883c1c7ed96fe..25256816743c9884a2761c0e24b0ab6b57d8dc09`
with no findings. Fresh workspace-wide strict lint, architecture and formatting
checks, Go core tests/vet, Windows acceptance dry runs, and PowerShell
asset-acquisition tests passed. The implementation commit was verified on
`origin/dev-ox-alpha`; hosted CI was still running at this checkpoint. These
checks do not close the remaining native or feature gates.

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

### Server diagnostics and lighting follow-up, 2026-09-06

Published through `e175556d61a09b8aa9d3d847712a3d273169fe9f`:

- Physics-install tests claim temporary directories atomically, including a
  deterministic repeated-clock regression; all eight native tests passed.
- Mixed vertical lighting batches use the combined solver instead of publishing
  an air prefix before accounting for emission below it. The new regression
  failed before the repair; the client-world suite passed 352 tests, with one
  ignored. Independent review approved the complete lighting change.
- The relay preserves upstream disconnect details even when the reverse writer
  observes closure first. Delivery precedes teardown, cancellation unblocks a
  stalled delivery, and both pump errors remain inspectable. Independent review
  approved the repair; repeated ordering/cancellation tests passed.

A fresh Windows/DX12 run against BDS 1.26.40.8 rendered a 314-column cohort and
drained pending/in-flight lighting, meshing, and upload work. The optimized debug
client SHA-256 was
`5b4fb031fca9fe30fb476b5b093aa58c2d23efc0a45ad4a37884f59a4a0079ec`;
client size was 1280x720 with GUI scale 2 and a 60 FPS cap. This is not release
performance acceptance or proof that the intermittent Lifeboat stall is cured.

The same live run exposed a separate decoder bug when the server kicked the
client: Disconnect lacked its conditional-message flag. Local repair
`24db95be904fc50dc63e5ee154fe36184b978881` adds visible, filtered, and hidden wire
fixtures from the pinned Go codec. Both new tests failed before the repair and
passed afterward; the full protocol suite, strict protocol lint, formatting,
architecture, and fixture-generator tests/vet passed. Independent review approved
the complete wire repair. A fresh native BDS 1.26.40.8 kick reached the client
with the exact message and `Kicked` reason, with zero decode errors. The rebuilt
optimized debug client SHA-256 was
`b81c554afe835712f74f2f15d0b039995676e335f56dd0dc1b72deb83a3c4407`.
This CLI-mode test does not close launcher return-screen or hidden-screen UI
behavior. The relay's additional distinct-pump-error regression passed 100
repetitions; the full Go core tests/vet also passed after the follow-up.

A signed-in launcher repeat on the Lifeboat catalog endpoint also released the
loading cover, rendered the 25-column cohort, and drained the queues: 4,711
accepted light jobs, including 2,952 value changes and 1,719 no-ops. One Escape
opened Pause; Disconnect returned to Home with zero rendered chunks, and normal
Quit exited successfully. This repeats the small-scene acceptance only; the
server-reported version was not captured, and intermittent-stall, full-view,
movement, and release-performance gates remain open. Hosted verification at
`e175556d` passed all jobs, including Windows workspace tests/lint, Go checks,
registry checks, and the native PowerShell harnesses.

Menu input and provisional Creative mining remain isolated, review-blocked work,
not integrated features. Their required corrections cover editor/highlight
agreement, refocus input replay, failed replacement navigation, post-admission
mining invalidation, and cross-chunk ray authority. No gameplay or parity
checkbox is closed by these checkpoints.

### Connection cleanup and Windows verification follow-up, 2026-09-06

The concrete upstream dial adapter now normalizes a nil connection before
converting it to the session interface. Failed dials keep their original errors;
real partial connections still follow the existing cleanup path. Independent
review approved `eb42a00da68d4628b9f14d91aed753fdd2b8898b` without findings.
A fresh Windows executable test against BDS 1.26.32.2 reproduced the original
version rejection without the former secondary abort/close panics. This does
not add support for that older server version.

The previous hosted run passed Rust and core checks but failed the registry
upgrade test on CRLF `.gitattributes`. Local correction `844f03df` now exercises
both LF and CRLF attribute files while retaining exact carrier-byte and hash
validation. Registry-tool tests/vet and independent review passed. Fresh
post-integration Go core tests/vet and architecture checks also passed.
Replacement hosted CI remains pending publication.

The latest stationary Lifeboat run exposed a separate startup stall: the required
25-column cohort was complete, but lighting repeatedly invalidated meshes and
the loading cover did not release. This remains an open runtime defect; do not
hide the cover or weaken readiness checks to claim a successful join workflow.

### Protocol infrastructure checkpoint, 2026-09-06

Locally integrated `9bbeeca762fe3310d87dc6d297babb4e7440dfae` from
`3b7f5dc3137ebe18c494965006a368601534cff7`: PlayerAuthInput now has a
bounded eight-action block-action list and optional embedded creative-break
payload. Interaction flags derive from validated payloads, the selected stack
retains its verified representation, and legacy movement fixture bytes remain
unchanged. Independent review approved the full range without Critical or
Important findings; 25 protocol fixture tests passed independently. The Windows
physics-install harness also passed all seven tests, including actual shell
detection, apostrophe paths and timeout coverage.

Post-integration Rust workspace tests passed (3,387 passed, zero failed, 13
ignored), as did Go core and fixture-generator tests/vet, formatting and the
Windows PowerShell acceptance dry-run harness. The Rust suite required a scoped
dependency rebuild and serial compilation after build-artifact and Windows
resource errors. Strict workspace lint, architecture and the PowerShell asset
harness also passed. The latter skipped deeper Bash extraction checks because
the discovered Bash lacked unzip or cc. This is a local checkpoint, not a
publication or hosted CI result.

Review disposition: corrected the comment explaining the omitted stop action
(the wire codec can represent it); no runtime behavior changed. The minor
test-output drain deadline edge under continuous descendant output remains open.
Neither was classified as blocking. This checkpoint has no app mining action producer,
transport integration, measured break timing or crack-visual acceptance.

### Native stabilization follow-up, 2026-09-06

Windows/DX12 testing of baseline `3b7f5dc3` at 1280x720 found reproducible
launcher field-focus and shortcut problems: Tab changed the highlight but text
still edited the previous field, and Ctrl+A appended text instead of selecting
it. Returning from in-session Settings reached launcher Home without the pause
actions. Closing chat then clicking to recapture the mouse produced a downward
camera snap; capture-warp handling still needs investigation. Larger GUI-scale
text also needs layout correction. These findings remain open.

Lifeboat disconnect under queued terrain work returned to zero rendered chunks,
and rejoin rendered terrain again. A separate diagnostic received the complete
25-column publisher cohort but did not establish full-view parity. Joining with
required resource-pack offers remained possible, while runtime pack application
was unavailable. Neither debug frame rates nor these smoke checks close release
performance, full inventory, movement or pack-loading gates.

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
