# Platforms and services status

[Roadmap](../../plan.md)

These topic notes retain the recorded implementation status and limitations.
An implementation entry does not close a parity or performance gate. Earlier
test results describe the recorded revision; revalidate changed code. Update the
owning section when its remaining work changes instead of appending a PR diary.

## Browser portability

Connection framing can prepare a bounded writer for a host executor, and deadlines accept a
host timer future. Native connections still spawn the writer on Tokio. Image work retains
its bounded read/decode queues, pending completion and pack cancellation. A browser image
driver runs one queued job after yielding. World jobs use the same urgent and ageing rules,
with caller-supplied monotonic observations and retained light-solver scratch; native hosts
keep their foreground/background threads and priority policy. Native cache-file reads and
FreeType compilation are excluded from browser targets. Divergent fragment samples use
explicit gradients computed before rejection.

Browser relay channels have a WebTransport adapter and a WebSocket fallback behind one
session controller. Both use the bridge codec and bounded receive/send queues. Control
resumption retains account intent and bearer expiry; play reconnects never replay packets.
Server archives can use a separate reliable WebTransport stream per join. The handoff receiver
checks count and byte admission before accepting archives, verifies streamed SHA-256 digests,
and waits for stream completion before startup. WebSocket keeps serial pack delivery on play.
Incomplete: connecting this receiver to the playable browser host and browser acceptance remain
part of the gameplay composition and release gates below.
One page driver schedules bounded retries with caller time and jitter, and cancels timers
on suspension or disposal. Explicit retries retain the account after admission or busy
refusals; expired or refused resume requires a new session. Typed control payloads share
Go/Rust wire fixtures, including idle join progress. Play opening has a host deadline;
finishing a join resets its play and pack channels while retaining control.
A valid resume replaces the prior relay controller and cancels its channels even during
transport teardown. New play waits for the old handler to finish while retaining the account.

The browser entry accepts verified memory carriers and an optional bundled font, then builds
shared world, actor, movement and HUD owners on the page canvas. A server-address field opens
one relay play lease through the existing login and packet pump. Disconnect retires gameplay
authority; bounded server transfers open fresh play generations. Canvas input enters the same
semantic router. Native pacing, processes, signing, disk caches and menu services remain native.
Renderer, geometry, UI preparation and retired snapshots use bounded browser job slots.
Pending cloud windows yield until their exact geometry is ready. WebGPU reports allocation
errors asynchronously; recoverable browser GPU exhaustion remains incomplete.

The optional sound bank uses bounded, cancellable HTTP ranges pinned to a strong ETag.
The existing decoder and mixer feed Web Audio. Headless Chrome with Metal WebGPU fetches
encoded ranges, plays nonzero game samples after a canvas gesture, suspends on cached
navigation, resumes on another gesture and closes its stream once on final disposal.
Native owner checks and a wasm artifact pass. The host boots, renders an offline world's
terrain and HUD, and moves its player. WebSocket transfer, disconnect and rejoin pass.
Incomplete: interactive inventory/chat/forms controls, initial terrain latency, shader output
comparisons, audio parity and indivisible job costs remain unverified. Cached navigation retains
page resources and restores relay controls with an unexpired capability. No browser release
or parity gate closes here.

The browser account adapter projects device-code prompts, refusals and identity into the shared
menu. External services expose account intents and preference changes without native helper
launches. Browser preferences bound saved servers and GUI scale; storage failures are reported.
The sign-in gesture bridge validates links and removes listeners on teardown. Wiring these
menu, gesture and persistence handoffs into the running browser host remains incomplete.
The browser entry preview supplies relay account controls, a browser deadline and verified memory carriers.
Memory startup requires the same bounded item-glint artwork as native startup. Cached
navigation retains page resources and restores relay controls with an unexpired capability.
Browser CI checks the web crates on wasm and runs the JavaScript edge suites. Shared
Go/Rust control fixtures cover Hello, Welcome, account, progress and error events.
The devtool static bundle command matches the locked wasm-bindgen version and includes
the canonical delivery index; TLS renewal reloads without dropping relay accounts.
Browser Dressing Room adapters provide a session-only catalog, bounded file-picker admission and
frame-sliced imports and preview preparation. PNG, companion geometry JSON and skin-pack
inputs use the native validators and content identities. Browser imports are lost when the
page closes. Native catalogs keep their existing file persistence and worker threads.
Incomplete: the playable browser host must connect the synchronous import gesture ticket
and optional starter/cosmetic carriers. Durable browser imports and in-game browser captures
remain pending; these changes do not close the browser release gate.

The browser account adapter projects checking, device-code prompts, refusals and identity
into the shared menu. External menu services expose account intents and preference changes
without native helper launches. Browser preference documents bound saved addressed servers
and GUI scale; storage failures are reported. The sign-in gesture bridge validates links,
resolves the current input action and removes its listeners on teardown.
Incomplete: these menu, gesture and persistence handoffs still need the playable browser host
and rendered browser acceptance. The current page remains the entry preview.
Incomplete: full gameplay composition, sound-bank ranges and audio playback remain pending. Relay integration
and real-browser interoperability still need acceptance tests. Shared
session/world/render clock migration and browser dependency composition still need a
wasm build gate. Shader output comparisons on WebGPU and Metal and indivisible browser
job costs remain unverified. No browser release or parity gate closes here.


## Experimental Android client

`packaging/android/build-apk.sh` builds an arm64-v8a APK holding the client library, the Go
core and `assetc` as native libraries and the prep kit as assets. A setup activity asks for
consent and runs headless first-run preparation; the NativeActivity then runs the client with
the install layout under the app's private files. Gameplay touch controls are a built-in JSON-UI
screen whose painted rectangles capture fingers; inventory and server forms take one finger as
the pointer. The Android workflow builds the APK for Android packaging changes, on a schedule
and on demand. Tagged Beta and nightly releases also publish a release-profile APK with a
persistent signing key and release checksums; Android installs updates through the APK installer.
APKs compile the shared product icon into their launcher resources. Android 11+ relaunches offer
copyable diagnostics and a report export to Downloads after an unexpected process exit. Native
reports include Android's retained tombstone on Android 12+ when available.

Incomplete: the touch layout, its sizes, the look gain and the stick behaviour are provisional,
not matched to the native touch schemes, safe areas or settings. There is no keyboard or IME,
Back routing, clipboard, browser links, local worlds, Microsoft sign-in background service,
menu touch scrolling or touch-mode form scrolling. Device frames, audio, suspend and resume,
and performance have no Android acceptance. No parity gate closes here.
The reported Galaxy S21 native crash occurs after Vulkan device creation and before confirmed
first presentation; its failing native call remains undiagnosed and device acceptance is open.


## Launcher updates

Windows runtime executables carry `asInvoker` manifests and refuse elevated game, core and
crash-monitor startup or relaunch. Setup does not launch the game, and MSI Restart Manager
relaunch is disabled. The machine-wide install identity and custom install folders remain supported.

One client per install owns update checks, Beta/Nightly/Stable channel selection, a compact corner
card with an explicit Restart and update or Download update button, and a separate default-off
install-on-quit preference. Signed manifests
carry monotonic build numbers, and digest-named partial downloads resume with HTTP ranges.
An offline Rust helper waits for the old process, applies the verified native package, and
restores failed macOS/Linux starts. Read-only installs offer the manual download action.
Windows uses this helper even without direct write rights: Windows Installer handles any
administrator authorization after shutdown, and the helper owns the client relaunch.
Manual Windows downloads open their folder so they cannot start an installer over a running client.
Each launch checks once startup completes; the fallback poll runs every six hours, and a Cinnabar
API release notice for the selected channel checks immediately.

Nightly checks both the nightly and beta signed manifests and offers the newer release while
keeping the Nightly preference. Release ordering compares the version core, then the shared
publication number, then SemVer prerelease precedence. An unavailable or invalid beta manifest
leaves nightly usable. Cached offers and retained stages use the same order to prevent automatic
downgrades when the winning publishing channel changes. Beta remains the default channel.

Incomplete: CI must compile and run the written regression tests on all platforms. A real Windows
check must confirm permission consent, installation and one relaunch. The package workflow
publishes each channel's signed manifest. The `CINNABAR_UPDATE_SIGNING_KEY` secret in
the protected `release` environment and the `UPDATE_TRUSTED_KEYS` repository variable are
configured. The first signed publication remains pending.
The launcher owner must review the notice screenshots before merge.


## RakNet transport compatibility

The Go core, local server, and fixture generator share a reviewed transport revision with
corrected handshake addresses and probe cadence, connected control framing, reliability
and ordering, peer GUID generation, datagram batching, and outgoing split envelopes.
The consumers now also include the merged fresh-entropy GUID, offline reply validation,
final MTU selection, retry deadline, and split-memory fixes. A wire regression exercises
the effective dependency, and manifest checks keep the three Go module pins together.

Full transport parity remains incomplete. Host-resolver timing and address admission,
absolute monotonic timestamp origin, and discovery-peer reuse remain provisional across
platforms. These limits and the unresolved server-routing behavior remain open gates.


## Login identity

- New core-owned logins pass the full stored ClientRandomId and existing install device
  ID to gophertunnel, which derives SelfSignedId. Unchanged inputs produce the same
  identifier across joins.
- Login names omit the legacy ThirdPartyNameOnly marker while preserving ThirdPartyName.
  Complete claim presence, ordering and platform lifecycle parity remain incomplete.
- Rust leaves SelfSignedId to the core. Skin uploads reuse the selected pack and
  image name at login and on skin updates, independently of authentication identity.
- Authentication-state reuse transitions and full platform device lifecycle parity remain
  incomplete. Skin, account and device claims retain their existing owners.
- Persona and platform-specific imported-skin behavior remain incomplete.


## Xbox Minecraft activity

- The Rust client reports menus and the committed world default, Realm and experience
  classification to the account core. Go publishes through the existing Xbox presence API;
  offline accounts do no service work and failures never block joins or gameplay.
- Fake-client tests cover joins, default-mode changes, leaving, cancellation and title
  cleanup. The control regression failed on the previous implementation. Rust state
  tests distinguish the world default from an individual player's mode.
- Named `COM_Experience_*` IDs are intentionally not sent. Featured servers and
  Experiences use `COM_Experience` without server-identity mappings.
- Exact current-version game-mode input, update trigger ordering, heartbeat cadence,
  platform-specific service configuration selection, server activity overrides and
  permission gates remain incomplete. The current heartbeat
  fallback is provisional. Signed-in friends-visible validation is incomplete, and no
  Xbox presence parity gate closes. Discord presence is unchanged.


## Compatibility landing

- Education construction terrain restores allow, deny and all border wall states
  from the pinned current palette, using the textures already in the fetched pack.
  Collision and light read the pinned metadata sources; no guessed shapes or
  sequential palette IDs are added. Linux captures at 1280×720 with GUI scale 2
  show textured allow/deny cubes and connected border shapes. Broader Education
  parity remains incomplete.

- The owner accepted the final live macOS Metal build and authorized landing the
  accumulated server compatibility changes. Formatting and architecture checks
  pass. Known test compilation errors were corrected; further local tests and
  waiting for CI were explicitly waived by the owner. Validation is incomplete.
- Earlier entries record the state when each fix was introduced. Native parity
  and performance limits marked incomplete below remain open.


## NetherNet server trust

- Plain-http NetherNet joins ask vanilla's first-use trust question through the JSON-UI modal
  popup, with the keys stored as vanilla stores them. Provisional deviation: an answer that takes
  longer than 5 s redials the server with the trusted key, because a dedicated server drops an
  idle negotiation within about ten seconds; vanilla's handling of a slow answer is unconfirmed.


## Servers tab experiences

- The Servers tab lists the ServerTab layout's experiences, joined by experience ID. The Go core
  now consumes typed player counts, using the service client's five-minute cache and retaining
  the last successful values on refresh failure. Counts are requested for visible experience
  details on an independent worker, so Home cannot delay them; background layout reads do not
  request them. The existing selected-details binding shows only positive counts as plain
  decimal numbers, without capacity or digit grouping.
- Incomplete parity: the OreUI experience banner still needs its count badge wired by its screen
  owner; this change only supplies data and existing JSON-UI bindings. The menu rereads counts
  through its independent featured worker every 30 seconds; exact refresh dispatch timing remains
  unverified. Listing-only experience details still need their linked detail page. No visual or
  performance gate is closed.


## Friends tab worlds

- Friends' worlds follow the vanilla list rule (`p2p.World.Listed`): members and a host, the player's
  own session only for a Realm, broadcast 3/4 always and 2 for friends. Incomplete parity: friends'
  Realm and experience sessions are left out because joining them from the friends tab is not
  implemented.


## Windowed Metal presentation

- macOS enables bounded asynchronous presentation for opaque, windowed Immediate
  surfaces on macOS 14 and later. Three reusable color targets decouple rendering
  from display drawable recycling; a separate queue copies the newest GPU-completed
  image at the original resolution and format. Superseded completed images are
  reusable, while pending writes and presentation reads retain ownership.
- Surface reconfiguration cancels and joins the display consumer. Allocation or
  worker setup failure uses the existing direct surface path. FIFO and native
  fullscreen retain direct presentation.
- The optimized build and live maximized-window rendering capture pass on M3 Pro;
  the owner confirms uncapped windowed play. Pool ownership regressions are authored
  but unrun at the owner's request. Mode transitions, broader hardware, sustained
  pacing and latency acceptance remain open.


## Remote session reconnect

- Reconnect is a requested menu extension on remote disconnect and connection-failure screens.
  Its OreUI panel preserves formatted, wrapped reasons and pins Reconnect and OK.
  It retries the selected destination through the normal join route and retains the originating
  page. OK and Escape dismiss the failure. Account changes and local-world exits clear retry.
- Behavioral regressions cover repeated attempts, teardown, transfers and account ownership.
- macOS rendered input checks passed at 1280×720: keyboard and pointer retry completed
  loopback joins after refusal and server disconnect; OK, Escape and loading Cancel worked.
  This requested extension does not establish native UI parity.


## Partial server faces keep the diagnostic material

A failed face no longer sinks its whole visual: cubes keep rendering with
the diagnostic material on failed faces, and model quads with failed
textures stay in the mesh instead of collapsing the visual. One missing
texture still counts once per visual, and server rasters keep precedence.
Random-offset cubes with failed faces skip carried-face publication, since
diagnostic carried faces would fail session validation and drop the whole
overlay. Failed faces never consume layers: materials are only pushed for
admitted textures, so the texture checkpoint rollback for fully diagnostic
visuals is unchanged.

Regression coverage: per-face cube and model tests asserting the surviving
kind, the diagnostic face material, session application, and gap counts;
both fail on the previous implementation. Existing suites updated to the
new kind expectations with unchanged gap totals. Block-item sheets still
fall back to placeholders for diagnostic faces; inventory 3D icons raster
the surviving mesh with diagnostic quads.

Failed model faces preserve the selected material's two-sided flag even
when a texture is missing or the material budget is full. Regression
coverage checks render methods, rotated and axis-aligned quads, and session
application.


## Import player data from Minecraft

The launcher offers a one-time Import or Not now choice when a known user-data
directory exists. File contents are read only after an explicit import action.
General Settings retains Import from Minecraft and Revert actions. Import previews
one selected profile with readable category counts in a bounded, scrolling modal.
Import requires a separate confirmation; cancelling returns to the preview. It merges server endpoints, validates and copies installed
resource packs, and preserves the chosen active order. A durable snapshot restores
settings, servers and active pack order without removing copied packs.

Supported preferences use a schema-checked mapping table. Primary desktop keyboard
and mouse bindings use the existing binding validator. Unknown settings, secondary
key assignments, controller and touch layouts, encrypted packs and invalid or
over-limit entries are counted and skipped. Existing pack identities are retained.

Incomplete: complete option and control-layout parity, Android scoped-storage
imports, custom launcher data roots, and verification of every supported desktop
location against a live install. This feature does not close a parity gate.


## Packet flight recorder and capture

Sessions record the last 4,096 packet headers in both directions, with each packet's
accept, skip or failure outcome, and keep up to four rejected bodies per packet ID from
the latest session that rejected one. `CINNABAR_PACKET_CAPTURE` writes a capture v2 that
`bedrock-replay` reads alongside v1, optionally on the recorded schedule. Its queue bounds
record count and owned body bytes, including the active disk write, and detaches admitted
bodies from their source batches. Captures that require an unavailable blob cache are
rejected before replay starts. Login, handshake and pack-offer bodies never reach samples
or captures.

Crash reports carry the ring and samples: the header ring and the log flight recorder (last 2,000 lines,
`debug` for net, session, core and packs under its own per-layer filter) are lock-free
rings in mapped files under `crashes/.live-<session id>/`. A background writer coalesces
sample changes from a store bounded by body bytes and sample count; ingress never waits
for persistence, and contention losses are counted. The monitor and the in-process fallback attach `packets.txt`,
`decode-samples/` and `flight.log` to every folder; the crash window lists the last packets
from `packets.txt`. Verified locally on Windows: ring layouts round-trip through their
readers, header recording into the mapped ring allocates nothing, and the crash-monitor
fixtures (panic, access violation, stack overflow, fail-fast abort, external kill, early
in-process panic) each carry the fixture's packets, sample and recorder line. Criterion,
shared build host under load: a recorder-only `debug` event costs about 1.7 µs through the
installed subscriber (both filters, formatting, mapped write); a disabled event about 2 ns.

Incomplete: recorded times are when the client reads each batch, not socket arrival;
no frame or join capture has measured the recorder's cost. Folders from fatal Unix
signals written in-process lack the three attachments;
Linux and macOS fixtures run only in CI; the recorder cost is not measured on an idle host
or in a frame or join capture; the Settings → Advanced "Record network session" option is
not built; recorded times are when the client read each batch, not socket arrival.


## Shared redaction

`crates/crash-report/data/redaction-rules.json` is the one redaction rule set: credentials
(authorization of any scheme, XBL3.0, bearer, JWTs, token/ticket/session id/content key
fields) and private data (IPv4/IPv6 with ports, hosts in address/target fields and bare
hostnames, home folders, XUIDs, gamertag and other identity fields), all replaced by
`[redacted]`. `crash_report::redact` compiles it and owns the share-time engine (learned
values, escape layers, JSON keys) that `devtool diag` and the crash window's Copy report use.
The Go core embeds a byte-for-byte copy in `core/internal/redact`; a crash-report test fails
when it drifts, and the file's conformance vectors pass in both languages, with each output
stable under a second pass. Go `redact.Error` replaces `control.RedactError` and the
resource-pack handler's URL pattern; `redact.Secret` logs as the placeholder and
`redact.Private` as a per-process keyed hash (`[redacted:xxxxxxxx]`) that the share-time rules
keep, so records about one server still correlate. Upstream targets, transfer hops, the
`core starting` upstream, the friend XUID and the raw upstream, session, friend and web-relay
errors log redacted, with regression tests asserting no address or XUID in the output.

Verified: crash-report, devtool diagnostic-bundle and crash-monitor summary tests; Go tests for
`internal/redact`, `control`, `launcher`, `webrelay`, `cmd/bedrock-core` and `proxy`.
Incomplete: Go applies only the single pattern pass; learned values and escape decoding are
Rust share-time only, so an escaped credential in a Go error relies on the JWT and XBL rules.
No Rust log site was found carrying a credential field, so no `Secret` wrapping was added; the
`developer-control` and `launcher-host` tokens sit in `Debug` structs that are never logged.
The core still logs its socket directory and catalog path in clear; share time redacts the
`endpoint` value whole but only the home folder name in the catalog path.
