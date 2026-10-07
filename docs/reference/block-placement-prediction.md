# Block placement prediction

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Placement position | Use the clicked block’s replacement predicate. Offset a nonreplaceable block along its face even when its type matches the held cube; identifier equality is not a blanket veto. |
| Local prediction | After resolving placement position/state and checks, synchronously set the local block with flags `3`, layer `0`, before local consume/sound/placement effects. Do not wait for server acceptance. |
| World mutation | Mutate the loaded chunk and notify listeners; missing chunks and out-of-bounds positions fail. |
| Block identity | Retain all 32 network-ID bits; `-1` is an uninitialized sentinel, not a general negative-hash rejection rule. |
| Server correction | Failed transactions resend blocks around the clicked and face-neighbor positions along with player/inventory correction. Later authoritative updates replace predictions. |

## Cinnabar correction and verification

The existing `WorldStream::predict_block` already commits collision/render source state,
queues urgent local lighting/meshing and admits later authoritative corrections. The app
disabled that path for a stateless full cube when held/clicked identifiers matched, and
discarded valid high-bit hashes through checked signed-to-unsigned conversion. Both guards
are corrected; the collision-box lookup shares the same bit-preserving ID normalization.

App regressions reproduced those two blockers before the fix; all 14 focused block-use tests
now pass. The world-stream regression publishes a nonempty urgent predicted mesh with no
server acceptance event, acknowledges its upload, then observes an urgent removal following
an authoritative air correction. All 4 prediction tests pass. Workspace all-target tests,
formatting, strict Clippy and the architecture policy check pass locally.

The canonical debug executable was exercised on macOS/Metal in a fresh loopback creative
world with server-to-client datagrams delayed 800 ms. The selected grass block uses a
high-bit wire hash and was placed against grass, exercising both corrected guards together.
At `2026-10-01T12:08:16.591813Z` the application logged a successful local prediction at
`[0,-60,-4]`. The right-button event was posted at Unix time `1790856496.5659251`; the
early rendered PNG was written at `1790856496.7921803`, about 226 ms later and before a
server block response could cross the delayed relay. The block is absent from the before
frame and present in both the early and post-reply frames. Frames are ignored local files
`2026-10-01_12.08.15.png`, `12.08.16.png` and `12.08.17.png` beneath `.local/screenshots/`.
The grass hand/hotbar visual and open-inventory icon also render after the carried-texture
fix. This is a live functional prediction witness, not vanilla Bedrock/BDS parity acceptance.
Platform: macOS 26.3 / M3 Pro / Metal, optimized debug, logical window 1280x752,
rendered content 2560x1440, Retina scale 2. Native capture succeeded; native input failed
its fresh focus check, so an explicit local macOS input helper held focus throughout the
three-frame witness and checked the title reported `captured` before use.

Provisional, labeled incomplete: oriented/sized/merging/non-cube block state resolution,
replacement and custom placement rules, Adventure item restrictions, selection shapes,
actor-overlap tolerances, repeat timing and complete vanilla material/side-effect behavior.
App prediction remains after successful local outbox admission, not before packet queuing;
vanilla ordering parity is not claimed. No full placement parity gate is closed. Changes
remain local and uncommitted.
