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
- Core: Go, `lunar` (gophertunnel, go-raknet, go-nethernet, go-xsapi/v2), dragonfly
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
  - `core/` — **Go module**: the core service (imports `lunar`; donor code: `platform/pc-client`)
  - `tools/` — Go: registry/asset exporters, conformance fixture generator
- **`platform/Lunar`** — small additions only: anything the core needs exposed through the facade; conformance fixture corpus generator may live in `cmd/`.
- **Decision log** (settled in design discussion, 2026-07-09/10): hybrid over pure-Rust (single protocol treadmill, reuse of auth/NetherNet/Realms/physics estate) and over pure-Go (renderer ecosystem); docs-generated codec over gophertunnel-AST codegen (docs are machine-emitted wire descriptions; gophertunnel Marshal funcs are not generatable); socket file over TCP loopback (permissions, no ports); valentine defs adopted as-is for v1 (Hashim PR'd 1.26.30 to axolotl) — vendor/fork decision deferred until Phase 1; conformance harness deferred to Phase 1 (Phase 0 spike acts as the manual conformance test).

## Risk Register (tracked, each owned by a phase)

| Risk | Phase | Mitigation |
|---|---|---|
| Bevy meshing/frame-pacing insufficient | 0 | Spike acceptance gates the whole program |
| valentine defs drift from gophertunnel bytes | 0→1 | Spike surfaces; Phase 1 builds automated conformance harness |
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
- `crates/protocol`: `fn decode_batch(bytes) -> Vec<Packet>`, `fn encode(packet) -> Bytes`, `enum Packet` (valentine-generated), plus `LoginSequence` state machine: `RequestNetworkSettings → Login (self-signed chain, no XBL) → handshake → ResourcePackClientResponse (decline/none for spike) → RequestChunkRadius(16) → await StartGame → SetLocalPlayerAsInitialized`
- `crates/world` (spike-minimal): `SubChunk::decode(&[u8]) -> SubChunk` (paletted storages), `Chunk { sub_chunks: Vec<SubChunk> }`

**Tasks (each = write failing test → run → implement → pass → commit):**

- [ ] **0.1 Repo scaffold.** Cargo workspace + `core/` Go module; CI stub (`cargo test`, `go test ./...`). Commit.
- [ ] **0.2 Spike core proxy.** Go: socket-transport `net.Listener` (port from `bedrock-mc/plugin`) + `minecraft.ListenConfig{AuthenticationDisabled: true}` + upstream `minecraft.Dialer` forwarding loop. Test: Go integration test dials the socket with gophertunnel's own client, joins local BDS through it. Run: `go test ./core/... -run TestProxyJoin -count=1`. This test is load-bearing: it proves the core path with a known-good client before Rust enters.
- [ ] **0.3 Bridge crate.** Rust: connect + framing. Test: echo fixture against a Go test binary serving the same transport. `cargo test -p bridge`.
- [ ] **0.4 Protocol crate: vendored defs + decode smoke.** Vendor valentine 1.26.30 generated output. Test: decode a fixture corpus of gophertunnel-encoded packets (generate fixtures with a small Go tool in `tools/fixturegen` — encode one of each: NetworkSettings, StartGame, LevelChunk, MovePlayer, AddActor). Any decode failure here = defs/gophertunnel drift: adjudicate against bedrock-protocol-docs, fix gophertunnel upstream or patch defs, record in `crates/protocol/DEVIATIONS.md`.
- [ ] **0.5 Login sequence.** Implement `LoginSequence`; test = drive it against the spike core to StartGame. `cargo test -p protocol --test login -- --nocapture` with core+BDS running.
- [ ] **0.6 Sub-chunk decode.** Port paletted-storage decode (reference: dragonfly `chunk` package). Test: golden fixtures exported from dragonfly (`tools/chunkfix` encodes known block patterns; Rust asserts exact block runtime IDs at coordinates).
- [ ] **0.7 Spike renderer.** Bevy app: consume LevelChunk → decode → cull-meshing on rayon → vertex buffers → draw untextured (per-runtime-ID debug colors); fly camera. No test — acceptance run below.
- [ ] **0.8 Acceptance run.** Connect to BDS world, render 16-chunk radius, fly at speed, break/place blocks from a second client to force live remeshing. **Gate: p99 frame time ≤ 8ms on the dev MacBook at 16 chunks; remesh of a modified sub-chunk visible ≤ 100ms; zero decode errors over a 15-minute session (or all errors adjudicated as 0.4-style findings and fixed).** Record numbers in the phase report.

**Exit criteria:** acceptance gate met; deviations documented; go/no-go written up. Everything after this phase is "build the game", with the architecture de-risked.

---

## Phase 1 — Core service (Go): productize the boundary

**Goal:** `bedrock-core` becomes a real service the client can ship: session lifecycle, auth,
control channel, conformance harness. Deliverable proof: a headless Go CLI (`corectl`) can
device-code-auth, list Realms/friends, and join any of the three transport targets through
the core — before any more Rust exists.

Scope (detailed plan to be written at phase start):
- Control channel on `control.sock`: protobuf or JSON-RPC; methods — `Status`, `StartAuth` (device-code events streamed), `SignOut`, `ListServers`, `ListRealms`, `ListFriends` (gophertunnel realms package + go-xsapi sessions; the join side of what go-mcxboxbroadcast does), `Connect{target}`, `Disconnect`; events — auth state, connection state, transfer notices, disconnect reasons.
- Session lifecycle: core dials upstream (RakNet / NetherNet via Xbox signaling / Realms address), serves the game socket, handles transfers by reconnecting upstream while holding the client session (Lunar already has this pattern).
- Resource-pack negotiation upstream; pack payloads handed to client over the control channel as files in a cache dir (client applies them — Phase 6 renders them).
- Windows transport flavor (named pipe or TCP) behind the same listener interface.
- **Conformance harness (promoted from deferral):** `tools/fixturegen` grows to full packet coverage; CI job round-trips gophertunnel↔valentine bytes both directions on every core and defs bump. This is the automated version of spike task 0.4.
- Consumer-surface work in `platform/Lunar` to expose what the core needs through the facade (measured, minimal, per AGENTS.md ABI rules).

Exit: `corectl join --friend <gamertag>` works from a clean machine; conformance CI green.

## Phase 2 — World rendering (textured, lit, real)

**Goal:** the spike renderer becomes the real world pipeline. Deliverable: fly through any
live server world and it *looks like Minecraft*.

Scope: block registry + block-state → model/texture mapping (generated export from dragonfly's registry via `tools/registrygen`, shipped as a binary asset); vanilla asset ingestion from **Mojang/bedrock-samples** pinned to the matching game version (block models, terrain textures, `blocks.json`, flipbooks) — NOTE: BDS `resource_packs/vanilla` is server-minimal (blocks.json + texts only), it is a data reference, not the texture source; texture atlas + mipmaps; greedy/culled meshing with transparency layers (opaque/cutout/blend) and per-face culling; **client-side light engine** (block + sky flood-fill, per-vertex light, day/night); biome tinting (grass/foliage/water); sky, fog, clouds; chunk streaming/eviction tied to `ChunkRadiusUpdated` + `SubChunk` request flow; block entities with custom renderers deferred (chests/signs get static models in this phase).
Perf budget carried from Phase 0 gate; add: full remesh of view distance after teleport ≤ 2s.

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
