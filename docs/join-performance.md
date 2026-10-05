# Join performance

The target is 500 ms from the Join action to a controllable frame with complete
visible terrain. This gate remains incomplete. StartGame arrival, loading-screen
release, and GPU terrain presentation are separate milestones.

## Measured bottlenecks

Live probes used the production authenticated dialer and verified resource-pack
cache, alternating serial and overlapping transport setup. They sent ordinary
login traffic and disconnected without movement or chat. Measurements below end
at upstream StartGame, not the first playable frame.

| Server and cache state | Serial median | Overlap median | Evidence |
| --- | ---: | ---: | --- |
| Zeno, authentication and packs warm | 751.95 ms | 623.18 ms | Six paired attempts at `zenomc.org:19132` |
| Zeqa, authentication and packs warm | 524.75 ms | 479.75 ms | Four paired attempts |
| Lifeboat, authentication and packs warm | 576.10 ms | 600.49 ms | Four paired attempts; server response variance outweighed transport savings |

Zeno resolved to `184.161.175.170` for this capture. Cold Zeno login took 12.785 s:
approximately 1.3 s authentication, 0.975 s transport/protocol negotiation, and
9.4 s pack acquisition. Its largest pack was 6,095,462 bytes, delivered in 47
chunks over 9.433 s. Every chunk was already requested before the first response;
increasing the request window would not remove this measured delay. Verified
pack reuse removes this transfer on subsequent joins.

Authentication-only preparation completed in 1.134 s before the join action.
The next fresh proof-bound multiplayer token took 84 ms instead of 1.304 s in
the unprepared first attempt. Preparation discards its key and token; real joins
still authenticate with fresh keys and perform normal verification.
Normal menu joins reuse the launcher's account core, so process-local verifier
caches survive idle preparation and later joins. Direct-address fallback cores
start fresh and have less preparation time.

An existing native optimized build joined Zeno on macOS and rendered its lobby.
The loading milestone occurred with zero opaque terrain, because this server
streams terrain after initialization. Full-view delivery then grew from five
columns at the first publication to about 820 at ten seconds. Approximately
20,776 section-light results accompanied the initial 861-column view, including
synthetic known-air slots. The longest recorded lighting worker took 304 ms;
decode and mesh worker maxima were approximately 5 and 6 ms. This instrumented
capture shared the machine with builds and does not qualify a release budget.
Relative to native bootstrap, the player column was already loaded at 909 ms;
the first terrain mesh arrived at 1,872 ms and its first GPU witness at 2,026 ms.
The aggregate trace cannot identify the exact arrival of all nine spawn columns.

## Implemented changes

- Prepare authentication while the core listener is available; overlap only
  identity-independent RakNet setup with authentication. Cancellation joins the
  worker and closes transports that were not retained.
- Prepare the local Login payload while awaiting NetworkSettings, retaining the
  required outbound packet order. Reuse public-key encoding during signing.
- Retain owned deferred packet frames without copying; preserve sibling-frame
  compaction, packet order and byte/count limits.
- Cache immutable pack presentation by content, selected files, effective
  decryption identity, exact startup inputs, language and vanilla context.
  Admission and session-owned components remain fresh. Artwork reuse requires
  the same underlying pages and actor pack.
- Schedule the complete spawn columns and their lighting halo ahead of distant
  work, including late-arriving spawn work behind a bounded ingress backlog.
  Return to camera ordering once actual local terrain is ready.
- Coalesce repeated invalidations while lighting is queued; the first mutation
  of an in-flight snapshot still rejects its obsolete completion. Preserve
  pending age, urgency promotion and dependency wakeups.
- Prove known-air lighting from trusted uniform boundary samples without
  scanning every face cell; retain the complete solver for mixed or packed
  boundaries, stale provenance and ambiguous sky input.
- Record generation-fenced Join milestones and keep the GPU probe alive until
  terrain has a later completed frame, including servers that initialize first.

Deterministic scheduler coverage completes nine spawn columns after 216 section
dispatches instead of 6,292 in a 6,936-section fixture. This measures work order,
not elapsed join time.

Local fixture medians: repeated hashing of an unchanged 32 MiB archive took
17.126 ms versus 0.011 ms for a cached fingerprint and subscriber lookup;
presentation preparation with a glyph image took 8.900 ms versus 0.042 ms on a
cache hit. These are warm same-process fixtures, not fresh admission or release
join measurements. Owned 8 MiB deferred frames fell from 138 microseconds to
0.792 microseconds in the focused fixture.

The uniform direct-sky lighting fixture replaces 1,792 boundary-cell reads with
seven uniform samples. Optimized test-profile median calls measured 41.524
microseconds for the face-scanning baseline and 0.218 microseconds for the
uniform proof. Full-solver comparison regressions cover both sky and dark
dimensions, packed boundaries, and stale trust/generation fallback.

## Verification and remaining gate

The focused tests cover cancellation, capability preservation, secret-safe
telemetry, wire order, signing, retained-buffer bounds, content/context cache
invalidation, stale generations, spawn priority and GPU witness ordering.
The opt-in live Go fixtures require `CINNABAR_JOIN_PROFILE_SERVER` and
`CINNABAR_JOIN_PROFILE_AUTH_CACHE`; they copy credentials into a private scratch
directory. `CINNABAR_JOIN_CACHE_BENCH=1` enables local cache timing fixtures.

The native join and hitch gate requires matched release captures under
[live-testing.md](agents/live-testing.md), including cold versus warm caches,
the first controllable frame, complete visible terrain and presentation
intervals. The near-terrain milestone is diagnostic and cannot alone certify
complete visible terrain. Protocol and loading behavior follow the existing
[startup rules](core-join-startup.md); this work changes computation and scheduling.
