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

The menu prepares the displayed default or highlighted server when Servers opens,
including mouse and keyboard selection. It retains one healthy idle RakNet
transport while selected, cancels it on selection change or shutdown, and renews
transient failures serially with bounded backoff. Unfinished setup is bounded;
explicit rejection stops renewal. No Login or Minecraft application packet is
sent before Join. A successful claim transfers the concrete connection's ownership.

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

The next native debug build reused warm presentation in 9.5–11.3 ms. Zeno's
near-terrain GPU milestones were 2.280 and 2.313 s from Join. Warm Zeqa reached
that milestone in 4.633 s across both endpoints, with 1.817 s after final-endpoint
bootstrap. Its distant light and mesh queues remained active beyond the nearby
milestone, so these results do not establish complete visible terrain.

A separate local pre-login prototype prepares a fresh proof-bound authentication
key and RequestNetworkSettings on the selected connection before Join. Login is
sent only when that exact healthy connection is claimed for the same account.
Two Zeno preparations took 386 and 429 ms before the click; subsequent upstream
StartGame took 491 and 418 ms from Connect. Warm loading release and nearby GPU
terrain took 838 ms and 2.125 s from Join. Zeqa's prepared entry hop took 184 ms,
but its second connection and terrain still produced a 4.742 s nearby milestone.
These are unmatched diagnostic samples. The prototype dependency remains local
until [the dependency change](https://github.com/HashimTheArab/gophertunnel/pull/175)
lands on the required fork branch; it is not the module pin shipped by this PR.

The combined native development client reached Zeno's warm nearby milestone in
1.912 s and transferred Zeqa's in 4.302 s from the original click. Both servers
successfully consumed the prototype connection after approximately six seconds
on the selected detail screen. This verifies those samples, not a universal
peer timeout. Zeqa's Transfer header arrived 991 ms after entry StartGame; the
next connection began about 22 ms later, attributing that wait to server delivery.

The new `view_drained` marker denotes the first quiescent admitted publisher
subset with an opaque GPU witness. Zeno reached it near eight seconds, then
announced more columns. Cold and warm column delivery continued at essentially
the same rate, reaching about 860 columns 11.46 s after bootstrap. It therefore
cannot certify final server delivery or complete visible terrain. The capture
also exposes roughly 20,000 no-op lighting jobs for further investigation.

Switching from Zeqa back to previously warm Zeno rebuilt pack presentation in
372.588 ms, versus 6.925 ms on its subsequent repeat. The presentation cache now
retains three recent immutable variants, with fresh admission and session facts
on each reuse. A failed-before returning-server regression covers this case;
native savings from that change are still unmeasured.

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
  the same underlying pages and actor pack. Recent variants use bounded LRU reuse;
  obsolete context generations are retired.
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
- Preserve already-current maximal direct sky and zero block light across proven
  air arrivals, retaining full work for uncertain or changing sources. Completion
  propagation skips known air only when its own current values are already maximal,
  so removing a roof still restores sky and removing emission still clears light.
- Prove known-air lighting from trusted uniform boundary samples without
  scanning every face cell; retain the complete solver for mixed or packed
  boundaries, stale provenance and ambiguous sky input.
- Recognize resident air only when every storage palette entry has the exact
  air classification. Solve mixed-column upper air uniformly only beyond the
  maximum possible block-light propagation distance, retaining a dense halo
  above all potentially emitting or retained nonzero-light sources.
- Publish proven resident-air sections through tracked empty-mesh results without
  a mesh worker, worker credit or neighboring lighting snapshots. Preserve the
  resident payload, revision, connectivity and normal GPU acknowledgement.
- Resolve screen policies without copying unused descendant controls and retain
  worker-prepared immutable policies for the exact catalog and context.
- Prepare the actual view's terrain pipeline variants while the world is hidden,
  and attribute synchronous pipeline processing separately from render submission.
- Record generation-fenced Join milestones and keep the GPU probe alive until
  terrain has a later completed frame, including servers that initialize first.
  Record original-action elapsed time across automatic transfers as well.
- Keep a bounded, separate publisher-view witness after nearby terrain completes.
  It requires drained admitted work and a later opaque GPU frame; it does not
  certify transparent rendering, future server delivery or controllability.

Deterministic scheduler coverage completes nine spawn columns after 216 section
dispatches instead of 6,292 in a 6,936-section fixture. This measures work order,
not elapsed join time.

Incremental nine-column air fixtures require 216 accepted lighting jobs instead
of 504, eliminating 288 unchanged results with exact full-solver comparisons.
In the warm native Zeno sample, no unchanged jobs preceded the nearby milestone;
this optimization targets later streaming work rather than proving a faster join.

A captured Zeno pack fixture's main-thread publication median fell from 10.463
to 0.622 ms after retaining prepared policies. The policy regression reduces
26,665 allocations to 39 for a screen with 1,024 unused descendant controls.
These isolated fixtures do not account for the whole native UI publication span.

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
