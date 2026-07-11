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
| Dragonfly registry sequential IDs differ from valentine protocol-1001 palette IDs | 1→2 | Keep hashed mode explicit for current sessions; Phase 1 conformance must reject or translate sequential mode before claiming arbitrary-server support |
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

Exit: `corectl join --friend <gamertag>` works from a clean machine; conformance CI green.

## Phase 2 — World rendering (textured, lit, real)

**Goal:** the spike renderer becomes the real world pipeline. Deliverable: fly through any
live server world and it *looks like Minecraft*.

Scope: block registry + block-state → model/texture mapping (generated export from dragonfly's registry via `tools/registrygen`, shipped as a binary asset); vanilla asset ingestion from **Mojang/bedrock-samples** pinned to the matching game version (block models, terrain textures, `blocks.json`, flipbooks) — NOTE: BDS `resource_packs/vanilla` is server-minimal (blocks.json + texts only), it is a data reference, not the texture source; 2D texture array + per-layer mipmaps; greedy/culled meshing with transparency layers (opaque/cutout/blend) and per-face culling; **client-side light engine** (block + sky flood-fill, per-vertex light, day/night); biome tinting (grass/foliage/water); sky, fog, clouds; chunk streaming/eviction tied to `ChunkRadiusUpdated` + `SubChunk` request flow; block entities with custom renderers deferred (chests/signs get static models in this phase).

**Phase 2 progress (kept current as work lands):**

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
  A 2026-07-11 interactive radius-16 run reached world-ready with zero missing
  mappings, but is diagnostic rather than acceptance evidence: 508,385 rendered
  quads used material zero, and exact blob inspection found only 616 of 16,913
  registry visuals currently mapped to real materials. Most of that visible gap
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
  - [ ] Compile grass/foliage/water tint classifications and biome color rules,
    upload palette-native biome/tint tables, and apply them in the chunk shader
    without widening the eight-byte quad record. Grass plus generic/birch/
    evergreen/dry foliage are now resolved from `MCBEAS03`, revision-gated,
    and applied palette-natively; real water-material production remains in 2.6.
- [ ] **2.6 Static/non-cube models, blend/water, and flipbooks.** Complete the
  remaining block visual classes and animation path.
- [ ] **2.7 Client lighting and atmosphere.** Block/sky flood fill, baked vertex
  light and day/night, then sky, fog, and clouds; finish the Phase 2 parity and
  teleport-remesh acceptance gates.

Perf budget carried from Phase 0 gate; add: full remesh of view distance after teleport ≤ 2s.

**Live visual acceptance (Computer Use):** run the Bevy app in representative vanilla
world scenes and compare visible results against the matching Mojang vanilla assets/reference
client at multiple distances and view angles. Verify exact texture/model selection, UV orientation
and wrapping, per-layer mip quality, opaque/cutout/blend behavior, flipbooks, biome tints,
block/sky lighting across day/night, fog, sky, and clouds. Exercise focus, keyboard input,
movement, and mouse-look/rotation during the pass. No placeholder/debug texture or visibly
non-vanilla rendering ships past this phase; record screenshots and any adjudicated parity gaps
in the phase report.

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
   one pipeline + one bind group (texture array), with `multi_draw_indirect` where available.
5. **Visibility culling.** Per-subchunk frustum culling + cave/connectivity culling
   (Checchi-style: face-to-face connectivity flood-filled at mesh time, then BFS from the
   camera through the chunk graph—the approach used by vanilla).
6. **Budget spiky work.** Decode/mesh/light only on Rayon workers; GPU uploads capped per
   frame and nearest-first; light updates deduplicated and queued; block + sky light baked
   per vertex at mesh time so lighting cost rides the remesh budget.
7. **2D texture array, not a stitched atlas.** This avoids mip bleeding, permits greedy-quad
   UV wrapping, and implements flipbooks as layer swaps; mipmaps are generated per layer.

Explicitly deferred past v1: distant-chunk LODs (not needed at a 16-chunk radius), GPU
occlusion queries (cave culling suffices), and mesh shaders.

Resource budget (tracked from Phase 2 onward; reference machine class = Ryzen 5 3600 / mid
Apple Silicon, 16-chunk radius, capped 60fps): combined RSS (client + core) ≤ 650MB
steady-state; steady-state CPU ≤ 15% total; join/teleport bursts may saturate cores but must
settle within ~2 seconds. Baseline for comparison: vanilla Bedrock client on the same
machine runs at 800MB–2GB and 30%+ CPU.

Binding Phase 2 scope: block registry + block-state → model/texture mapping (generated
export from dragonfly's registry via `tools/registrygen`, shipped as a binary asset);
vanilla asset ingestion from **Mojang/bedrock-samples** pinned to the matching game version
(block models, terrain textures, `blocks.json`, flipbooks). BDS
`resource_packs/vanilla` is server-minimal (`blocks.json` + texts only): it is a data
reference, not the texture source. Use a 2D texture array + per-layer mipmaps; meshing per
this playbook with opaque/cutout/blend layers; a client-side block + sky flood-fill light
engine with per-vertex light baked at mesh time and day/night; biome tinting for
grass/foliage/water; sky, fog, and clouds; chunk streaming/eviction tied to
`ChunkRadiusUpdated` + `SubChunk` request flow. Custom block-entity renderers remain
deferred; chests/signs receive static models in this phase. The Phase 0 performance budget
carries forward, with full remesh of view distance after teleport ≤ 2 seconds.
