# Rust Bedrock Client (Bevy + Go Core) — Master Implementation Plan

> **For agentic workers:** This is a program-level master plan. Phases 1–8 are sub-projects;
> each gets its own detailed task-by-task plan (per superpowers:writing-plans) when its turn
> comes, executed via superpowers:subagent-driven-development or superpowers:executing-plans.
> Phase 0 is fully detailed here and is executable directly.

**Goal:** A performant, vanilla-parity Minecraft Bedrock client for macOS/Linux/Windows that
joins current third-party servers (RakNet), Realms and friend worlds (NetherNet), and creates
local worlds — built as a Rust/Bevy renderer in front of a Go core derived from Lunar.

**Architecture:** The Rust client owns everything visual and interactive (rendering, world
model, input, UI, audio, one pinned protocol codec). The Go core owns everything network and
identity (Xbox/PlayFab auth, RakNet, NetherNet, Realms, friends, protocol conversion,
resource-pack negotiation), reusing gophertunnel/go-xsapi/go-nethernet unchanged. They talk
over a local byte-stream socket (UDS on macOS/Linux; named pipe or 127.0.0.1 TCP on Windows)
carrying plain Bedrock packets pinned to ONE protocol version, plus a small control channel.
Local worlds run dragonfly behind the same core, over the same client path.

**Tech Stack:**
- Client: Rust, Bevy (wgpu), rayon (meshing), axolotl-stack `valentine` packet defs (1.26.30)
- Core: Go, `lunar` gophertunnel + go-raknet fork; upstream `df-mc/go-nethernet` and `df-mc/go-xsapi/v2`; dragonfly
- Boundary: socket-file transport already implemented in `bedrock-mc/plugin` (reference impl)
- Assets: Mojang/bedrock-samples (full vanilla resource pack); `refs/pocketmine/bds-data`
  for server data (`definitions/`, `blocks.json`) and a live BDS test server

## Global Constraints

- Pinned loopback protocol version: **1.26.30** (bumps are deliberate, lockstep with a core release; the core's gophertunnel protocol conversion absorbs upstream server version variance).
- The Rust side NEVER implements auth, encryption-to-upstream, RakNet-to-upstream, or NetherNet. If a task seems to need one of those in Rust, the task is wrong.
- The loopback game channel is Bedrock packets with length-prefixed framing; no RakNet on this leg. Encryption on this leg: whatever gophertunnel's Listener does by default — do not fork to remove it; AES on loopback is negligible.
- Single source of truth for protocol/data lives in the Go estate: packet truth = gophertunnel (validated against Mojang bedrock-protocol-docs via `cmd/protocoldrift`); block/item/biome registries = generated exports from dragonfly; client packet defs = valentine (docs-generated), conformance-tested against gophertunnel bytes.
- New Go types get doc comments at creation (contract on the type, one-liner per method).
- `lunar` gains at most ONE new consumer (the core). Respect the frozen-facade/ABI rules in `platform/Lunar/AGENTS.md`.
- **Go relay source rule:** copy Lunar's `lunar/internal/relay/relay.go` package logic and
  its relay tests into this repository's Go core (target: `core/internal/relay`), recording
  the exact Lunar source commit. Preserve its forwarding, transfer, resource-pack, and
  lifecycle behavior. Replace only its tiny `lunar/utils` panic-helper dependency with a
  local equivalent. **Do not import `github.com/lunar-bedrock/lunar` or add Lunar as a Go
  module dependency; the copied relay package is the only Lunar code the core consumes.**
- Never edit anything under `refs/` (read-only).
- Vanilla-behaviour parity is the spec language; no decompile/RE provenance in code, commits, or public docs.
- Model routing (per workspace CLAUDE.md): bulk/mechanical implementation → gpt-5.5 via codex skills; anything user-facing (UI, menus, copy) and plan/impl reviews → fable-5/opus-4.8 taste bar.

## Repos and Layout

- **`bedrock-mc/client`** (new greenfield repo; do not reuse `bedrock-mc/Rust-LCE` code, assets, renderer, or world model because it targets the Legacy Console Edition rather than current Bedrock/BDS data):
  - `crates/protocol/` — vendored/generated valentine 1.26.30 defs + login-sequence state machine
  - `crates/world/` — chunk store, sub-chunk decode, block registry, light engine
  - `crates/render/` — meshing, atlas, chunk/entity/sky rendering (Bevy plugins)
  - `crates/sim/` — movement physics (bedsim-parity port)
  - `crates/assets/` — vanilla + server resource-pack loading (models, textures, sounds, lang, fonts)
  - `crates/ui/` — menus, HUD, inventory, forms, chat
  - `crates/bridge/` — socket transport (from `bedrock-mc/plugin`) + control-channel client
  - `app/` — the Bevy application binary
  - `core/` — **Go module**: the core service; copy/adapt Lunar's relay package and logic as donor code, but never import Lunar as a module dependency
  - `tools/` — Go: registry/asset exporters, conformance fixture generator
- **`platform/Lunar`** — small additions only: anything the core needs exposed through the facade; conformance fixture corpus generator may live in `cmd/`.
- **Decision log** (settled in design discussion, 2026-07-09/10): hybrid over pure-Rust (single protocol treadmill, reuse of auth/NetherNet/Realms/physics estate) and over pure-Go (renderer ecosystem); docs-generated codec over gophertunnel-AST codegen (docs are machine-emitted wire descriptions; gophertunnel Marshal funcs are not generatable); socket file over TCP loopback (permissions, no ports); valentine defs adopted as-is for v1 (Hashim PR'd 1.26.30 to axolotl) — vendor/fork decision deferred until Phase 1; conformance harness deferred to Phase 1 (Phase 0 spike acts as the manual conformance test).

## Risk Register (tracked, each owned by a phase)

| Risk | Phase | Mitigation |
|---|---|---|
| Bevy meshing/frame-pacing insufficient | 0 | Spike acceptance gates the whole program |
| valentine defs drift from gophertunnel bytes | 0→1 | Spike surfaces; Phase 1 builds automated conformance harness |
| Dragonfly registry sequential IDs differ from valentine protocol-1001 palette IDs | 1→2 | Resolved: validated runtime assets now derive the unique canonical air identity for both sequential (`13094`) and hashed (`0xdbf44120`) sessions; the checked-in BREG→compiler→blob regression prevents stale protocol constants from turning air into diagnostic geometry on third-party servers |
| Client-side lighting (Bedrock sends no light data) is a full subsystem | 2 | Scoped task; flood-fill block/sky light, correctness vs vanilla screenshots |
| Molang/entity animation scope explosion | 4 | v1 = molang subset for vanilla mobs; static fallback pose; explicit cut-line |
| Sound binaries not fully in bedrock-samples | 8 | Audit early (Phase 2 asset task); fallback = user-supplied client assets import step |
| Windows transport (no tokio UDS) | 1 | Transport behind trait/enum; named pipe or TCP flavor chosen at startup |
| axolotl-stack bus factor | 1 | Vendor valentine output (generated code) into `crates/protocol`; upstream fixes when friendly |
| dragonfly vanilla worldgen parity | 7 | v1 local worlds = dragonfly's gen as-is; parity gaps documented, not chased |
| Bevy 0.x quarterly breaking releases | all | Pin per phase; upgrade as a deliberate task, never mid-phase |

---

## Phase 0 — Spike: prove the stack end-to-end (DETAILED, executable now)

**Goal:** Bevy app connects through a core process to a real server, decodes chunks with
valentine defs, meshes and renders a 16-chunk radius with acceptable frame pacing.
**This phase gates the program.** Failure modes and their outs: defs drift → fix
gophertunnel/defs (expected, fine); frame pacing fails → revisit meshing strategy before
any other phase proceeds.

**Files:**
- Create: `bedrock-mc/client` repo — `app/` (Bevy spike), `crates/protocol/`, `crates/bridge/`, `core/` (minimal Go main)
- Reference (do not modify): `bedrock-mc/plugin` (socket transport), `platform/pc-client` (lunar embedding), `libs/gophertunnel/minecraft/dial.go`, `libs/dragonfly` `chunk` package (sub-chunk decode reference)
- Test server: `refs/pocketmine/bds-data/bedrock_server-1.26.32.2` (run a local BDS) or a dev Lunar upstream

**Interfaces (produced for later phases):**
- `core/`: Go binary `bedrock-core` — flags `-socket-dir <dir> -upstream <host:port>`; listens with `minecraft.Listener` on the socket transport, dials upstream via `minecraft.Dialer`, forwards packets both ways at pinned protocol (this is a ~200-line pc-client-shaped proxy main for the spike; productized in Phase 1)
- `crates/bridge`: `fn connect(socket_dir: &Path) -> anyhow::Result<FramedStream>` where `FramedStream: Stream<Item=Bytes> + Sink<Bytes>` (length-prefixed batches)
- `crates/protocol`: `fn decode_batch(bytes) -> Vec<Packet>`, `fn encode(packet) -> Bytes`, `enum Packet` (valentine-generated), plus `LoginSequence` state machine: `RequestNetworkSettings → Login (self-signed chain, no XBL) → handshake → ResourcePackClientResponse (decline/none for spike) → await StartGame → RequestChunkRadius(16) → await spawn → SetLocalPlayerAsInitialized`
- `crates/world` (spike-minimal): `SubChunk::decode(&[u8]) -> SubChunk` (paletted storages), `Chunk { sub_chunks: Vec<SubChunk> }`

**Tasks (each = write failing test → run → implement → pass → commit):**

- [x] **0.1 Repo scaffold.** Cargo workspace + `core/` Go module; CI stub (`cargo test`, `go test ./...`). Complete at `41112b2` (review clean).
- [x] **0.2 Spike core proxy.** Complete at `823bf49` (live BDS join passed; lifecycle hardening review clean). Go: socket-transport `net.Listener` (port from `bedrock-mc/plugin`) + `minecraft.ListenConfig{AuthenticationDisabled: true}` + upstream `minecraft.Dialer` forwarding loop. Test: Go integration test dials the socket with gophertunnel's own client, joins local BDS through it. Run: `go test ./core/... -run TestProxyJoin -count=1`. This test is load-bearing: it proves the core path with a known-good client before Rust enters.
- [x] **0.3 Bridge crate.** Complete at `7ce7309` (17 Rust unit tests + Go echo integration; review approved). Rust: connect + framing. Test: echo fixture against a Go test binary serving the same transport. `cargo test -p bridge`.
- [x] **0.4 Protocol crate: vendored defs + decode smoke.** Complete at `a7bbfac` (five exact gophertunnel fixtures; review approved). Vendor valentine 1.26.30 generated output. Test: decode a fixture corpus of gophertunnel-encoded packets (generate fixtures with a small Go tool in `tools/fixturegen` — encode one of each: NetworkSettings, StartGame, LevelChunk, MovePlayer, AddActor). Any decode failure here = defs/gophertunnel drift: adjudicate against bedrock-protocol-docs, fix gophertunnel upstream or patch defs, record in `crates/protocol/DEVIATIONS.md`.
- [x] **0.5 Login sequence.** Complete at `1fa35ee` (encrypted Rust bridge login, strict protocol-1001 conformance fixtures, bounded malformed-input handling, independent review approved). `LoginSequence` reaches StartGame through the spike core. With `BEDROCK_BDS_DIR` set, `cargo test -p protocol --test login --locked -- --nocapture` builds the Go external-client harness, starts/stops core+BDS itself, and verifies clean shutdown.
- [x] **0.6 Sub-chunk decode.** Complete at `7d9248a` (12 reproducible goldens from pinned Dragonfly, packed/paletted v1/v8/v9 decode, atomic sparse chunk ingestion, 28 Rust world tests, three independent reviews approved). Runtime storage remains palette + packed words and preserves high-bit network block hashes without a flat per-block array.
- [x] **0.7 Spike renderer.** Complete at `f2a6a1c` (400 Rust tests, strict all-target Clippy, independent review approved, and live fly/input pass recorded). First extend `crates/world` with packed-palette `UpdateBlock`/`UpdateSubChunkBlocks` mutation and full-column eviction APIs; expand each changed key through `mesh_dependents` before remeshing. Bevy app: consume LevelChunk and SubChunk responses → decode → cull-meshing on rayon → vertex buffers → draw untextured (per-runtime-ID debug colors); fly camera. Pure meshing remains unit-tested. Use Computer Use for a live interaction pass covering window focus/capture, keyboard inputs, fly movement on every axis, mouse-look yaw/pitch, and clean input release (no stuck movement or rotation); the acceptance run below remains the end-to-end renderer gate.
- [ ] **0.8 Acceptance run.** Connect to BDS world, render 16-chunk radius, fly at speed, break/place blocks from a second client to force live remeshing. Repeat the Task 0.7 Computer Use interaction checklist in the live streamed world and record the result. Before the run, resolve the recorded `AvailableCommands` live drift and add/fix `MaterialReducer` output-count conformance coverage from `crates/protocol/DEVIATIONS.md`. **Gate: p99 frame time ≤ 8ms on the dev MacBook at 16 chunks; remesh of a modified sub-chunk visible ≤ 100ms; zero decode errors over a 15-minute session (or all errors adjudicated as 0.4-style findings and fixed).** Record numbers in the phase report.
  - Windows portion passed at `3898530`: 900.0015 s, radius 16/16/16, p99 5.1 ms, 432/432 visible mutations, max mutation-to-visible 45.4522 ms, zero decode errors, clean shutdown. Phase status is **CONDITIONAL GO**; this checkbox remains open only for the authoritative dev MacBook p99 run.

**Exit criteria:** acceptance gate met; deviations documented; go/no-go written up. Everything after this phase is "build the game", with the architecture de-risked.

---

## Phase 1 — Core service (Go): productize the boundary

**Goal:** `bedrock-core` becomes a real service the client can ship: session lifecycle, auth,
control channel, conformance harness. Deliverable proof: a headless Go CLI (`corectl`) can
device-code-auth, list Realms/friends, and join any of the three transport targets through
the core — before any more Rust exists.

Scope (detailed plan to be written at phase start):
- Control channel on `control.sock`: protobuf or JSON-RPC; methods — `Status`, `StartAuth` (device-code events streamed), `SignOut`, `ListServers`, `ListRealms`, `ListFriends` (gophertunnel realms package + go-xsapi sessions; the join side of what go-mcxboxbroadcast does), `Connect{target}`, `Disconnect`; events — auth state, connection state, transfer notices, disconnect reasons.
- Session lifecycle: begin by copying Lunar's `lunar/internal/relay/relay.go` and relay tests
  into `core/internal/relay`, adapting only imports/helpers needed to make the package
  standalone. The core uses that relay logic to dial upstream (RakNet / NetherNet via Xbox
  signaling / Realms address), serve the game socket, and handle transfers. Lunar remains a
  source donor, never a module dependency.
- Resource-pack negotiation upstream; pack payloads handed to client over the control channel as files in a cache dir (client applies them — Phase 6 renders them).
- Windows transport flavor (named pipe or TCP) behind the same listener interface.
- **Conformance harness (promoted from deferral):** `tools/fixturegen` grows to full packet coverage; CI job round-trips gophertunnel↔valentine bytes both directions on every core and defs bump. This is the automated version of spike task 0.4.
- Consumer-surface work in `platform/Lunar` to expose what the core needs through the facade (measured, minimal, per AGENTS.md ABI rules).

**Early authenticated RakNet smoke (Zeqa):**

- [x] Add an explicit, ignored Microsoft token cache and optional authenticated upstream
  dial mode while preserving the offline BDS path when `-auth-cache` is omitted.
- [x] Document the exact `bedrock-core` and release `bedrock-client` commands, device-code
  stdout flow, cache privacy requirements, and Rust → local socket → Go → RakNet boundary.
- [x] Report the core's startup lifecycle synchronously: Go build start, process/auth state,
  published local endpoint, local Rust-client acceptance, and upstream connect/success/failure.
  Commit `46a4e9f` covers ordered, secret-safe logging and fatal-startup tests.
- [x] Live smoke: authenticate `bedrock-core` to `zeqa.net:19132` with
  `.local/auth/microsoft-token.json` and confirm the current release client reaches Zeqa.
- [x] Record non-secret live evidence below; never record the device code, access token,
  refresh token, or token-cache contents.

Live evidence:

- Date/time: 2026-07-11 19:17 PDT
- Authenticated upstream connection observed: yes; the authenticated protocol-1001 entry
  connection returned Zeqa's pre-login transfer to `pvp.inpvp.net:19132`, and the bounded
  core transfer follower completed the regional connection.
- Client reached Zeqa lobby/session: yes; the release client reached position
  `(-117.50, 87.62, 195.50)`, streamed `1105/5376` chunks while the count continued rising,
  and held approximately 100 FPS. A native Windows screenshot was inspected from the user
  temp directory and was not added to the repository.
- Credential hygiene (`git ls-files .local` empty; no credential material in retained logs):
  passed. The token cache remained inside the ignored `.local/` tree, its contents were never
  inspected, and the temporary device-code stdout log was removed after authentication.

This early direct-RakNet smoke does not close the phase-wide control-channel, `corectl`,
Realms, friends, NetherNet, general/post-login transfer, or sign-out work above.

Exit: `corectl join --friend <gamertag>` works from a clean machine; conformance CI green.

## Phase 2 — World rendering (textured, lit, real)

**Goal:** the spike renderer becomes the real world pipeline. Deliverable: fly through any
live server world and it *looks like Minecraft*.

Scope: block registry + block-state → model/texture mapping (generated export from Dragonfly's registry via `tools/registrygen`, shipped as a binary asset, pinned PMMP BedrockData as the exact protocol-1001 canonical palette/property/biome cross-check, Axolotl Valentine's versioned typed-state approach as a state-selector/catalog reference, and Axolotl's exact pinned PrismarineJS Bedrock collision shapes as reviewed cuboid-template/occlusion inputs—not render/UV authority); vanilla asset ingestion from **Mojang/bedrock-samples** pinned to the matching game version (terrain textures, `blocks.json`, flipbooks, and biome colors) — NOTE: the pinned samples contain no block-render model JSON, so deterministic reviewed family generators combine these sources and vanilla-reference evidence; BDS `resource_packs/vanilla` is server-minimal (blocks.json + texts only), a data reference rather than the texture source; 2D texture array pages + per-layer mipmaps; greedy/culled meshing with transparency layers (opaque/cutout/blend) and per-face culling; **client-side light engine** (block + sky flood-fill, per-vertex light, day/night); biome tinting (grass/foliage/water); sky, fog, clouds; chunk streaming/eviction tied to `ChunkRadiusUpdated` + `SubChunk` request flow; block entities with custom renderers deferred (chests/signs get static models in this phase). Zuri is not a rendering or asset-system input.

**Phase 2 progress (kept current as work lands):**

- [x] **Sequential/hash third-party block identity compatibility.** Runtime
  classification derives canonical air from the validated compiled registry
  instead of the stale protocol bootstrap constant. The checked-in production
  path proves sequential air `13094` and hashed air `0xdbf44120`, rejects
  ambiguous/decoy AIR records, and covers both `WorldStream` modes. This closes
  the all-pink-air failure seen on sequential-ID third-party servers while
  preserving hashed servers.
- [x] **2.1 Local-only vanilla source and deterministic asset pipeline.** Pinned
  `bedrock-samples` provenance, Dragonfly registry export, pack parsing, bounded
  compiler, per-layer mips, versioned runtime blob, and diagnostic fallback are
  implemented; Mojang payloads remain ignored.
- [x] **2.2 Opaque full-cube render path.** Material-aware binary greedy meshing,
  exact eight-byte quads, one shared material buffer/texture array/bind group,
  vertex-pulled repeating UVs, oriented faces, and live asset selection are
  implemented. Two current-HEAD 60-second Windows radius-16 runs passed with
  p99 4.1 ms and zero errors; see `docs/phase-2-texture-slice-report.md`.
- [ ] **2.3 Close the opaque texture slice.** The deterministic named-block BDS
  gallery now passes with all faces/log axes, greedy repetition, mips, supported
  and diagnostic cases recorded, and the clean no-assets full gate passes. The
  fail-closed material path, local relay 1,600-packet ceiling, and deterministic
  inbound/command network arbitration are implemented and independently reviewed.
  A 2026-07-11 interactive radius-16 run at `00b7a32` reached world-ready with
  zero missing mappings, but is diagnostic rather than acceptance evidence:
  849,117 rendered quads used material zero across 9,040 resident/7,093 visible
  subchunks, and exact inspection of blob SHA-256
  `1fbd361c489d3cf90edb49c0056b83ffd9a2a114a36ac1eaf28cfd1103ecf508`
  found only 661 of 16,913 registry visuals
  mapped to real materials. Evidence is in
  `.local/acceptance/20260711T192110Z-16912/app.stdout.log`. Most of that visible gap
  belongs to Tasks 2.4–2.7 (leaves, tint/grass, water/blend, and models). The exact
  two-second teleport/full-view remesh gate and fresh combined RSS/steady-CPU
  evidence remain open; close those findings before completing Task 8.
- [ ] **2.4 Cutout cube materials and leaves.** Preserve independent geometry,
  occlusion, and cave-connectivity semantics; keep the packed subchunk/quad and
  shared GPU architecture. Tasks 1–4/5 are complete at `f768cfa`, `4d23356`,
  `f33b71c`, and `8391a58`: the versioned
  registry now exports independent air, cube-geometry, full-face-occlusion,
  and leaf-model facts with exact pinned counts, and leaf-only cutout materials
  now use coverage-preserving per-layer mips. Palette-native `u64` meshing now
  applies ordered leaf/opaque culling and non-occluder cave connectivity without
  widening the eight-byte quad. The existing single opaque shader now applies
  bit-8 alpha cutout with depth writes and no blending. No Mojang payload is
  tracked. The deterministic live-evidence task remains open.
- [ ] **2.5 Biome palettes and tinting.** Decode/store biome data and apply
  grass/foliage/water tint without widening the eight-byte quad record.
  - [x] Palette-native v1001 biome storage/column decoding, including padded
    Bedrock words, `0xff` previous-storage reuse, strict malformed-input
    rejection, atomic inline block+biome commits, and biome-only column
    lifetime independent of all-air block subchunks.
  - [x] Carry request-mode and inline `LevelChunk` biome payloads through the
    Rayon/FIFO streaming path, decode the full dimension column independently
    of requested block count, and commit it before subchunk requests.
  - [x] Retain the live biome definition mapping needed to resolve palette IDs
    to climate and vanilla tint rules, including bounded custom-biome fallback.
  - [x] Remove the grass-block diagnostic fallback: compile bottom/top/side
    independently, preserve grass-side alpha as an opaque tint mask through
    mip generation, and apply the pinned pack's deterministic default grass
    tint until live per-biome color lookup replaces it.
  - [x] Compile grass/foliage/water tint classifications and biome color rules,
    upload palette-native biome/tint tables, and apply them in the chunk shader
    without widening the eight-byte quad record. Grass plus generic/birch/
    evergreen/dry foliage are now resolved from `MCBEAS03`, revision-gated,
    and applied palette-natively. **Complete (2026-07-13):** Task 13's real
    animated water route applies the live palette-native water tint in the
    liquid shader without widening the eight-byte cube quad. Native run
    `20260712T203607Z-7596` proved five runtime water tints, consecutive exact
    GPU witnesses, generation 518 presented, p99 14.0 ms, and zero decode
    errors; fresh assets/render/client focused suites remain green.
  - [ ] Add vanilla-reference biome tint blending for grass, foliage, and
    water. Determine the matching Bedrock radius/kernel from native reference
    evidence, sample the bounded palette-native biome neighbourhood across
    chunk boundaries without flattening columns, preserve special foliage
    rules and custom-biome fallback, and cover abrupt-boundary, missing-neighbour,
    teleport/eviction, GPU, and performance cases.
    **Bounded implementation landed; parity evidence remains open (2026-07-14):**
    grass, generic foliage, and water now share a radius-one horizontal 3x3
    linear-colour box blend across self-contained palette-native neighbour
    snapshots. The descriptor deduplicates equal packed payloads, clamps a
    missing neighbour to the center's nearest edge, records a uniform fast path,
    retains direct birch/evergreen/dry-foliage selection, and validates all nine
    immutable source identities before publication so neighbour replacement,
    eviction, and teleport churn fail stale. Request-mode biome-only commits now
    dirty resident cross-column consumers. The eight-byte cube quad and packed
    Bedrock storages remain unchanged; a uniform record is 52 bytes, or
    1,359,072 bytes for 33x33x24 subchunks at radius 16, while the exact
    descriptor ceiling and the existing GPU arena cap bound adversarial palettes.
    The 3x3 kernel is explicitly provisional because no reviewed native Bedrock
    radius/weight evidence was available; keep this checkbox open until a native
    abrupt-boundary reference fixes or confirms the kernel and the live
    performance/visual gate passes.
- [ ] **2.6 Static/non-cube models, blend/water, and flipbooks.** Complete the
  remaining block visual classes and animation path per
  `docs/superpowers/specs/2026-07-11-phase-2-6-noncube-water-design.md`.
  - [x] Pin and securely acquire the exact local-only PMMP, PrismarineJS,
    Axolotl, and Dragonfly evidence bundle. Whole-bundle atomic publication,
    byte/hash/time bounds, junction rejection, concurrent-winner handling,
    exact license notices, and the no-tracked-payload contract are complete at
    `c44de03`.
  - [x] Preserve complete bounded flipbook metadata and compile the real pinned
    pack's physical frames into deterministic page-aware staging data without
    changing the v3 runtime schema. Commits `143c68d` and `e6e49e1` cover 83
    animations, 1,209 physical frames, 1,323 timeline references, and 1,901
    deduplicated layers on one 2,048-layer page.
  - [x] Export and strictly decode `BREG1003` typed model/contributor/selectors,
    face coverage, collision seeds, and per-state provenance at `e58d083`.
    PMMP/Dragonfly/Prismarine form a full 16,913-state/1,356-name bijection;
    Valentine is an exact ordered 15,845-state subset with 1,068 attributable
    missing states across 35 wholly absent names and zero extra/mismatched
    states. The deterministic registry SHA-256 is
    `3669be82850824af8592276afe864d903495e743b8af81dfcf1d3aa1586231a4`.
  - [x] Version the bounded runtime asset schema to `MCBEAS04`; compile the
    typed registry selectors, template tables, page-aware flipbook data, and
    attributable per-family diagnostics without committing Mojang payloads.
  - [x] Upload the bounded one/two-page `MCBEAS04` texture resource, immutable
    material/animation/frame tables, and a stable 16-byte animation clock in
    one shared chunk bind group. Commit `a30a0ef` adds page-aware current/next
    frame selection, cross-page interpolation and wraparound, a real diagnostic
    second-page fallback, atomic asset-revision replacement, derivative-safe
    WGSL sampling, and no per-frame texture upload; 82 render tests, strict
    Clippy, and independent spec/quality review are green.
  - [x] Generalize bounded chunk rendering to named cube, model,
    model-lighting, liquid, and liquid-lighting streams while preserving the
    eight-byte greedy cube record. Commit `5734872` adds exact combined byte
    accounting, one consolidated word-addressed geometry arena, transactional
    all-stream allocation/rollback/retry, generation/tint gates, expected/drawn
    presentation masks, and identical direct/MDI addressing. The projected
    vertex storage-binding count including future templates is seven of the
    common minimum eight; 89 render tests, strict Clippy, and independent
    re-review are green.
  - [x] Produce stable eight-byte face-specific model/liquid lighting sidecars
    from a palette-native center-plus-26-neighbour snapshot. Commit `8b5c5a6`
    bakes exact Phase 2.6 block-light 0 / sky-light 15 values plus per-vertex AO,
    registers generation-scoped diagonal AO/liquid dependency masks, and covers
    inline columns, known-air replacement, stale rejection, and conservative
    unknown targets. Render 93/93, world 51/51, client 187/187, strict combined
    Clippy, and independent re-review are green; Phase 2.7 will replace only the
    light inputs, not this format or addressing.
  - [x] Add palette-native multi-layer contributor resolution, retaining the
    eight-byte greedy cube record and adding compact model/liquid streams with
    atomic queue/GPU generation accounting and direct/MDI parity. Task 10 now
    resolves up to 16 packed storage layers without a flat 4,096-block array,
    fails closed on contributor conflicts, and retains simultaneous primary and
    liquid contributors. All three seagrass states and all 26 kelp ages compile
    with exact animated material identities; kelp head/body selection is driven
    only by the primary block above, including across subchunk boundaries. The
    deterministic 29-state BDS water-tank gallery passed from both directions
    at current HEAD with zero target diagnostics/decode errors, p99 15.5 ms,
    377,843,712-byte peak combined RSS, 5.78% mean combined CPU, and native temp
    screenshots confirming real green cutout models. Water geometry remains
    deliberately invisible until Tasks 11–13.
  - [x] Track the exact bounded liquid mesh neighbourhood and invalidation set.
    Task 11 keeps the shared palette-native `world::MeshNeighbourhood` as the
    render API boundary, exposes the deduplicated 23-subchunk liquid sample set
    (current/upper horizontal 3x3 plus lower center and four cardinals),
    applies its checked inverse for liquid-only diagonal dirtying, preserves
    ordinary six-face cube invalidation, coalesces duplicate rapid updates, and
    rejects stale dependency masks. World, client, formatting, strict Clippy,
    and independent review are green; no flat block or whole-column snapshot was
    introduced.
  - [x] Compile crossed cutout plants/crops with exact variants and biome tint;
    compile all physical flipbook frames into texture-array layers and animate
    them from immutable descriptors without per-frame texture uploads. Commit
    `a24370b` covers all 443 terrestrial Cross/Crop states (279 Cross, 164 Crop)
    with zero diagnostics, reusable two-quad templates, compact model refs,
    face-specific lighting, a shared bounded/no-cull direct+MDI GPU path, and an
    exhaustive hash-bound gallery; assets 112, render 102, world 51, client 187,
    acceptance, strict Clippy, and final independent re-review are green.
  - [x] Mesh animated, biome-tinted water from the shared bounded palette
    snapshot. Task 12 preserves all 16 water depth/falling states, vanilla-like
    weighted four-corner surfaces, diagonal and cross-subchunk influence,
    same-water/solid culling, clipped sides and bottoms, signed flow gradients,
    waterlogging, still/flow per-face materials, stable stream order, and one
    face-specific eight-byte lighting sidecar per 16-byte liquid quad. The
    liquid dependency set was corrected to the exact 23 samples needed by
    lower-cardinal waterfall flow. Only all-face alpha-blended, water-tinted
    families enter this stream; lava remains attributable diagnostic until its
    Task 19 depth-writing route. Seventeen liquid integration tests, full locked
    affected-crate suites, exact strict Clippy, real 16,913-visual asset compile,
    and final independent re-review are green. Water remains deliberately
    invisible until Task 13 installs the transparent GPU path.
  - [x] Mesh animated, biome-tinted water with same-liquid culling, vanilla-like
    corner heights, diagonal invalidation, and a correctly ordered transparent
    phase with depth testing and no depth writes. Its deterministic BDS gallery
    requires one real water tint referenced by the committed/presented liquid
    snapshot (BDS commands cannot assign fixture biomes); a separate end-to-end
    app integration test proves two raw biome IDs with distinct map-water
    colours preserve dense lookup and distinct renderer tint records. Water
    visibility churn and generation-only remeshes retain the last ordered
    snapshot only while every absolute address stays physically resident: same
    key/metadata, active tint, valid lighting, and a same-start liquid range
    containing the old range. Moved/shrunk ranges, eviction, metadata reuse, or
    asset/tint replacement use bounded copy-on-write quarantine: retired spans
    remain drawable and unreusable until an empty-or-nonempty replacement frame
    is submitted and its independent GPU retirement epoch completes. Cap
    exhaustion backpressures the update/removal rather than risking stale reads
    or a blank transparent frame.
    Gallery freezes its 60-second duration/frame metrics at the original
    deadline, then permits at most two unmeasured seconds for a non-empty
    committed=encoded=GPU-presented transparent generation; timeout is a
    logged nonzero failure. Its manifested p99 frame-time gate is exactly
    1000/60 ms.
    **Task 13 complete (2026-07-12):** after fixing bounded GPU-upload
    starvation and four-word liquid-stream arena alignment, exact four-key GPU
    witnesses passed repeatedly across `WaterGalleryFront` and
    `WaterGalleryBack`. Commit `38c1f5d` moved the block-constant packed biome
    tint lookup from the fill-heavy liquid fragment stage to a flat vertex
    varying without changing tint, alpha, ordering, uploads, or lifecycle
    bounds. Native run `20260712T203607Z-7596` froze exactly 60 seconds and
    passed at p99 14.0 ms (limit 16.667 ms), with 38,214 transparent refs,
    five runtime water tints, consecutive exact four-key GPU witnesses,
    request=result=committed=encoded=presented generation 518, zero ceiling
    rejects, zero decode errors, radius 16, 414,187,520-byte peak combined RSS,
    and 6.438% mean combined CPU. Full render/client tests, strict Clippy,
    acceptance tests, and independent re-review through test-hardening commit
    `ba3ea3f` are green with no findings.
    **Camera-motion regression closed (2026-07-13):** exact floating-point view
    keys no longer discard a safe same-address transparent snapshot midway
    through its bounded inactive-slot upload. The staged generation now commits
    atomically before the latest pose is requested, while allocation, asset,
    tint, or stream-address changes still cancel immediately. This prevents
    newly streamed water from starving and appearing or disappearing with
    camera movement (`a4f7da5`; regression-first test, full 278-test render
    suite, strict Clippy/formatting, and diff check green). A fresh native BDS
    camera-motion capture is required below before closing live evidence.
    **Native rerun integration follow-up (2026-07-13):** the first DX12 water
    rerun exposed two independent GPU setup defects introduced by the new
    transparent-model path. The opaque packed-model pipeline now explicitly
    selects `fragment` after `model.wgsl` gained the separate
    `fragment_blend` entry point. Debug DX12 also uses the equivalent direct
    cube/model/depth-liquid draw path because wgpu 27's indirect validator
    expands a 20-byte indexed command to 32 bytes for D3D12 special constants
    while its debug batching assertion still advances by 20; release DX12 and
    unaffected backends retain multi-draw indirect. Both failures have
    regression-first coverage, the full 279-test render suite and strict
    Clippy/formatting are green, and independent review found no blocking
    issues. The corrected native run stayed alive and continuously committed
    transparent snapshots through generation 187 / 31,915 refs instead of
    panicking or starving. It still timed out at the 180-second fixture-ready
    marker while loading 7,934/9,132 debug-path subchunks, and the required
    fresh GDI capture remained pure black under the already-isolated RX 570
    Bevy/wgpu presentation failure, so this is correctness evidence rather
    than visual closure.
  - [ ] Add compact static templates in impact order: slabs/stairs,
    wall-attached vines/lichen/sculk-vein and related thin faces,
    doors/trapdoors, connection-aware panes/fences/gates, then static
    chest/sign models; retain conservative culling/connectivity for partial
    models until exact face-coverage optimization is separately verified.
    - [x] Slab asset templates: all 272 BREG1003 bottom/top/double states now
      compile through opaque six-quad packed model templates with exact
      face-specific materials, UV crops, boundary cull flags, deterministic
      deduplication, and zero pinned-pack slab diagnostics (`c64330b`; assets
      tests, strict Clippy/formatting, and independent review green).
    - [x] Slab packed rendering and occlusion: lower/upper/double slabs remain
      compact model references with six lighting sidecars and no cube stream;
      double slabs provide full-face cave/cull occlusion while partial slabs
      remain conservatively cave-open. Internal and all six cross-subchunk
      model/cube boundaries are covered without lighting reindexing
      (`6df380d`, `09279a1`; 122 asset tests, 44 render-mesh tests, strict
      Clippy/formatting, and independent review green). Gallery acceptance
      remains part of Task 14 before the parent item can close.
    - [x] Stair templates and neighbor-derived straight/inner/outer selection:
      all 512 BREG1003 states across 64 names compile through compact five-shape
      groups per material/upside signature, with exact S/W/N/E transforms,
      both Dragonfly side-isolation guards, same-half matching, all four
      horizontal cross-subchunk boundaries, selected-template lighting, and
      conservative cave connectivity (`859fb13`, `0475516`, `e1732eb`,
      `1766a56`, `469695b`). The exact pinned pack has zero stair diagnostics;
      real-pack assets/render tests, the 43-witness/five-pose deterministic
      gallery, strict MCBEAS04 integrity/tamper gates, full PowerShell dry-run
      acceptance, strict Clippy/formatting, and independent re-review are green.
    - [x] Door and trapdoor templates: all 672 door states and 336 trapdoor
      states compile through compact six-quad alpha-cutout cuboids with exact
      typed open/orientation/hinge/half selection, 3/16-block thickness,
      lower/upper door materials, conservative partial-model culling, and
      deterministic template reuse. Legacy oak-through-iron door texture arrays
      and modern bamboo/cherry/mangrove/pale-oak/nether/copper/waxed aliases are
      covered. Dragonfly's rotated door-state encoding is inverted before its
      logical-facing/open-hinge transform; an independent review caught and
      corrected the initial direct-orientation interpretation before push.
      Missing or out-of-range selectors fail closed, collision-only seeds are
      not used as render authority, and the real-pack exhaustive gate removes
      exactly 1,008 diagnostics with no additions (`1a69fca`; 145 assets tests,
      strict Clippy/formatting, full 16,913-state ratchet, and independent review
      green). Deterministic gallery/native GPU evidence remains in the shared
      residual-family live gate.
    - [x] Connected wall templates: all 5,184 states across 32 wall materials
      decode the exact 9-bit north/east/south/west none/short/tall selector and
      center-post bit into deterministic zero-to-30-quad packed models. Visible
      bounds come from the local vanilla `template_wall_post`,
      `template_wall_side`, and `template_wall_side_tall` render models rather
      than Dragonfly/Prismarine collision boxes: post-off states omit the post,
      short arms reach 14/16, tall arms reach full height, and UV projection
      follows the vanilla UV-locked blockstate contract. Invalid selectors fail
      closed, partial-model culling remains conservative, collision-seed removal
      leaves output byte-identical, and the real-pack ratchet removes exactly
      5,184 diagnostics with no additions (`09ba163`; 148 assets tests, strict
      Clippy/formatting, full 16,913-state ratchet, and independent correction
      review green). Deterministic gallery/native GPU evidence remains in the
      shared residual-family live gate.
    - [x] Pressure-plate templates and typed pressed selector: BREG1003 now
      preserves `redstone_signal` only for the 256 pressure-plate states as an
      explicit unpressed/pressed flag, without affecting redstone wire or any
      other record. All 16 material families compile through two deterministic
      opaque templates using the vanilla `pressure_plate_up/down` bounds and
      exact UV crops, including the pressed model's half-texel side strip;
      missing/invalid selectors fail closed and collision data is not render
      authority. The selector-only registry regeneration is byte-reproducible,
      changes exactly those 256 records, and the real-pack ratchet removes all
      256 pressure-plate diagnostics with no additions (`4c83afd`; registry SHA
      `fda4b40335c24b0019049ce572668b03f8ddb9a705de88abb4d724aa7ff81106`,
      152 assets tests, 23 strict-coverage tests with one real-blob gate ignored
      by default, registrygen tests/vet, strict Clippy/formatting, and independent
      correction review green). Deterministic gallery/native GPU evidence
      remains in the shared residual-family live gate.
    - [x] Fence-gate templates and bounded compound model references: all 192
      states across 12 materials require the exact typed `Orientation`, `Open`,
      and in-wall flag mask, fail closed on missing, invalid, or additional
      selectors, and compile from the vanilla render-model oracle rather than
      collision boxes. Closed/open and normal/in-wall forms preserve exact
      UV-locked geometry; bamboo uses its distinct custom 38/40-quad topology
      and reversed/rotated UV rectangles. Because exact gates exceed the
      existing 32-quad mask, one visual now selects a validated pair of
      consecutive 24+16 (bamboo closed 22+16) templates, emitted as two
      independent packed model references with contiguous lighting and draw
      records while preserving the 16-byte reference, `u32` visible mask,
      MCBEAS04 field widths, and GPU shader contract. Encoder/runtime trust
      boundaries reject empty, truncated, nested, incompatible, or directly
      referenced continuations. The production 16,913-state ratchet removes
      exactly 192 gate diagnostics with no additions and now holds 8,301
      diagnostics including air (`f4bcfe0`, `1aaf952`; full assets/render and
      visualcoverage suites, strict Clippy/formatting, real pinned-pack run, and
      independent review/re-review green). Deterministic gallery/native GPU
      evidence remains in the shared residual-family live gate.
    - [x] Connection-aware pane and fence templates plus transparent model
      streaming: all 43 pane/bar states select one of 16 exact post-and-arm
      masks, and all 13 fence states select compact post plus connection-arm
      templates while preserving wood/nether connection classes. Internal and
      all four horizontal cross-subchunk seams suppress only true pane joins;
      fences connect to full occluders, matching fences, and only the sides of
      axis-aligned gates. Mixed connected-template flags fail closed. Alpha
      admission is descriptor-scoped so stained panes retain blend materials
      without accidentally admitting full stained-glass cubes that share the
      same texture path; reviewed beacon and liquid routes remain intact.
      Alpha-blended model quads now reuse the same packed model references and
      lighting sidecars but enter a dedicated no-depth-write phase, sorted
      back-to-front by retained view and face. Sorting runs through a
      latest-wins Rayon worker cache keyed by exact CPU/GPU generation and
      stream identity; water and model uploads share one whole-subchunk,
      per-frame transparent-reference budget. The production 16,913-state
      ratchet removes exactly 56 diagnostics with zero additions and leaves
      8,066 diagnostics including air (`a2c3a5a`, `5024f21`; full assets/render
      suites, strict Clippy/formatting, pinned-pack ratchet, and independent
      review/re-review green). Deterministic gallery/native GPU evidence
      remains in the shared residual-family live gate.
    - [x] Carpet and stateful pale-moss-carpet templates: all 17 ordinary
      stateless carpets compile as exact opaque 1/16-block cuboids with the
      pinned wool/moss aliases, while all 162 pale-moss states enforce the
      exact four `none`/`short`/`tall` side properties and upper-bit contract.
      Pale bases stay opaque; side planes use the pinned two-entry cutout pair
      in its verified tall/short order, render two-sided with conservative
      connectivity, preserve the isolated-upper base-plus-four-tall special
      case, and quantize vanilla's unrepresentable 1.6/256 inset symmetrically
      to 2/254. Missing, invalid, extra, or mismatched typed selectors fail
      closed; collision seeds do not affect rendering. The production ratchet
      removes exactly 179 carpet diagnostics with no additions and now holds
      8,122 diagnostics including air (`8087b6a`, `9323093`, `9e99a5e`; exact
      opposing and direction-specific Java UV corner orders, two byte-identical
      pinned builds, full assets/visualcoverage suites, renderer regressions,
      strict workspace Clippy/formatting, and independent final re-review green).
      Deterministic gallery/native GPU evidence remains in the shared
      residual-family live gate.
    - [x] Button templates and exact wall UV locking: all 168 states across 14
      materials enforce the exact `Orientation` plus pressed-flag mask and
      canonical schema, fail closed on missing/extra/invalid selectors, and map
      Bedrock's six outward-facing values to deterministic floor, ceiling, and
      four wall transforms. Unpressed and pressed forms use the exact vanilla
      bounds and face rectangles, with the unrepresentable 1.02-pixel pressed
      depth deliberately quantized to one pixel. Wall faces derive UV-locked
      rectangles from rotated target bounds; independent literal six-face
      goldens cover all four directions and both pressed states after review
      caught and corrected the initial source-space projection. Materials stay
      opaque, partial models advertise no boundary culling/coverage, and
      collision seeds are not render authority. The production ratchet removes
      exactly 168 button diagnostics with no additions and now holds 7,898
      diagnostics including air after integrating the already-landed 56-state
      pane/fence tranche (`8b427eb`, `fe55779`; deterministic pinned
      builds, full assets/render/visualcoverage suites, strict Clippy/formatting,
      and independent final re-review green). Deterministic gallery/native GPU
      evidence remains in the shared residual-family live gate.
    - [x] Canonical huge-mushroom cube states: all 48 states across brown
      mushroom blocks, red mushroom blocks, and mushroom stems now select the
      pinned pack's exact six-face material aliases from the canonical tagged
      `huge_mushroom_bits` integer. Missing, extra, untagged, mistyped,
      noncanonical, or out-of-range selectors fail closed. The focused
      production-pack gate preserves diagnostics for all 43 legacy flags-zero
      cube records, all 25 stained-glass/copper-grate/slime transparency-family
      cubes, and `minecraft:invisible_bedrock`; record reordering remains
      byte-deterministic. The production ratchet removes exactly 48 intended
      diagnostics with zero additions. After integrating the already-landed
      128-state glow-lichen/sculk-vein tranche, that historical checkpoint held
      7,722 diagnostics including air (full assets/visualcoverage suites, pinned
      compiler tests, strict Clippy/formatting, and zero-delta refreshed ratchet
      green).
    - [x] Ordinary stained-glass cubes: the exact stateless
      `minecraft:black_stained_glass`, `minecraft:blue_stained_glass`,
      `minecraft:brown_stained_glass`, `minecraft:cyan_stained_glass`,
      `minecraft:gray_stained_glass`, `minecraft:green_stained_glass`,
      `minecraft:light_blue_stained_glass`,
      `minecraft:light_gray_stained_glass`, `minecraft:lime_stained_glass`,
      `minecraft:magenta_stained_glass`, `minecraft:orange_stained_glass`,
      `minecraft:pink_stained_glass`, `minecraft:purple_stained_glass`,
      `minecraft:red_stained_glass`, `minecraft:white_stained_glass`, and
      `minecraft:yellow_stained_glass` records now render as alpha-blended
      six-quad models. Palette-native meshing suppresses a shared face only for
      an equal six-face material identity under the checked transparent-cube
      semantic, preserves both cross-colour boundary faces, culls glass behind
      full opaque neighbours without hiding the opaque face, stays cave-open,
      and applies across all six subchunk boundaries. Education `hard_*` glass,
      stained-glass panes, copper grates, slime, legacy flags-zero cubes, and
      `minecraft:invisible_bedrock` remain excluded. The production ratchet
      removes exactly these 16 IDs with zero additions, leaving 7,706
      diagnostics and 7,235 cumulative removals; the ignored integrated blob is
      SHA-256
      `61025bb3e8e1b9ca0d5e2ec1cd7847433333a20f99948c6193fbb370a0d4900f`.
    - [x] Copper grates: the exact stateless `minecraft:copper_grate`,
      `minecraft:exposed_copper_grate`, `minecraft:weathered_copper_grate`,
      `minecraft:oxidized_copper_grate`, `minecraft:waxed_copper_grate`,
      `minecraft:waxed_exposed_copper_grate`,
      `minecraft:waxed_weathered_copper_grate`, and
      `minecraft:waxed_oxidized_copper_grate` records now use homogeneous
      alpha-cutout six-quad transparent-cube templates. Waxed variants retain
      the exact unwaxed face-material aliases, while shared-face culling uses
      exact network identity so wax and oxidation boundaries remain visible in
      sequential and hashed modes. Grates stay cave-open, route only through
      ordinary depth-writing model draws, cull against identical states across
      all six subchunk boundaries, and preserve opaque-neighbour asymmetry.
      Slime, stained/hard glass, panes, copper bars/bulbs/doors/trapdoors,
      unrelated grate names, legacy flags-zero records, and
      `minecraft:invisible_bedrock` remain outside this admission. The
      production ratchet removes exactly eight IDs with zero additions, leaving
      7,698 diagnostics and 7,243 cumulative removals; the ignored integrated
      blob SHA-256 is
      `20cd1b4301f40736468a3249acf21fdea0544d74fa238d8faae04aaee1af9940`.
    - [x] Chiseled bookshelves: all 256 canonical
      `minecraft:chiseled_bookshelf` states (sequential IDs 1,605–1,860) now
      compile from the exact `books_stored:int 0..63 × direction:int 0..3`
      product into 64 immutable north-facing templates and four opaque source
      materials. Each template has five ordinary full faces plus six coplanar
      front-slot quads; native 1.26.33.1 evidence fixes bit order to top-left,
      top-middle, top-right, bottom-left, bottom-middle, bottom-right and fixes
      directions 0/1/2/3 to south/west/north/east. Exact pair/static terrain
      access, unit collision, flags, face coverage, typed state, ID formula,
      and complete-family cardinality all fail closed. Ordinary and six-quad
      front faces cull across every subchunk boundary, the full model closes
      cave connectivity, and the dense-subchunk fixture emits exactly 1,352
      model refs and 2,816 visible quad refs with stable 11-record lighting
      spans. The exact production ratchet removes only IDs 1,605–1,860 with
      zero additions, leaving 2,570 diagnostics including air. Registry SHA-256
      is `3e0a67718b6368d8b5f7755e9e49a1241233f21bcea8724a9163febb4f1b1d92`;
      the ignored compiled pack SHA-256 is
      `df82f3408ee5805bcd536a484b6d0e8831eb972d76225c17eda005695e4d982c`.
      - [ ] Live presentation acceptance: capture Cinnabar from the matching
        native-gallery viewpoints and require two consecutive exact
        GPU-completed model-stream witnesses. Keep both native and Cinnabar
        screenshots local-only; do not commit Mojang-derived imagery.
    - [x] Resin clumps: all 64 canonical `minecraft:resin_clump` states
      (sequential IDs 2,930–2,993) now require the exact typed
      `multi_face_direction_bits:int 0..63` product, formula IDs, empty flags and
      face coverage, empty collision, and the exact scalar/static
      `resin_clump` texture route. Native 1.26.33.1 support-removal/readback
      fixes bits 1/2/4/8/16/32 to down/up/south/west/north/east, matches the
      glow-lichen UV projection, and proves that a written zero mask reloads as
      63. The compiler preserves every protocol record while aliasing mask 0 to
      mask 63, emitting one static alpha-cutout material, 63 templates, and 192
      quads. Sequential and hashed mesh gates cover every mask, all six
      boundaries, cave openness, opaque-support visibility, layered water, and
      the dense 4,096-reference/24,576-draw-light bound. The exact production
      ratchet removes only IDs 2,930–2,993 with zero additions, leaving 2,506
      diagnostics including air. Registry SHA-256 is
      `33a31ec89a04fe638a4f59ab315561c1c0d897e04f2041d5643262d3de56d30c`;
      the ignored compiled pack SHA-256 is
      `91998c61a9f8c40a72e73e45167d7448e9ad18271b561bc61f8d839584603e19`.
      - [ ] Live presentation acceptance: reproduce the native resin viewpoints
        in Cinnabar and require two consecutive exact GPU-completed model-stream
        witnesses. Native and Cinnabar screenshots remain local-only.
    - [x] Reviewed selector-alias opaque cubes: validate all 38 records in the
      complete hay, bone, quartz-block, smooth-quartz, chiseled-quartz, purpur,
      and TNT products, then promote exactly 27 compatibility states. Exact
      typed wrappers, raw keys, values, formula IDs, Primary/Cube facts, shape 1
      CollisionOnly unit bounds, complete-product cardinality, and exact
      static/non-tinted/opaque vanilla pack descriptors fail closed. Native
      1.26.33.1 evidence preserves Y caps down/up, X caps west/east, Z caps
      north/south, and a quarter-turn on all four non-cap faces for X/Z;
      `deprecated=0..3` and `explode_bit=0..1` are static visual aliases.
      Sequential/hash rendering covers every state, all six cross-subchunk
      culls, dense six-quad greedy output, cave closure, and zero model,
      transparent, and liquid streams. The exact production ratchet removes
      only IDs 2,908-2,910, 2,912-2,914, 2,916-2,918, 5,443-5,444,
      6,466-6,468, 6,470-6,472, 6,474-6,476, 7,082-7,083, 13,113,
      14,686-14,687, and 15,345-15,346 with zero additions, leaving 2,479
      diagnostics including air. Registry SHA-256 is
      `9f67a14d73cf958b53557cc31c601168aa0eb95c5d46dfac1299f8412a0cb74f`;
      ignored compiled-pack SHA-256 is
      `18a4718d6fd03a66c0eb30e0a28444dcf80159c658cf4f7712e5ff342f7740ca`.
      - [ ] Live presentation acceptance: reproduce the matching native axis,
        TNT, and deprecated-state viewpoints and require two consecutive exact
        GPU-completed cube-stream witnesses. Screenshots remain local-only.
    - [x] Exact cactus family implementation: all 16 canonical
      `minecraft:cactus` states (sequential IDs 13,606-13,621) now require the
      complete exact `age:int 0..15` product, formula IDs, Primary/Cuboid
      ownership, empty flags and face coverage, exact shape-84 collision, and
      exact static side/down/up pack routes. Native 1.26.33.1 evidence fixes the
      visible X/Z inset to 1/16, full Y height, and side UV crop to source
      columns 1..14. Every age reuses one six-quad template and three static
      alpha-cutout materials. Sequential/hash, all-boundary, opaque-adjacency,
      cave-open, additional-water, dense 4,096-reference/24,576-draw-light,
      deterministic-registry, deterministic-pack, and exact 2,479 -> 2,463
      visual-coverage gates are green. No Mojang payload or screenshot is
      tracked.
      - [ ] Live presentation acceptance: reproduce the matching native cactus
        overview, stack, grazing, and top-inset viewpoints in Cinnabar and
        require two consecutive exact GPU-completed model-stream witnesses with
        stable generation/ref counts and zero contamination counters.
    - [x] Exact cake family implementation: all seven canonical
      `minecraft:cake` states (sequential IDs 14,055-14,061) now require the
      complete exact `bite_counter:int 0..6` product, formula IDs,
      Primary/Cuboid ownership, empty flags/coverage, exact collision shapes
      89-95, exact six-face block routing, and literal untinted terrain pairs.
      Native 1.26.33.1 evidence fixes west as the advancing cut plane and binds
      bite zero to `cake_side` versus bites one through six to `cake_inner`.
      Seven immutable six-quad templates use
      `[16+32*b,0,16]..[240,128,240]`; sequential/hash, all-boundary,
      opaque-adjacency, cave-open, additional-water, dense
      4,096-reference/24,576-draw-light, deterministic-registry,
      deterministic-pack, and exact 2,463 -> 2,456 visual-coverage gates are
      green. No Mojang payload or screenshot is tracked.
      - [ ] Live presentation acceptance: reproduce representative bite 0, 1,
        and 6 native viewpoints in Cinnabar and require two consecutive exact
        GPU-completed model-stream witnesses with stable generation/ref counts
        and zero contamination counters.
    - [x] Exact farmland family implementation: all eight canonical
      `minecraft:farmland` states (sequential IDs 6,122-6,129) now require the
      complete exact `moisturized_amount:int 0..7` product, formula IDs,
      Primary/Cuboid ownership, empty flags/coverage, exact shape-43 collision,
      and literal untinted side/top routes. Native 1.26.33.1 evidence binds
      amount zero to dry terrain-array index 1 and amounts one through seven to
      wet index 0. Two immutable six-quad templates use full X/Z and 15/16 Y;
      sequential/hash, all-boundary, opaque-adjacency, cave-open,
      additional-water, uniform/mixed dense 4,096-reference/24,576-draw-light,
      deterministic-registry, deterministic-pack, and exact 2,456 -> 2,448
      visual-coverage gates are green. No Mojang payload or screenshot is
      tracked.
      - [ ] Live presentation acceptance: reproduce representative moisture 0,
        1, and 7 native viewpoints in Cinnabar and require two consecutive
        exact GPU-completed model-stream witnesses with stable generation/ref
        counts and zero contamination counters.
    - [ ] Slab/stair native and packed-GPU live acceptance: capture all five
      fixed Cinnabar poses through native `%TEMP%` screenshots and require two
      consecutive exact GPU-completed model-stream witnesses. Automated gallery
      construction is complete. The Top pose now reaches two consecutive exact
      7-key GPU witnesses with 276 model references and zero missing, stale,
      wrong-stream, zero-reference, or draw-mismatch counters after moving the
      camera teleport ahead of the synthetic fixture-update flood. The five
      inspectable native captures and a clean performance-gate run remain open;
      the first repaired live run was rejected at 138.3439 ms mutation-to-visible
      against the 100 ms gate. Audit found that the already-complete exact model
      witness remained armed throughout the later timed session, rebuilding a
      full frame probe over thousands of instances every frame. The probe now
      disarms immediately after its exact two-frame pair, and gallery publication
      now waits for an exact Rust-side committed-camera marker before sending the
      fixture-update flood. Unit, full-workspace, acceptance dry-run, and runtime
      safety regressions are green; a fresh native five-pose rerun remains open.
      North run `20260714T011758Z-840` then passed its exact camera fence,
      77-command result fence, and consecutive GPU model witnesses (sequences
      907/908, seven keys, 277 refs, all contamination counters zero), but the
      timed gate still failed at p50 41.7 ms / p99 47.6 ms and 140.1161 ms
      mutation-to-visible. Its 55-second camera delay was a bounded Rust ingress
      bottleneck (four queued packets and eight admissions per rendered frame),
      not relay reordering; the channel and per-frame admission window are now
      coherently 32, matching the existing heavy-event cap while preserving FIFO
      order and decode/mesh worker budgets. GPU cost and fresh native visual
      evidence remain open. The first GPU-side correction now rejects all padded
      and neighbour-masked slots in the model vertex stage before template,
      lighting, texture, tint, and fragment work; the fixed 32-quad/reference
      storage contract remains bounded while a live A/B measures the reduction.
      Optimized North run `20260714T013915Z-6480` reduced teleport
      acknowledgement-to-ingress/commit to 8.18 seconds (sequence 2,128, with
      ingress and commit in the same update) and again passed the exact model
      witness. Vertex culling improved p50 from 41.7 to 39.6 ms despite 8,699
      versus 5,679 resident subchunks, but p99 remained 47.7 ms and the 100 ms
      mutation gate still failed at 139.9718 ms. Structural exact-count model
      drawing is now complete in `fcb1989` and ownership-hardening `b07e194`:
      one exact eight-byte visible-quad indirection record replaces the fixed
      32-quad vertex launch while preserving 16-byte model refs, ordered
      lighting, one direct/MDI command per subchunk, arena/COW bounds, and model
      witness semantics. Full render/client tests, strict Clippy/format/shader
      validation, release build, and independent review/re-review are green.
      Live VineGallery run `20260714T030538Z-22388` passed exact GPU witnesses
      at sequences 191/192 (four keys, 95 stable refs, all contamination
      counters zero) but measured p50 40.6 ms, p99 47.7 ms, and 142.6 ms
      mutation-to-visible with 8,345 resident subchunks—neutral versus the prior
      p50 40.3 / p99 47.8 / 138.9454 ms run. Exact drawing therefore closes the
      required packed per-quad architecture but not the performance gate; GPU
      stage timestamps/workload counters must identify the remaining cost before
      considering a one-sided/two-sided pipeline split. Resident and
      frustum-visible model workload counters are now implemented: acceptance
      JSON distinguishes 16-byte model refs from exact eight-byte quad draw refs
      and reports the former fixed 32-quad slot invocations avoided. Full
      render/client suites are green. Acceptance/profiling runs now also enable
      Bevy's asynchronous DX12 timestamp recorder and report paired, deduplicated
      p50/p95/p99/max timings for the chunk-containing opaque and transparent 3D
      passes without blocking the GPU. Live VineGallery North run
      `20260714T032404Z-12360` recorded 1,296 GPU samples: combined opaque plus
      transparent was 4.9 ms p50 / 10.2 ms p99 (10.54048 ms max), while full
      frame time remained 40.2 ms p50 / 47.6 ms p99. Its 29,083 visible model
      refs issued 80,233 exact quad draws and avoided 850,423 of the former
      930,656 fixed-slot invocations (91.38%); resident totals were 63,327 refs,
      161,125 draws, and 1,865,339 avoided invocations. The exact-draw path is
      therefore effective and model shader work is not the remaining frame-time
      bottleneck; do not add a speculative one/two-sided model pipeline split.
      The run again passed adjacent exact GPU witnesses (sequences 801/802,
      four keys, 92 refs, zero contamination), stayed within the RSS/CPU budget
      at 638,537,728 bytes and 2.82% mean CPU, and failed only the shared 100 ms
      mutation gate at 142.5286 ms. The next performance investigation must
      target frame scheduling/presentation and mutation-to-frame latency.
      Two process-scoped pacing A/Bs on the same approved DX12 runtime ruled
      out unsafe workarounds: AutoNoVsync run `20260714T033621Z-14584` never
      reached the world-ready/mutation fence, while FIFO with
      `WGPU_DX12_USE_FRAME_LATENCY_WAITABLE_OBJECT=dontwait` in run
      `20260714T034035Z-22400` reached the clean gallery/camera fence but never
      produced a GPU-completed model witness and resisted graceful shutdown.
      Keep wgpu's default waitable-object behavior for correctness; surface
      acquisition/presentation remains the evidence-backed external blocker.
      The backend/presentation investigation is now
      conclusive: five direct swapchain captures from Cinnabar, minimal Bevy
      Camera3d and Camera2d clear-only probes, a camera-local red clear, and
      DX12/FXC were byte-identical pure black. Vulkan exposes no surface present
      modes and GL exposes no adapter on this machine. This isolates the native
      black-window symptom below Cinnabar to Bevy 0.18.1/wgpu DX12 on the RX 570
      driver `31.0.21924.61`; chunk, camera, shader, and custom render-phase code
      must not be changed to mask it. A driver or isolated Bevy/wgpu A/B plus a
      tiny startup clear-color smoke gate remains required before native visual
      evidence can close, while deterministic GPU witnesses can continue.
    - [ ] Wall-attached vine family: replace the diagnostic pink-cube fallback
      for every `minecraft:vine` direction-bit state with compact cutout face
      templates selected from its exact attachment mask, including conservative
      cave connectivity, cross-subchunk neighbours, texture/UV parity, and a
      deterministic gallery plus native screenshot/GPU evidence. Extend the
      same reviewed thin-face route to glow lichen and sculk vein separately;
      do not collapse their distinct state/property contracts into vine logic.
      **Implementation complete; live gate open (2026-07-13):** all 16 masks
      compile to foliage-tinted two-sided attachment planes with exact UV axes,
      zero diagnostics, zero-mask no-draw behavior, and all-mask/all-boundary
      CPU mesh coverage (`ff7066b`; focused Go/assets/render tests and two
      independent reviews green). Deterministic acceptance is now complete in
      `489af26` and `748438c`: five canonical poses bind the exact 0..15 mask
      bijection and compiled-asset hashes, build isolated direction-exact stone
      supports, preserve mask 0 as zero-draw, fence the committed camera ahead
      of publication, and require two adjacent GPU-completed markers over the
      exact requested subchunks with stable generation, manifest, nonzero model
      reference total, and zero missing/stale/wrong-stream/zero-ref/draw-mismatch
      counters. Live evidence proved the total is subchunk-wide rather than one
      reference per central fixture (94 refs in run `20260714T023010Z-7488`), so
      the invalid interim 15/43 equality was removed in `eec96e2`; review then
      withdrew that recommendation and passed the corrected contract. The run
      produced adjacent exact witnesses at sequences 461/462, four keys, 94
      stable refs, and all contamination counters zero. Its only failure was the
      shared model-performance gate: 138.9454 ms mutation-to-visible against
      100 ms, p50 40.3 ms, p99 47.8 ms, with 8,595 resident subchunks. Combined
      RSS peaked at 532,107,264 bytes and mean CPU was 2.52%, within resource
      budgets. Native captures and the structural exact-count model-draw
      optimization remain required before this item closes; native capture is
      separately blocked by the confirmed RX 570 Bevy/wgpu presentation failure
      above. Exact visible-quad drawing is complete but was performance-neutral;
      GPU-stage measurement and the shared 100 ms gate remain open.
      **Glow-lichen/sculk-vein implementation complete (2026-07-13):** the
      remaining vine-like pink blocks were not `minecraft:vine`; they were all
      64 `minecraft:glow_lichen` and 64 `minecraft:sculk_vein` states still
      classified as unknown. Distinct registry families now preserve their
      different six-bit face orders, render mask zero with the vanilla all-six
      fallback, and compile exact 1/256-inset two-sided cutout planes with no
      occlusion coverage. Sculk vein additionally binds its pinned four-frame,
      20-tick flipbook. Exhaustive 128-state selector/geometry/UV/material
      tests, the real pinned-pack compiler, registrygen, runtime rendering,
      strict visual-coverage ratchet, Clippy, formatting, and independent
      review are green; the combined 16,913-state report has zero
      glow-lichen/sculk-vein diagnostics and 7,770 diagnostics remaining
      overall (`a70d3c6`). Native screenshot closure remains part of the shared
      RX 570 presentation gate above.
    - [x] Exhaustive vanilla visual-coverage ratchet: inventory every one of
      the 16,913 protocol-1001 canonical states through the production registry
      and runtime decoders, bind the exact registry/asset hashes, and reject any
      newly diagnostic or unjustifiably invisible state. Diagnostic shrinkage is
      allowed while residual families are implemented; the final gate requires
      zero diagnostic non-air states. The accepted design is recorded in
      `docs/superpowers/specs/2026-07-13-exhaustive-vanilla-coverage-design.md`.
      **Complete (2026-07-13):** `visualcoverage` uses the production decoders,
      enforces the exact 1,356-name/16,913-state/one-air protocol corpus and
      exact hash-to-sequential bijection, bounds all inputs, rejects diagnostic
      regression/invisible laundering, and writes deterministic hash-bound
      reports (`b131247`; 11 tests, strict Clippy, real-pack run, and independent
      review green). The reviewed baseline was refreshed cumulatively for the
      already-landed door, trapdoor, wall, pressure-plate, fence-gate, pane,
      fence, carpet, button, huge-mushroom, glow-lichen, sculk-vein, exact
      ordinary stained-glass, exact copper-grate, static-sign,
      chiseled-bookshelf, resin-clump, selector-alias opaque-cube, and exact
      cactus tranches.
      After lava, vine, and those connected/static/multiface/glass/grate
      families plus the exact chiseled-bookshelf, resin-clump, selector-alias
      opaque-cube, cactus, cake, farmland, and exact bee-housing tranches, the
      current residual has 2,400 diagnostics including
      the single air diagnostic, with zero
      diagnostics in every implemented family; each remaining family must shrink
      that exact set.
  - [ ] Complete the exhaustive residual-family report, continuing from the
    completed lava/flowing-lava depth-writing non-water-liquid pipeline, so
    every non-air one of the 16,913 canonical states has a non-diagnostic visual;
    close deterministic galleries and live acceptance with globally zero
    diagnostic counters, vanilla-reference screenshots, upload/memory/CPU
    metrics, and teleport-remesh evidence.
    - [x] Lava implementation: all 32 `minecraft:lava` and
      `minecraft:flowing_lava` depth states compile through the animated liquid
      mesher without water tint or alpha blending, use an immutable packed route
      bit, retain the O(n) transparent-water/depth-lava partition, and draw in a
      separate opaque depth-writing direct/MDI pipeline. Mixed interfaces and all
      six cross-subchunk boundaries are covered. Full assets/render suites,
      strict Clippy/formatting, the real 16,913-state ratchet, and independent
      review are green; deterministic native gallery/GPU/performance evidence
      remains part of the residual-family live gate.
    - [x] Exact bee nest/beehive cubes: all 48 canonical states across
      `minecraft:bee_nest` and `minecraft:beehive` now preserve the typed
      direction 0..3 by honey-level 0..5 product, full-cube collision and
      occlusion, and the compact packed cube stream. Protocol direction maps
      the front south/west/north/east; only honey level 5 selects the honeyed
      front. The compiler requires the pinned six-face block maps, singleton
      static terrain arrays, exact two-entry front arrays, literal paths, and
      no tint/extra/flipbook metadata. Both network-ID modes, all states, dense
      greedy closure, and all six cross-subchunk boundaries are covered. The
      full real-pack ratchet removes exactly IDs 10,395..10,418 and
      12,495..12,518 with zero additions, shrinking 2,448 -> 2,400 diagnostics.
      Bee occupants remain a separately reviewed block-entity concern rather
      than block-state geometry.
    - [ ] Shelf visual authority: the exact twelve-name/384-state registry
      contract is classified and now fails closed if one complete family is
      missing or an unexpected `_shelf` family appears. The installed Bedrock
      1.26.3301.0 package exposes direction-specific
      `minecraft:voxel_shape` files and the installed/pinned vanilla packs
      expose shelf texture routes, texture sets, and pixels, but none defines
      visible render geometry or per-face UV mapping. Collision/voxel bounds
      must not be promoted into a render model. All 384 shelf states therefore
      remain diagnostic and the global residual remains 2,400. Resume only
      from legitimate version-matched render/UV authority or a reviewed native
      procedure; precise paths and hashes are recorded in
      `docs/evidence/phase-2-shelf-source-reference.md`.
    - [ ] Close static sign visual parity and its deterministic native gallery.
      - [x] Eliminate all 4,872 standing, wall, and hanging-sign diagnostics
        with typed, order-independent selectors; exact 16-way rotation, six-way
        facing, attachment, and hanging matrices; pinned terrain aliases; and
        bounded static model templates. All-six-facing tests caught and fixed
        reversed wall/wall-hanging support placement. The classic raw 24x12
        board plus 2/3 render transform establishes the 16x8 world silhouette,
        and sign text remains a block-entity deferral (`5987ed6`, `fba9e2e`).
      - [ ] Source-adjudicate exact board thickness, standing-post dimensions,
        and hanging board/support cuboids, then close UV/native-reference and
        GPU evidence. The pinned Bedrock sample pack exposes sign terrain aliases
        but no authoritative sign geometry, so the current shapes must not be
        described as final 1:1 geometry until this evidence is recorded.
    - [ ] Run all 67 exact-state GPU gallery pages (256 targets per logical page,
      with one final 17-state page), require exact palette readback plus two
      consecutive GPU-completed frames for every canonical target, and inspect
      fresh native `%TEMP%` screenshots. Family-specific support/neighbour
      fixtures do not count toward the 16,913 target inventory. The reviewed
      implementation order starts with a deterministic BREG/MCBEAS/hash-bound
      logical page inventory, then adds exact app-side palette witnesses,
      per-target GPU evidence, family-aware placement, and native captures; the
      logical inventory is independent of the RX 570 presentation blocker.
      - [x] Compile the exact logical target inventory: all 16,913 sequential
        IDs are assigned once and in order to 66 full 256-target pages plus a
        final 17-target page. The deterministic artifact binds BREG, MCBEAS, and
        coverage-baseline hashes, retains per-target diagnostic/drawable/
        invisible status, and is deliberately non-accepting until both the
        diagnostic count reaches zero and the strict semantic render-route gate
        passes. The CLI writes atomically and preserves an existing output on
        failure (`500c4af`, `59f692c`; full tests, pinned real-pack check,
        strict Clippy/formatting, and independent re-review green).
    - [ ] Generate a separate version-pinned block-entity inventory and reviewed
      renderer manifest. Prove chunk-NBT and live-update handling, required NBT
      variants, and GPU/no-draw evidence for every source ID; block entities are
      not folded into the canonical block-state count. The ingestion audit found
      21 explicit Dragonfly source IDs plus an id-less Note producer that must be
      explicitly adjudicated; current packet-56 and chunk/subchunk-tail NBT is
      dropped before the world store. The bounded implementation order is
      NetworkLittleEndian NBT prefix decoding plus atomic sparse storage first,
      then a separate deterministic source/renderer-manifest generator and
      strict join, followed by per-ID GPU/no-draw witnesses.
      - [x] Implement bounded NetworkLittleEndian NBT prefix decoding and
        atomic sparse ingestion for inline and request-mode LevelChunk tails,
        successful SubChunk tails, and packet-56 live updates. Exact NBT bytes,
        optional source IDs, and absolute positions remain sparse and
        palette-native; strict byte/depth/collection/entity limits, scope and
        duplicate checks, vanilla dimension Y validation, per-chunk cumulative
        record/raw-byte caps across tail and live updates, FIFO worker decoding,
        malformed-update retention, all-air cleanup, and chunk eviction are
        covered without flat block expansion. The separate deterministic
        inventory/renderer manifest and per-ID GPU/no-draw witnesses remain
        open.
    - [ ] Squash-merge both the Axolotl protocol-fix branch and Cinnabar feature
      branch into their respective `main` branches only after the applicable
      deterministic tests, native/GPU acceptance, zero-diagnostic state gate,
      and block-entity manifest gate are green.
- [ ] **2.7 Client lighting and atmosphere.** Block/sky flood fill, baked vertex
  light and day/night, then sky, fog, and clouds; finish the Phase 2 parity and
  teleport-remesh acceptance gates.
  - [x] Normalize SetTime and rain/thunder level events into bounded,
    vendor-independent protocol events; retain StartGame's initial current
    tick, day-cycle lock time, case-insensitive boolean `doDaylightCycle`
    state (default enabled when absent), and clamped initial rain/lightning.
    Runtime `GameRulesChanged` packets normalize only a case-insensitive
    boolean `doDaylightCycle` entry and ignore wrong types. Two deferred
    pre-spawn SetTime
    packets retain FIFO order in Play, post-spawn normalization is identical,
    and non-finite initial weather values fail closed.
  - [x] Consume the normalized environment stream into app-owned clock and
    weather resources without interpreting visual curves. A replacement
    StartGame begins a new environment session, anchors its exact initial tick,
    preserves its cycle lock and bounded rain/lightning targets, advances only
    while `doDaylightCycle` is enabled, and uses the lock tick only when the
    rule is explicitly disabled. A runtime false transition freezes the exact
    current visual tick; true re-anchors and resumes it. Exact signed SetTime
    values immediately re-anchor either a running or frozen clock. Dimension
    changes
    preserve that world-session snapshot. FIFO-committed SetTime/day-cycle/weather
    updates do not dirty meshes, enqueue mesh changes, or change cave
    connectivity. Mesh-baked light response and vanilla atmosphere parity
    remain open.
  - [x] Implement the renderer-independent sparse light core: independent
    uniform-or-packed block/sky nibble volumes, copy-on-write snapshots,
    generation-checked storage/eviction, and a bounded darken-then-increase
    solver over explicit Unknown/KnownAir/Resident cells. Exact one-cell halo
    samples carry scheduler-owned trust and direct-sky provenance; unknown,
    dirty, or untrusted boundaries never seed light, Nether/End reject sky,
    and all propagation shares one enforced queue budget. WorldStream now owns
    the authoritative state metadata and bounded scheduling below; mesh baking,
    GPU/shader light consumption, and vanilla atmosphere parity remain open.
  - [x] Generate and ship bounded per-runtime-state light metadata without
    changing the reviewed `BREG1003`: `LREG1001` proves the exact protocol-state
    count, identity, property order, and committed-BREG SHA-256 before emitting
    one packed emission/filter byte per state. Dragonfly revision
    `dbbd8b787946e53b1def8d532050751dfcdc80e7` is authoritative for 16,911
    concrete states; exact, identifier-uniform pinned PMMP fallback supplies only
    `minecraft:redstone_lamp` and `minecraft:lit_redstone_lamp`, with deterministic
    provenance IDs/reporting and fail-closed disagreement/range checks. `MCBEAS05`
    atomically carries the byte beside each visual, exposes it through both
    sequential and network-hash resolution, rejects stale `MCBEAS04`, and names
    the `--light-registry` rebuild command. The solver and WorldStream scheduling
    integration are covered below; mesh baking, GPU/shader light consumption,
    vanilla sky/fog/cloud parity, and visual acceptance remain open.
  - [x] Derive a deterministic per-frame atmosphere snapshot from the app-owned
    clock and weather state: real elapsed time advances enabled sessions at 20
    ticks per second, explicitly disabled cycles freeze at their lock tick,
    signed times use
    Euclidean day and moon-phase wrapping, and rain/thunder remain bounded.
    Extract one stable 96-byte uniform, render the first procedural sky/sun/moon
    pass at reversed-Z far depth with per-view MSAA specialization, and apply
    camera-distance fog to chunk, model, and liquid paths without adding storage
    bindings or per-frame texture/bind-group churn. These checked curves and
    procedural disks establish the integration slice, not vanilla parity;
    asset-backed GPU hookup, precipitation visuals, underwater/lava medium fog,
    and live reference acceptance remain open.
  - [x] Pin and carry the exact vanilla sun, moon-phase atlas, and cloud texture
    in an independent bounded `MCBEATM1` runtime blob. The compiler requires the
    exact tracked Mojang manifest fields, canonical LF bytes (accepting only a
    uniform LF or Windows CRLF checkout), and per-source PNG hashes, records
    encoded and decoded hashes, rejects malformed/noncanonical layouts, and
    publishes only to ignored local paths. `make assets` and `make client`
    refresh the serialized blob/report pair through one portable producer;
    normalized, case-variant, symlink/junction, and hardlink output aliases fail
    before either write. Focused pinned tests, full assets/client-asset suites,
    strict Clippy, and independent re-review are green through `aed8d7f`; no
    Mojang payload is tracked.
  - [x] Load the required sibling `MCBEATM1` carrier once at startup and render
    its exact sun, 4x2 moon-phase atlas, and repeating cloud texture through the
    existing reversed-Z atmosphere phase. Three persistent GPU textures, one
    sampler, and one identity-cached bind group replace the procedural celestial
    disks without per-frame or per-subchunk resource churn; missing or malformed
    carriers fail hard with the exact rebuild command. Clouds use absolute world
    time, altitude 128, weather/fog fades, and a tested +X eastward speed of 0.03
    blocks per tick. Full app/render suites, strict Clippy, WGSL validation, and
    independent review are green through `4bf2c8c`; native multi-platform visual
    tuning and parity acceptance remain open.
  - [x] Resolve the camera-eye medium directly from palette-native liquid
    contributors, including secondary waterlogged layers, and use the exact
    two-triangle surface drawn by the shared quad index buffer for the air/water
    or air/lava transition, including noncoplanar corner heights. The sparse
    camera query reads packed indices and palette entries directly without
    constructing the mesher's allocating palette-fact cache; missing and
    non-finite samples fail open to air. Water and lava replace weather distance
    fog in the existing 96-byte atmosphere uniform, so chunk, model, liquid, and
    infinite-sky/celestial rendering share one medium response without a new
    binding or per-subchunk resource. The bounded water/lava colours and 32/3-
    block visibility ranges are the Phase 2.7 baseline; native reference
    calibration and precipitation visuals remain open.
  - [x] Wire the sparse solver and MCBEAS05 per-state emission/filter metadata
    into generation-qualified WorldStream light storage and bounded,
    nearest-first one-subchunk solves. Exact face block/light halos, dirty
    boundary trust, separately retained direct-sky provenance, decoded/update/
    eviction invalidation, convergent neighbour iteration, and current-light
    mesh scheduling are covered without flat block arrays. Mesh light baking,
    GPU/shader consumption, sky/fog/cloud rendering, and visual acceptance
    remain open.
    - Release scheduler workload gate on 2026-07-14: the exact radius-16 square
      (33×33 columns) across all 24 Overworld subchunks completed 26,136
      known-air light solves, with 26,136 uniform fast-path completions, zero
      stale completions, and all keys current in 1,006 ms. This measures the
      lighting scheduler only; live teleport/full-view remesh acceptance,
      mixed-block workloads and rendering acceptance remain open.
  - [x] Capture palette-native mesh lighting in a fixed 27-slot identity halo,
    gate mesh dispatch until every known slot is current, reject and losslessly
    requeue completions after exact light or direct-sky identity changes, and
    invalidate the center plus all 26 dependants on light completion, load, or
    eviction. Center/face/edge/corner routing and absent dark fallback read
    nibble channels directly without flat staging arrays. App tests passed
    233/233 (with one release-only test ignored), all app integration and world
    suites passed, and the exact release workload completed 26,136/26,136
    current subchunks with zero stale completions in 987 ms. The halo now
    implements the allocation-free render sampler and the worker calls the
    light-aware mesher. Cube, model, cross, and liquid CPU sidecars retain
    independent block/sky/AO channels; cube greedy merges split on exact packed
    lighting, and the cube sidecar survives bounded render-queue extraction
    with exact byte accounting. The combined app/render suites and strict
    Clippy are green. GPU arena/shader consumption, mixed-block visual
    acceptance, and live teleport full-view remesh acceptance remain open.
  - [x] Consume cube, model, and liquid light sidecars in the GPU world shaders
    without adding a buffer, bind group, or per-subchunk render resource. Commit
    `fe1a2ea` appends cube sidecars to the existing binding-13 arena, expands the
    per-draw origin ABI to carry exact cube/light bases, validates aligned and
    disjoint direct/MDI addressing, and converts discrete block/sky/AO samples
    at the vertex before smooth interpolation. Daylight affects only sky light;
    full solved skylight retains a named provisional `0.2` transfer floor at
    true night while block light remains independent. This floor is a
    conservative calibration to the existing horizon baseline, not a vanilla
    parity claim; native Bedrock reference tuning remains open. Alpha and fog
    ordering remain intact. Full render and app suites, strict
    combined Clippy, WGSL semantic/Metal-stage checks, formatting, and diff
    checks are green. Live mixed-block GPU parity and teleport/performance
    acceptance remain open.
  - [x] Preserve the authoritative all-air suffix omitted by limited-request
    `LevelChunk` columns and feed it into the sparse top-down skylight graph.
    `highest=0` columns require no outbound slot or packet; replacement,
    eviction, direct-sky propagation, and the final mesh-light sidecar are
    covered. This fixes the live zero-skylight world while leaving block light
    independent. The pinned opaque-RGB sun and moon textures now derive
    coverage from their exact black key instead of their unusable all-opaque
    alpha channel, removing the expanding black celestial quads. Full client,
    render-atmosphere, asset, camera, strict Clippy, WGSL, and independent
    review gates are green through `7805402`.

Perf budget carried from Phase 0 gate; add: full remesh of view distance after teleport ≤ 2s.

**Edge anti-aliasing (2026-07-14):** the client explicitly selects portable 4x
MSAA for the primary camera. All custom chunk, static-model, transparent-model,
water, and depth-liquid pipelines specialize from the camera sample count, so it
covers the complete world render without an FXAA blur pass over the pixel-art
textures. An attempted 8x default (`ac1da9e`) was reverted after a real macOS
adapter rejected eight samples for `Depth32Float`; any higher setting must be
capability-checked rather than hardcoded (camera suite and strict Clippy green).

**Live visual acceptance (Computer Use):** run the Bevy app in representative vanilla
world scenes and compare visible results against the matching Mojang vanilla assets/reference
client at multiple distances and view angles. Verify exact texture/model selection, UV orientation
and wrapping, per-layer mip quality, opaque/cutout/blend behavior, flipbooks, biome tints,
block/sky lighting across day/night, fog, sky, and clouds. Exercise focus, keyboard input,
movement, and mouse-look/rotation during the pass. No placeholder/debug texture or visibly
non-vanilla rendering ships past this phase; record screenshots and any adjudicated parity gaps
in the phase report. If Computer Use window capture fails, take a native Windows screenshot,
store it only under the user's temporary directory, inspect that file, and never commit it.

## Phase 3 — Movement and the local player

**Goal:** playable movement that servers accept. Deliverable: walk/sprint/jump/sneak/swim/
climb on a vanilla parkour course and on Lunar-fronted servers with server-auth movement,
no rubber-banding.

Scope: input → `PlayerAuthInput` at 20Hz with correct flags; client prediction in `crates/sim`
as a **behavioral port of bedsim** — test strategy: golden traces (bedsim runs input scripts →
JSONL of per-tick positions; Rust sim must match within epsilon; reuse the pathfind-bot log
tooling patterns); collision against `crates/world`; camera = per-frame interpolation of
tick states; correction/rewind handling (`CorrectPlayerMovePrediction`).

## Phase 4 — Entities and other players

Scope: actor lifecycle packets, metadata/attributes, movement interpolation, biped rendering
with standard + persona skins (skin data arrives via PlayerList/AddPlayer), name tags, vanilla
mob geometry + textures from bedrock-samples, **molang subset** for vanilla animation
controllers (walk cycles, look-at; documented cut-line, static pose fallback), item entities
and dropped-item rendering, paper-doll first-person arm/held item.

## Phase 5 — Interaction, inventory, UI

Scope: block breaking (server-auth crack progress overlay), placement, item use via
`InventoryTransaction`/`ItemStackRequest`; hotbar + survival/creative inventory + containers
(chest/furnace/crafting UIs); forms (modal/menu/custom JSON forms — Lunar's ClickUI depends on
these); chat with Bedrock formatting codes; HUD (health/hunger/armor/air, bossbar, scoreboard,
title/actionbar); Bedrock bitmap font rendering from pack `font/` assets. Taste bar applies:
UI phases get fable-5/opus-4.8 review before merge.

## Phase 6 — Online product surface

Scope: main menu + settings (video/controls/audio/account); server browser (saved servers);
Realms and friends lists in-UI (control-channel data from Phase 1) with one-click join;
transfer/reconnect UX; **server resource packs applied at runtime** (cache from core →
`crates/assets` hot-swaps textures/models/sounds/lang over the vanilla base — the asset
system from Phase 2 must have been built pack-stack-aware); disconnect screens with real
reasons; auth/device-code UX polish. Optional stretch: Lunar module toggles surfaced in-client
via control channel (v1.x, not v1).

## Phase 7 — Local worlds on dragonfly

Scope: core embeds/spawns dragonfly (`platform/pc-server` and dragonfly-server skill patterns
as reference); world create/select/delete UI; settings (name, gamemode, seed, flat/normal);
LevelDB world persistence via dragonfly; pause/resume semantics on window focus; same client
path as online (core points the game socket at the local dragonfly). Documented v1 limits:
dragonfly's generation and mob AI parity gaps are accepted, not chased.

## Phase 8 — Audio, polish, packaging

Scope: audio via bevy_audio/kira — sound events mapped through `sound_definitions.json`,
positional sounds, music/ambient (asset-availability audit from Phase 2 decides
bedrock-samples vs. client-assets-import); performance hardening pass against budgets;
macOS .app + codesign/notarize, Windows installer, Linux AppImage; core binary bundled and
lifecycle-managed by the app; crash reporting (sentry for Rust + core); auto-update channel;
first-run experience.

**Final Go relay/batching polish:** adopt the batch-boundary API from
[`HashimTheArab/gophertunnel` PR #80](https://github.com/HashimTheArab/gophertunnel/pull/80)
after it lands on the pinned `lunar` line. The integration commit must retain the pinned
`Conn.Abort` work as well as the PR's batch API. Enable `Dialer.EnableBatchReading` and
`ListenConfig.EnableBatchReading` on the two core legs, replace the relay's single-packet
`ReadPacket` pumps with `ReadBatch`, and forward each returned slice as exactly one downstream
batch using `WritePacketImmediate(batch...)` (or a tested `WritePacket` + single `Flush`
equivalent that preserves buffered ordering). Never mix `ReadBatch` with
`ReadPacket`/`ReadBytes`/`Read` on a batch-reading connection. Port the PR's
ordering, slow-reader, mid-batch decode-error, deferred-login-boundary, and pre-disconnect flush
regressions into `core/internal/relay`; retain bounded lossless backpressure and verify that the
change improves batching without regressing join latency, memory, or shutdown behavior.
The PR #80 API is now carried on the published `cinnabar-batch-reading` fork branch at
`bbe6cfdeed39713c2b20103a1294e609d5841615`; Cinnabar enables batch reading on both legs,
preserves source batch boundaries, and retains the exact 1,600-packet split ceiling. Core
now forwards each bounded slice with `WritePacketImmediate`, pre-flushes existing buffered
output, tests boundaries in both directions, and prevents the initial loading-screen filter
from merging adjacent source wire batches. Unit tests and the first real BDS join/GPU-witness
run are green for that slice. Porting the
remaining PR-specific slow-reader/decode-error/disconnect regressions and completing the
join-latency/resource comparison remain open, so this final polish item is not yet complete.

---

## Sequencing and program rules

- Order is 0 → 1 → 2 → 3 → (4 ∥ 5) → 6 → 7 → 8. Phases 4 and 5 can run in parallel worktrees once 3 lands (disjoint crates; both consume `crates/world` + `crates/protocol` which are stable by then).
- Each phase starts by converting its scope block into a full task-by-task plan (superpowers:writing-plans), gets brainstorm-level review if its scope shifted, and ends with the requesting-code-review flow. PR-bot adjudication rules apply throughout.
- Every phase must leave `main` in a runnable state (`app` launches, joins BDS, does everything prior phases delivered) — CI runs the Phase 0 acceptance connect as a smoke test forever.
- Protocol bumps during the program: deliberate, one task, lockstep — regenerate valentine defs, run conformance, bump core, bump `registrygen` exports, fix findings. Never mid-phase.

## v1 Definition of Done

From a clean machine: install → sign in with Xbox (device code) → join a third-party RakNet
server, a Realm, and a friend's world (NetherNet) → play survival basics (move, build, mine,
chest, craft, chat, forms) with vanilla look and feel at 60fps on the dev MacBook → create a
local dragonfly world, play it offline, reload it. Server resource packs render. No Rust-side
auth/transport code exists.

---

## Appendix: Rendering Performance Playbook (binding for Phases 0 and 2)

FPS and memory in this client are dominated by chunk meshes; these techniques stack
multiplicatively and are the required approach, not suggestions:

1. **Paletted chunk data stays paletted at runtime.** Mesh directly from palette + packed
   indices; never expand to flat per-block arrays (the naeast2 lesson, client-side). Uniform
   subchunks (all air/all one block) store one palette entry and skip meshing entirely.
2. **Binary greedy meshing.** Per-axis-column `u64` bitmasks; face culling and coplanar
   merging via bitwise ops (target: tens of µs per subchunk, making remesh-on-update ~free).
   Merges split where baked AO/light values differ. References: `block-mesh` crate and
   TanTanDev binary-greedy-meshing demos.
3. **Packed vertices / per-quad vertex pulling.** Local position 5+5+5 bits, face ID 3 bits
   (normal from LUT), texture-array layer index, AO 2 bits, light 8 bits → 1–2 `u32` per
   vertex, subchunk origin as a per-draw push constant. Preferred form: one ~8-byte record
   per quad in a storage buffer, corners reconstructed in the vertex shader, and one shared
   static index buffer for all chunks. This targets roughly 20–40× less mesh memory than
   naive 32-byte vertices.
4. **Custom Bevy render phase for chunks.** No per-subchunk `Mesh`/`StandardMaterial`; use
   one chunk pipeline family with at most two immutable state variants
   (opaque/cutout with depth writes, blend without depth writes) and one shared bind group,
   with `multi_draw_indirect` where available.
5. **Visibility culling.** Per-subchunk frustum culling + cave/connectivity culling
   (Checchi-style: face-to-face connectivity flood-filled at mesh time, then BFS from the
   camera through the chunk graph—the approach used by vanilla).
6. **Budget spiky work.** Decode/mesh/light only on Rayon workers; GPU uploads capped per
   frame and nearest-first; light updates deduplicated and queued; block + sky light baked
   per vertex at mesh time so lighting cost rides the remesh budget.
7. **2D texture arrays, not a stitched atlas.** This avoids mip bleeding, permits greedy-quad
   UV wrapping, and implements flipbooks as layer swaps; mipmaps are generated per layer.
   Use one measured physical array when the reachable deduplicated layer inventory fits the
   minimum target adapter, otherwise at most two equal-format array pages in the same shared
   bind group. More pages, frame dropping, or silent animation degradation are forbidden.

Explicitly deferred past v1: distant-chunk LODs (not needed at a 16-chunk radius), GPU
occlusion queries (cave culling suffices), and mesh shaders.

Resource budget (tracked from Phase 2 onward; reference machine class = Ryzen 5 3600 / mid
Apple Silicon, 16-chunk radius, capped 60fps): combined RSS (client + core) ≤ 650MB
steady-state; steady-state CPU ≤ 15% total; join/teleport bursts may saturate cores but must
settle within ~2 seconds. Baseline for comparison: vanilla Bedrock client on the same
machine runs at 800MB–2GB and 30%+ CPU.

Binding Phase 2 scope: block registry + block-state → model/texture mapping (generated
export from Dragonfly's registry via `tools/registrygen`, shipped as a binary asset, with
pinned PMMP BedrockData as the exact protocol-1001 canonical palette/property/biome
cross-check, Axolotl Valentine's typed state catalog as a versioned selector reference, and
Axolotl's exact pinned PrismarineJS Bedrock collision shapes as reviewed cuboid-template and
occlusion inputs rather than render/UV authority); vanilla
asset ingestion from **Mojang/bedrock-samples** pinned to the matching game version
(terrain textures, `blocks.json`, flipbooks, and biome colors). The pinned samples have no
block-render model JSON, so deterministic reviewed family generators combine collision
bounds, Dragonfly behavior rules, Mojang texture mappings, and vanilla-reference evidence.
Zuri is not a rendering or asset-system input. BDS
`resource_packs/vanilla` is server-minimal (`blocks.json` + texts only): it is a data
reference, not the texture source. Use the bounded one-or-two-page 2D texture-array scheme
above with per-layer mipmaps; meshing per
this playbook with opaque/cutout/blend layers; a client-side block + sky flood-fill light
engine with per-vertex light baked at mesh time and day/night; biome tinting for
grass/foliage/water; sky, fog, and clouds; chunk streaming/eviction tied to
`ChunkRadiusUpdated` + `SubChunk` request flow. Custom block-entity renderers remain
deferred; chests/signs receive static models in this phase. The Phase 0 performance budget
carries forward, with full remesh of view distance after teleport ≤ 2 seconds.
