# Phase 8 — Audio, polish, packaging

[Roadmap](../../plan.md) · [Acceptance ledger](../evidence/phases-2-5-completion-ledger.md)

This is the retained phase checklist. Checked items describe their recorded scope only;
unchecked items and explicit limitations remain open. Historical verification statements
are not fresh test results. Follow current repository guidance when running checks.


**Optional mod updates:** Components can declare a GitHub latest-release WASM URL
for automatic background updates, or a repository URL for notifications alone.
The launch-scoped worker compares stable SemVer tags, caches repository checks,
and bounds API responses, asset downloads and redirects. Exact assets require a
verified SHA-256, matching embedded version/source and host compatibility before
atomic replacement of an unchanged managed file. Settings and grants persist;
the existing watcher reloads enabled mods. Separate General settings disable
automatic installation or all mod checks, including in passive app-updater builds.
The per-mod Update button downloads and installs the exact WASM in-app when
automatic installation is off. Pack-compatible progress reports bytes, percentage,
smoothed speed and ETA, then verification, installation and reload. Reload completes
only on the new runtime acknowledgement; duplicate presses are rejected and failures
can be retried without changing the automatic preference.
Regressions cover metadata, versions, cancellation, stale sources, download
integrity and unchanged permissions/settings. Offline home and Mods-tab captures
pass at desktop and small window sizes; live automatic-update acceptance remains
incomplete.

**Rain ambience:** completed weather ticks now admit overlapping finite rain
sounds from loaded warm precipitation columns. Sample density follows rain
strength and the graphics setting; sheltered rain uses reduced volume and pitch.
Clear weather stops admissions and lets existing samples finish. Precipitation
height/material parity, native voice limits and retail timing/audio acceptance remain
incomplete.

**World-drop audio:** successful world-input single and whole-stack drops now
emit one local `drop.slot` cue through the active pack, without waiting for or
repeating server replies. Failed and inventory-screen drops stay silent on this
route. See [the rules and regressions](../../docs/reference/item-drop-audio.md).
Matched-version live audio acceptance remains incomplete.

**Audio callback scheduling:** Hardware playback requests Windows MMCSS priority
once per pulling thread, with a current-thread priority fallback. Submission-time
resampling leaves the frame thread alone; capture playback keeps its existing
scheduling. Windows scheduling and audible acceptance remain incomplete.

**Audio ownership:** `client-audio` owns the sound engine, session admission,
named playback, predicted cues, ambience and UI sounds. It borrows listener and
particle inputs without depending on `client-presentation` or `render`.
`client-presentation` owns camera publication checks and actor presentation.
The existing sound and camera regressions remain in their owning crates and app
integration tests. This move does not close audio parity or performance gates.

**Audio output opening:** Frame systems poll the output worker and retain admitted
sources within the existing voice limits until a transport is ready. Stops,
session resets and failed opens still retire waiting work. Channel-controlled
regressions are in place; final-revision verification and native first-sound
frame timing remain incomplete.

**Sound-bank loading:** First-use lookups queue file descriptors; the existing
workers read and decode the sounds. Loading queues up to four finite sounds each
for UI, footsteps and interactions, with a 1 MiB input limit. Generation checks,
admission limits and PCM cache accounting stay in place. Live first-play latency
and cold-cache frame acceptance remain incomplete.

Scope: audio via bevy_audio/kira — sound events mapped through `sound_definitions.json`,
positional sounds, music/ambient (asset-availability audit from Phase 2 decides
bedrock-samples vs. client-assets-import); performance hardening pass against budgets;
macOS .app + codesign/notarize, Windows installer, Linux AppImage; core binary bundled and
lifecycle-managed by the app; local crash records; auto-update channel;
first-run experience.

**Packaging status (provisional):** `packaging/` holds macOS `.app`/DMG, Windows MSI, and Linux
AppImage recipes plus `.github/workflows/package.yml`; first-run asset preparation, local crash
records (never uploaded), signed-manifest update checks, and the core log/backoff helpers are in
`crates/{first-run,launcher-host,update-manifest}` and the app lifecycle entry point. Unverified until compiled and run on a clean machine: every
recipe, the WiX authoring, and notarization. Incomplete: a graphical progress/consent surface (native
dialogs only), locating a user's own Bedrock install instead of the pinned pack, in-app update
install, mid-session core restart wiring, and any crash upload (removed until a reporting project exists).

**Core joins phase 2 review corrections:** the core uses the fork pinned in `core/go.mod`,
including the listener shutdown fix. It preserves batch boundaries, retains friend-world Xbox
services until the joined session leaves, and checks required acquisition separately from the
selected pack stack. Offline fixtures cover all required-bit combinations and selected subsets.
The comparison benchmark now accounts for every packet and measures the first nonempty flush;
manual ticks and barriers cover idle delivery, coalescing, and failure attribution.

Rust forwards startup Transfer as a typed reconnect event. The startup contract is
radius → loading-start → local presentation readiness → loading-end → initialized, with
an actual Rust-through-core order fixture. See [the vanilla startup rules](../../docs/core-join-startup.md).
**Incomplete parity:** the existing terrain presentation thresholds, consent UI, dimension
transitions, and matched retail/live visual and timing evidence remain open. This packet-order
correction does not close those broader gates. No live server was used for these corrections.
**Dragonfly join (provisional, incomplete):** Dragonfly streams no terrain until initialized, which
deadlocked the gate. A session whose server sent no terrain before spawn now releases once received
work drains. The vanilla zero-terrain completion path is unconfirmed: see the open questions
in [the join evidence](../../docs/core-join-startup.md). StartGame's vanilla data-driven
definitions are retained separately from server custom visuals: the complete carrier holds their
states, but remote sessions admit them only when StartGame supplies their definitions.
The namespace distinguishes vanilla definitions from server custom blocks; the presence of
`vanilla_block_data` alone does not. A server
block's own item (no components, no item version) stacks to `Item`'s default 64.
Server custom block admission fits large pack stacks: the aggregate admits one
million states with the 64 MiB identity estimate as the memory bound (an observed
5,589-block server stack declares 136,909 states; the old 65,536-state aggregate
cut its palette mid-list and left the tail absent from the world). Rejected runs
still retain their names and counts without materialized states. Full-block custom
blocks without `minecraft:material_instances` draw their textures from the pack's
legacy `blocks.json`, matching vanilla, where only explicit material instances
override that binding. Live-server visual and interaction acceptance for these
stacks remains open.

---
