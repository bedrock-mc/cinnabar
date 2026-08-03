# Live testing, capture, and native evidence

Load this before running the Bevy client or BDS on Windows, capturing frames, or
closing a native/visual/performance acceptance gate.

## Windows capture

Use native Computer Use/WGC as the primary path for Cinnabar window inspection and
input testing. Do not assume the Bevy window is inaccessible because an earlier run
failed: refresh app/window discovery for each live run and diagnose a missing
target as a current integration bug. If native capture genuinely fails after fresh
discovery and recovery, use Windows GDI `CopyFromScreen` only as an explicit
fallback, write PNGs beneath `%TEMP%`, and inspect those fresh files with the
image-viewing tool. Never claim visual verification from a stale or occluded
capture.

## Stable executable paths

Windows Firewall consent is path-specific, so reuse the paths the user already
approved: `.local/bds-runtime/bedrock-server-1.26.32.2/bedrock_server.exe` for BDS,
and `target/debug/bedrock-client.exe` for the Rust client (rebuild in place to keep
it stable). Do not copy either executable to a new worktree or temporary path for a
live run, do not change firewall policy, and do not automate UAC or security-consent
dialogs. If a genuinely new listening executable is required, explain why and wait
until the user is at the PC.

## Remote movement test targets

Use these user-designated Bedrock endpoints for Phase 3 movement and session
acceptance:

- Zeqa: `zeqa.net:19132`.
- Lifeboat: `play.lbsg.net:19132`. After joining, `/transfer sm3` exercises a
  deeper transfer/session path.
- Zeno external BDS: `zenomc.org:19197`. This is the low-population
  server-authority target for observing official-BDS movement rejection and
  correction behavior without depending on other players.

Treat these as compatibility and server-authority targets, not as substitutes
for a version-matched native Bedrock parity comparison. Record the resolved
endpoint, server-reported version, scenario, duration, and exact client build in
each acceptance artifact.

## Visual acceptance

A UI, HUD, text, graphics, shader, or rendering change is not ready to push or
describe as done without a real rendered-frame pass on the target platform,
resolution, and DPI/scale. Unit tests, snapshots, draw-list checks, GPU adapter
tests, lint, and code review are necessary but never substitutes for seeing the
output. The pass must explicitly check legibility, geometry, clipping,
depth/layering, scaling, colors, and the relevant live input/focus behavior, and
must record the tested platform and visible result. If the target-platform pass
cannot be performed, keep the change local and say it is not cleared to push.

## Native and performance evidence

Use native Bedrock/BDS comparison when it decides a contract or closes an explicit
acceptance gate, preferring version-matched, reproducible, fixed-state galleries and
exact protocol fixtures over visual guesswork. Perform live acceptance only from the
firewall-approved paths above, after integration and a build at the canonical path.
Batch equivalent captures and reuse an authoritative existing witness when it covers
the same version, state product, camera, geometry, material, and behavior question.
Performance claims require measured release evidence against the stated `plan.md`
budgets; a debug screenshot, small test scene, or green unit suite is not
performance acceptance.

For main-thread attribution, set `RUST_MCBE_STAGE_PROFILE=1` only on an
instrumented release acceptance run. The client emits one
`RUST_MCBE_STAGE_PROFILE` record per second with count, cumulative milliseconds,
and maximum milliseconds for each runtime stage. Compare runs with the same
scene, BDS state, duration, release profile, and present mode. Treat overlapping
worker and main-thread stages as attribution rather than additive wall time, and
run the final performance gate again without the variable because profiling
changes the measured workload.
