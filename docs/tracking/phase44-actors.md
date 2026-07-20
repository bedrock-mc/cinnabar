# Phase 4.4 actor authority and live interpolation tracker

Current progress: **75%** at implementation head `296b7f3` (authority, production eligibility, acceptance-only correlation, strict entity/player geometry authority, bounded texture routes, and native local-F5 presentation are implemented; the broader movement/native-comparison/performance gates remain open).

This estimate uses equal contract, runtime, presentation, deterministic-verification, and live/native/performance gates.

Gate scores: contract 96%, runtime 94%, production presentation 86%, deterministic verification/review 89%, live/native/performance 10%; arithmetic mean 75%. A bounded Windows LBSG local-F5 witness has passed, but it does not satisfy the broader actor movement/native-comparison/performance gate.

## Landed

- [x] Bounded actor lifecycle/store, roster, skins, metadata, and foreign-player movement routing.
- [x] Exact actor packet-origin and network-offset normalization.
- [x] Oomph-style three-tick player convergence.
- [x] Separate adjacent-frame renderer interpolation.
- [x] Teleport/replacement snaps plus finite, frustum, distance, and capacity controls.
- [x] Deterministic ordinary-move, teleport, origin, and interpolation tests.
- [x] Retain and resolve `AddPlayer` game mode against the authoritative default (`29b47bb`).
- [x] Handle `UpdatePlayerGameType` and `SetDefaultGameType` correctly (`29b47bb`, completed for StartGame-only local authority by `b68adb5`).
- [x] Apply spectator and metadata-invisible filtering in the production rig renderer before culling and the 128-instance cap (`b5f1c09`, `20534df`, `b68adb5`), including synthetic local F5 suppression across post-StartGame mode changes.
- [x] Treat `FORCE_MOVE` as a snap without falsely reporting a teleport (`29b47bb`).
- [x] Add bounded packet-to-store-to-presented-frame correlation (`9d938e2`, corrected by `20534df`): capture is disabled during normal gameplay; timed acceptance runs correlate an exact committed identity across two adjacent GPU draw generations, reset at session/dimension boundaries, record rejected/drop counts, and emit at most 64 parsed witnesses per session.
- [x] Compile and render bounded schema-v6 PNG/TGA actor assets (`8cb2518`, corrected through `4d3e2cf`): the pinned official pack currently yields 5,768 sources, 3,014 symbols, 245 geometries, exact wide/slim player rig authority, and 2 carrier textures / 32,768 RGBA bytes after duplicate-generation and material-authority rejection. Client-world joins exact PlayerList skin geometry to local and remote rigs; the renderer uses geometry-derived conservative bounds and a bounded variable-size active-frame atlas with replicated gutters and exact boundary UVs. Dynamic, conditional, multiple, duplicate-generation, per-bone, ambiguous, custom/persona, or unsupported-material routes remain explicit `NoDraw` cases rather than guessed presentation.
- [x] Retain exact bounded local login appearance authority and support legacy 64x32 Bedrock skins (`a403ff7`, `296b7f3`): the decoded appearance that feeds ClientData is carried through login/bootstrap into the session-owned WorldStream, survives animation resets, fast-transfer FIFO barriers, and dimension changes, and cannot be suppressed by a missing or incompatible PlayerList self echo. Remote actors remain PlayerList-authorized and fail closed. Legacy right limbs are mirrored into the canonical 64x64 left-limb UV islands without inventing overlay pixels.

## Remaining production authority

- [x] Retain and resolve `AddPlayer` game mode against the authoritative default.
- [x] Handle `UpdatePlayerGameType` and `SetDefaultGameType` correctly.
- [x] Correct production renderer eligibility filtering before culling/capacity.
- [x] Treat `FORCE_MOVE` as a snap without falsely reporting a teleport.
- [x] Add bounded packet-to-store-to-presented-frame correlation.

## Deterministic verification through `296b7f3`

- Actor witness, movement-authority, and Windows make-target regressions pass.
- Strict `client-world` and `bedrock-client` Clippy with warnings denied passes.
- Architecture enforcement and the full acceptance PowerShell dry-run suite pass.
- The prior `actor_store/tests.rs` and `asset_startup.rs` file-size violations are split without policy exemptions.
- Full assets/compiler, client-world, render, and bedrock-client suites pass in the isolated implementation lanes; the pinned official schema-v6 entity carrier and local/remote carrier-to-render witnesses pass.
- Strict Clippy with warnings denied passes for every changed crate. Fresh independent review approved both the legacy-skin and login-authority correction ranges; combined-head architecture, acceptance PowerShell, Go test, and Go vet gates pass.

## Live/native acceptance

- [x] On Windows against `play.lbsg.net:19132`, toggle F5 after live spawn and render the local actor from retained login authority: `local_authority=ready`, `selected_count=1`, `frame_manifest=1`, queued/drawn/acknowledged exact submission, and zero renderer rejects (`.local/acceptance/pr8-native-loginauthority`, local-only evidence).
- [ ] Authenticate to LBSG and capture spawn, ordinary movement, rotation, and teleport in one valid witness.
- [ ] Prove feet remain on the same ground plane without a 1.6-block jump.
- [ ] Compare matched native actor movement/interpolation.
- [ ] Render representative supported literal-default mobs from the pinned pack and verify texture orientation, atlas UVs, geometry, animation, culling, and reset behavior on Windows; record unsupported dynamic/multi-texture families without visual substitution.
- [ ] Pass release performance/resource gates and final independent review.

## Historical references

- `origin/backup/completion-phase4-20260719` contains the old authority/filter experiments.
- `origin/backup/completion-phase4-f9-integration-20260719`, `origin/backup/phase44-ground-contact-convergence-20260719`, and `origin/phase44-presented-ground-contact` contain witness history.

These references must be selectively reimplemented and reviewed on fresh `main`; they must not be bulk merged or rebased.
