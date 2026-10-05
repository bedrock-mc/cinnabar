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

Six additional paired attempts compared current join-time overlap against an
explicit selection 250 ms before Join. Zeno's upstream StartGame median fell
from 635.677 to 551.332 ms; Zeqa's initial endpoint fell from 388.953 to
364.873 ms. These exclude terrain and Zeqa's later transfer. A 3.149 s unfinished
Zeqa transport outlier remains in the sample; it motivated consuming only
completed, healthy preparations, with immediate ordinary-dial fallback otherwise.
The paired measurements preceded that guard and do not measure its effect.

The menu prepares only its explicitly selected server. One idle RakNet transport
expires after five seconds and is cancelled on selection change or shutdown.
No Login or Minecraft application packet is sent before Join. A successful claim
detaches expiry and shutdown cleanup from the retained concrete connection.

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

The first updated debug client reproduced the user's longer waits. Four warm
Zeno joins reached the near-terrain GPU witness in 2.161–2.527 s from the menu
action. Unchanged pack presentation unexpectedly recompiled on every join,
taking 201–291 ms; equivalent item registries arrived in different wire orders.
Two Zeqa joins transferred to `pvp.inpvp.net:19132` and reached the near witness
in approximately 9.25 and 7.42 s from the original action. The final endpoint's
terrain phase consumed 4.95 and 4.81 s despite most column delivery finishing
earlier, with thousands of lighting and meshing jobs pending. These captures
are diagnostic debug evidence, not matched release or complete-view acceptance.

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
- Compare the compiler's effective registry inputs: ignore irrelevant map/set
  order while preserving ordered palettes, permutations and duplicate winners.
  Prepare initial server UI catalogs on the worker against the exact carrier,
  retaining bounded reuse for unchanged source and carrier identities.
- Schedule the complete spawn columns and their lighting halo ahead of distant
  work, including late-arriving spawn work behind a bounded ingress backlog.
  Return to camera ordering once actual local terrain is ready.
- Recheck higher-priority deferred startup jobs even while a ready queue remains
  populated. Preserve urgency, stale-revision rejection and bounded scan work.
- Coalesce repeated invalidations while lighting is queued; the first mutation
  of an in-flight snapshot still rejects its obsolete completion. Preserve
  pending age, urgency promotion and dependency wakeups.
- Prove known-air lighting from trusted uniform boundary samples without
  scanning every face cell; retain the complete solver for mixed or packed
  boundaries, stale provenance and ambiguous sky input.
- Recognize resident air only when every storage palette entry has the exact
  air classification. Solve mixed-column upper air uniformly only beyond the
  maximum possible block-light propagation distance, retaining a dense halo
  above all potentially emitting or retained nonzero-light sources.
- Record generation-fenced Join milestones and keep the GPU probe alive until
  terrain has a later completed frame, including servers that initialize first.
  Record original-action elapsed time across automatic transfers as well.

Deterministic scheduler coverage completes nine spawn columns after 216 section
dispatches instead of 6,292 in a 6,936-section fixture. This measures work order,
not elapsed join time.

Local fixture medians: repeated hashing of an unchanged 32 MiB archive took
17.126 ms versus 0.011 ms for a cached fingerprint and subscriber lookup;
presentation preparation with a glyph image took 8.900 ms versus 0.042 ms on a
cache hit. These are warm same-process fixtures, not fresh admission or release
join measurements. Owned 8 MiB deferred frames fell from 138 microseconds to
0.792 microseconds in the focused fixture.

An equivalent reordered 2,077-icon/98-name fixture with a glyph image took
21.699 ms without reuse versus 0.998 ms with canonical input comparison. A mixed
24-section resident-air lighting fixture took 46.460 ms through the full solver
versus 18.014 ms through the prefix batch. Dense work decreased from 24 sections
to 11, with exact full-solver comparisons including emitters, layered palettes,
packed boundaries and stale trust. These fixtures do not establish live savings.

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
