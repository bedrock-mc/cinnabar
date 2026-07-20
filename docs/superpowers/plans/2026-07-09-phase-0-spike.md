# Phase 0 End-to-End Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove that a greenfield Rust/Bevy client can join BDS through a Go gophertunnel core, decode current Bedrock chunks, and render a live 16-chunk-radius world within the Phase 0 performance gates.

**Architecture:** The Go core terminates an encrypted Bedrock session on each side and relays decoded gophertunnel packet values after the upstream/downstream spawn barrier completes. Rust connects to the core over a local length-framed byte stream, uses vendored Valentine/Jolyne 1.26.30 code for Bedrock batches and login, decodes sub-chunks into a small world store, meshes them on Rayon, and uploads debug-colour geometry to Bevy.

**Tech Stack:** Rust 1.93.1, Bevy 0.18.1, Tokio 1.52.x, Rayon 1.11.x, Valentine/Jolyne from axolotl-stack commit `6f6806e821a579c183c44d786f76d9b358a2b825`, Go 1.26.1, gophertunnel branch `rust-mcbe-bounded-close` commit `9948b1729395d2e819fce28e079d4a7bfc67716c`, Dragonfly commit `b85c56ffea6b306798a935f14cc941c76618be52`, BDS 1.26.32.2.

## Global Constraints

- Pinned loopback protocol version: **1.26.30 / protocol 1001**.
- The Rust side never implements upstream auth, upstream encryption, RakNet, or NetherNet. Its encryption is only the default gophertunnel listener session on the local leg.
- Do not reuse Rust-LCE code, assets, renderer, world model, or world generation. The client is greenfield against current Bedrock/BDS data.
- Relay decoded Bedrock packet values between the two independently negotiated Go connections. Never forward RakNet datagrams or encrypted/compressed batches between legs.
- The proxy does not start ordinary packet pumps until downstream `StartGame` and upstream `DoSpawn` have both completed successfully.
- Login order is `RequestNetworkSettings → Login → encrypted handshake → resource packs → StartGame → RequestChunkRadius(16) → PlayerSpawn/ChunkRadiusUpdated → SetLocalPlayerAsInitialized`.
- Handle both `LevelChunk` payloads and the `SubChunkRequest`/`SubChunk` response path.
- New Go exported types and methods receive doc comments at creation.
- `.local/refs` and `.local/bds` are read-only development inputs and remain gitignored. Committed builds must not require those directories except live tests guarded by `BEDROCK_BDS_DIR`.
- Never edit reference checkouts under `.local/refs`.
- No decompile/reverse-engineering provenance appears in code, commits, or public docs.
- Every behavioral change follows RED → GREEN → REFACTOR. Generated/vendor copying and repository configuration are the only non-behavioral exceptions.

## Local Stream Contract

- Unix endpoint: `<socket-dir>/game.sock`; create `<socket-dir>` owner-only, remove only an existing socket owned by the current user, and create the socket as owner-only.
- Windows endpoint: bind an ephemeral `127.0.0.1` TCP port and atomically write its ASCII address plus newline to `<socket-dir>/game.addr`.
- Each connection carries exactly one Bedrock client session. No multiplexing or control messages exist in Phase 0.
- Each outer frame is `u32` big-endian payload length followed by exactly that many bytes. The payload is one complete gophertunnel Bedrock batch, including its `0xfe` batch header.
- Valid payload length is 1 through 64 MiB. Zero, oversized lengths, partial headers, and partial payloads are protocol errors. Clean EOF is valid only between frames.
- A Go framed connection implements gophertunnel `packet.PacketReader`; `Write` emits one outer frame. It does not implement `EncryptionDisabler`, so default AES remains enabled.
- Backpressure is lossless: writes block or return a context/deadline error. Login, spawn, and gameplay frames are never silently dropped.

---

### Task 1: Repository and Workspace Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `README.md`
- Create: `app/Cargo.toml`, `app/src/main.rs`
- Create: `crates/bridge/Cargo.toml`, `crates/bridge/src/lib.rs`
- Create: `crates/protocol/Cargo.toml`, `crates/protocol/src/lib.rs`
- Create: `crates/world/Cargo.toml`, `crates/world/src/lib.rs`
- Create: `crates/render/Cargo.toml`, `crates/render/src/lib.rs`
- Create: `core/go.mod`, `core/cmd/bedrock-core/main.go`
- Create: `go.work`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces a Cargo workspace with packages `bridge`, `protocol`, `world`, `render`, and `bedrock-client`.
- Produces Go module `github.com/hashimthearab/rust-mcbe/core` and binary `bedrock-core`.

- [ ] **Step 1: Create workspace manifests and zero-behaviour crate entry points**

  Pin `bevy = "=0.18.1"`, `tokio = "1.52"`, `tokio-util = "0.7"`, `bytes = "1"`, `futures = "0.3"`, `anyhow = "1"`, `thiserror = "2"`, and `rayon = "1.11"` in `[workspace.dependencies]`. Use Cargo resolver 3 and Rust edition 2024.

- [ ] **Step 2: Pin Go sources**

  In `core/go.mod`, require `github.com/sandertv/gophertunnel` and replace it with `github.com/hashimthearab/gophertunnel v1.25.3-0.20260710063825-9948b1729395`. Repeat only the go-raknet fork replacement from the pinned gophertunnel `go.mod`, because dependency replacements are not inherited. Use the upstream `df-mc/go-nethernet v1.0.18` and `df-mc/go-xsapi/v2 v2.0.2` selected by that commit.

- [ ] **Step 3: Add CI commands**

  CI runs `cargo fmt --check`, `cargo test --workspace`, `go test ./core/...`, and `go vet ./core/...`. Live BDS tests skip only when `BEDROCK_BDS_DIR` is absent.

- [ ] **Step 4: Verify the scaffold**

  Run: `cargo test --workspace`
  Expected: all empty crate test targets build and pass.

  Run: `go test ./core/...`
  Expected: the core command package builds and reports no test failures.

- [ ] **Step 5: Commit**

  Commit message: `chore: scaffold phase zero workspace`

---

### Task 2: Framed Go Network and Core Proxy

**Files:**
- Create: `core/internal/streamnet/conn.go`
- Create: `core/internal/streamnet/conn_test.go`
- Create: `core/internal/streamnet/endpoint.go`
- Create: `core/internal/streamnet/network.go`
- Create: `core/internal/streamnet/network_test.go`
- Create: `core/proxy/proxy.go`
- Create: `core/proxy/proxy_test.go`
- Create: `core/proxy/proxy_integration_test.go`
- Modify: `core/cmd/bedrock-core/main.go`

**Interfaces:**
- Produces `streamnet.New(socketDir string) minecraft.Network`.
- Produces `streamnet.Resolve(socketDir string) (network, address string, error)` for Rust-compatible endpoint discovery.
- Produces `proxy.Config{SocketDir, Upstream string}` and `proxy.Serve(ctx context.Context, cfg Config) error`.
- CLI flags are exactly `-socket-dir <dir>` and `-upstream <host:port>`.

- [ ] **Step 1: Write framing tests**

  Cover a one-byte frame, a 64 MiB boundary decision without allocating 64 MiB, sequential FIFO frames, partial header EOF, partial payload EOF, zero length, oversized length, write deadline propagation, and blocked-peer cancellation.

- [ ] **Step 2: Verify RED**

  Run: `go test ./core/internal/streamnet -run 'TestFrame|TestResolve' -count=1`
  Expected: compile failure because `NewFramedConn` and endpoint resolution do not exist.

- [ ] **Step 3: Implement stream framing and endpoints**

  Implement a `net.Conn` wrapper whose `ReadPacket` uses `io.ReadFull` for the big-endian length and payload, and whose `Write` serializes a complete frame under one write mutex. Implement a `minecraft.NetworkListener` wrapper with stable random `ID`, no-op `PongData`, safe close, and safe Unix socket cleanup. Unix uses UDS; Windows uses loopback TCP plus atomic `game.addr` publication.

- [ ] **Step 4: Verify GREEN**

  Run: `go test ./core/internal/streamnet -count=1`
  Expected: all framing and endpoint tests pass with no goroutine leaks or warnings.

- [ ] **Step 5: Write proxy lifecycle tests**

  Use fake packet connections to prove: no forwarding before both spawn-barrier functions return; a failure on either barrier cancels the other; FIFO forwarding begins after readiness; disconnect closes both sides; runtime-ID mismatch is returned by gophertunnel rather than swallowed.

- [ ] **Step 6: Verify proxy RED**

  Run: `go test ./core/proxy -run 'TestRelay|TestSpawnBarrier' -count=1`
  Expected: compile failure because `ServeConnections`/relay logic does not exist.

- [ ] **Step 7: Implement the gophertunnel MITM flow**

  Accept the downstream `*minecraft.Conn`, dial upstream with copied `ClientData` and offline identity for the Phase 0 BDS, run downstream `StartGame(serverConn.GameData())` and upstream `DoSpawn()` in an error group, then start two packet-level `ReadPacket`/`WritePacket` pumps. Return errors; never panic in connection goroutines.

- [ ] **Step 8: Verify proxy GREEN**

  Run: `go test ./core/proxy -count=1`
  Expected: unit tests pass.

- [ ] **Step 9: Add and run the load-bearing BDS integration test**

  `TestProxyJoin` takes an exclusive OS-backed lease on an ignored stable runtime cache, preserves the stable executable path, resets mutable BDS data from the read-only source, waits for `Server started.`, starts the core on a temporary socket directory, dials the core with gophertunnel over `streamnet`, completes `DoSpawn`, and asserts protocol 1001 plus non-zero StartGame runtime entity ID. It sends `stop` and waits for a clean BDS exit in cleanup.

  Run from PowerShell:
  `$env:BEDROCK_BDS_DIR="$PWD/.local/bds/bedrock-server-1.26.32.2"; go test ./core/... -run TestProxyJoin -count=1 -v`
  Expected: PASS and BDS exits cleanly.

- [ ] **Step 10: Commit**

  Commit message: `feat: add framed core proxy`

---

### Task 3: Rust Bridge Crate

**Files:**
- Create: `crates/bridge/src/endpoint.rs`
- Create: `crates/bridge/src/framed.rs`
- Create: `crates/bridge/src/error.rs`
- Create: `crates/bridge/tests/go_echo.rs`
- Create: `core/cmd/frame-echo/main.go`
- Modify: `crates/bridge/src/lib.rs`

**Interfaces:**
- Produces `pub async fn connect(socket_dir: &Path) -> anyhow::Result<FramedStream>`.
- `FramedStream` implements `Stream<Item = Result<Bytes, BridgeError>> + Sink<Bytes, Error = BridgeError> + Unpin + Send`.
- Produces `pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024`.

- [ ] **Step 1: Write Rust codec tests**

  Test big-endian frame bytes, FIFO frames, zero/oversized rejection, partial EOF, and conversion from `BytesMut` to immutable `Bytes`.

- [ ] **Step 2: Verify RED**

  Run: `cargo test -p bridge --lib`
  Expected: compile failure because `FramedStream` and `connect` do not exist.

- [ ] **Step 3: Implement platform stream and codec**

  Use `tokio::net::UnixStream` on Unix and `tokio::net::TcpStream` with `game.addr` on Windows. Configure `LengthDelimitedCodec` for a four-byte big-endian length at offset zero, no adjustment, and 64 MiB maximum. Wrap platform variants behind one enum and map all errors to `BridgeError`.

- [ ] **Step 4: Verify GREEN**

  Run: `cargo test -p bridge --lib`
  Expected: unit tests pass.

- [ ] **Step 5: Add the cross-language echo fixture**

  `frame-echo` listens with `streamnet`, echoes each `ReadPacket` through `Write`, and exits after its client disconnects. The Rust integration test builds/spawns it, waits for endpoint publication, round-trips binary payloads including embedded zero bytes and a 1 MiB frame, then verifies clean shutdown.

- [ ] **Step 6: Run the bridge suite**

  Run: `cargo test -p bridge -- --nocapture`
  Expected: unit and Go echo integration tests pass.

- [ ] **Step 7: Commit**

  Commit message: `feat: add framed rust bridge`

---

### Task 4: Vendored Protocol Definitions and Byte Fixtures

**Files:**
- Create: `crates/protocol/vendor/UPSTREAM.md`
- Create: `crates/protocol/vendor/LICENSE`
- Vendor: `crates/protocol/vendor/valentine/`
- Vendor: `crates/protocol/vendor/jolyne/`
- Create: `crates/protocol/src/codec.rs`
- Create: `crates/protocol/src/packet.rs`
- Create: `crates/protocol/tests/fixtures.rs`
- Create: `crates/protocol/fixtures/*.bin`
- Create: `crates/protocol/fixtures/manifest.json`
- Create: `crates/protocol/DEVIATIONS.md`
- Create: `tools/fixturegen/go.mod`
- Create: `tools/fixturegen/main.go`
- Modify: `go.work`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Produces `pub type Packet = valentine::bedrock::version::v1_26_30::McpePacket`.
- Produces `pub fn decode_batch(bytes: Bytes, session: &BedrockSession) -> Result<Vec<Packet>, ProtocolError>`.
- Produces `pub fn encode(packet: &Packet, session: &BedrockSession) -> Result<Bytes, ProtocolError>`.
- Re-exports protocol constants `GAME_VERSION == "1.26.30"` and `PROTOCOL_VERSION == 1001`.

- [ ] **Step 1: Vendor exact upstream sources**

  Copy Valentine generated/core crates and the Jolyne session crate from axolotl-stack merge commit `6f6806e821a579c183c44d786f76d9b358a2b825`. Preserve the upstream MIT license. Convert their workspace manifests to local path/version dependencies without changing generated packet code. Guard Jolyne RakNet-only imports so the `raknet` feature remains disabled.

- [ ] **Step 2: Add fixture generation**

  For `NetworkSettings`, `StartGame`, `LevelChunk`, `MovePlayer`, and `AddActor`, write a gophertunnel packet header and payload using the pinned protocol writer, wrap it with `packet.NewEncoder`, and emit deterministic raw batch files plus a manifest containing packet name, ID, byte length, and SHA-256.

- [ ] **Step 3: Write fixture decode tests**

  Assert protocol/version constants and exact decoded packet variants plus representative fields for every fixture. Re-encode each decoded value and assert exact gophertunnel bytes.

- [ ] **Step 4: Verify RED**

  Run: `go run ./tools/fixturegen -out ./crates/protocol/fixtures`

  Run: `cargo test -p protocol --test fixtures -- --nocapture`
  Expected: compile failure because the wrapper codec is absent, or a precise field mismatch identifying protocol drift.

- [ ] **Step 5: Implement the wrapper codec**

  Adapt Jolyne batch encode/decode around Valentine `McpePacketData`, retain the `0xfe` header, enforce 16 MiB decompressed input and 1,600 packets per batch, and preserve sender/target subclient header fields. Do not accept trailing bytes.

- [ ] **Step 6: Resolve drift against primary sources**

  For each mismatch, compare gophertunnel and Mojang bedrock-protocol-docs. Patch the vendored Valentine definition when it is wrong; patch a new branch from gophertunnel `lunar` only when gophertunnel is wrong. Record packet, field, evidence, and chosen fix in `DEVIATIONS.md`. An empty deviations file states that no mismatches were found and names the tested commits.

- [ ] **Step 7: Verify GREEN**

  Run: `cargo test -p protocol --test fixtures -- --nocapture`
  Expected: all five fixtures decode and byte-round-trip exactly.

- [ ] **Step 8: Commit**

  Commit message: `feat: vendor protocol 1001 definitions`

---

### Task 5: Encrypted Login Sequence over the Bridge

**Files:**
- Create: `crates/protocol/src/socket_transport.rs`
- Create: `crates/protocol/src/login.rs`
- Create: `crates/protocol/tests/login_state.rs`
- Create: `crates/protocol/tests/login.rs`
- Modify: `crates/protocol/vendor/jolyne/src/stream/client.rs`
- Modify: `crates/protocol/src/lib.rs`

**Interfaces:**
- Produces `pub struct LoginSequence` with `pub async fn connect(socket_dir: &Path, display_name: &str) -> Result<(PlaySession, GameData), ProtocolError>`.
- `PlaySession` exposes `async fn recv(&mut self) -> Result<Packet, ProtocolError>` and `async fn send(&mut self, packet: Packet) -> Result<(), ProtocolError>`.
- The transport implements Jolyne `Transport` with `USES_BATCH_PREFIX = true` and never implements or requests RakNet.

- [ ] **Step 1: Write scripted-transport state tests**

  Feed real encoded batches through an in-memory framed transport and assert outgoing order: RequestNetworkSettings, Login, encrypted ClientToServerHandshake, resource-pack responses, RequestChunkRadius with both radius fields 16 only after StartGame, loading-screen end, and SetLocalPlayerAsInitialized with the exact StartGame runtime ID. Cover PlayStatus/ChunkRadiusUpdated arriving in either order and runtime-ID mismatch failure.

- [ ] **Step 2: Verify RED**

  Run: `cargo test -p protocol --test login_state -- --nocapture`
  Expected: compile failure because `LoginSequence` and socket transport do not exist.

- [ ] **Step 3: Implement socket transport and strict typestate wrapper**

  Adapt the vendored Jolyne client: self-signed P-384 login chain, server JWT verification, ECDH/SHA-256 key derivation, AES-CTR/checksum batches, negotiated deflate/snappy handling, empty resource-pack completion, StartGame capture, radius 16, and exact initialization ID. Decode errors are returned and counted; they are not silently skipped.

- [ ] **Step 4: Verify state GREEN**

  Run: `cargo test -p protocol --test login_state -- --nocapture`
  Expected: all state/order tests pass.

- [ ] **Step 5: Add live core+BDS login test**

  The test starts BDS and the Go core using temporary run/socket directories, calls `LoginSequence::connect`, asserts GameData protocol/version/runtime ID and receipt of world-stream packets, then shuts both processes down cleanly.

- [ ] **Step 6: Run live login**

  Run from PowerShell:
  `$env:BEDROCK_BDS_DIR="$PWD/.local/bds/bedrock-server-1.26.32.2"; cargo test -p protocol --test login -- --nocapture`
  Expected: reaches Play state and passes without decode warnings.

- [ ] **Step 7: Commit**

  Commit message: `feat: add encrypted bridge login`

---

### Task 6: Sub-Chunk Decoder and Golden Fixtures

**Files:**
- Create: `crates/world/src/error.rs`
- Create: `crates/world/src/palette.rs`
- Create: `crates/world/src/sub_chunk.rs`
- Create: `crates/world/src/chunk.rs`
- Create: `crates/world/src/store.rs`
- Create: `crates/world/tests/sub_chunk.rs`
- Create: `crates/world/fixtures/*.bin`
- Create: `crates/world/fixtures/manifest.json`
- Create: `tools/chunkfix/go.mod`
- Create: `tools/chunkfix/main.go`
- Modify: `go.work`
- Modify: `crates/world/src/lib.rs`

**Interfaces:**
- Produces `SubChunk::decode(bytes: &[u8]) -> Result<SubChunk, DecodeError>`.
- Produces `SubChunk::runtime_id(layer: usize, x: u8, y: u8, z: u8) -> Option<u32>`.
- Produces `ChunkStore::apply_level_chunk(...)` and `ChunkStore::apply_sub_chunk(...)` with dimension/chunk/sub-chunk keys.

- [x] **Step 1: Generate Dragonfly goldens**

  Use pinned Dragonfly `chunk.EncodeSubChunk(..., chunk.NetworkEncoding, index)` to emit version-9 sub-chunks for uniform, checkerboard, vertical layers, two storage layers, and palette widths crossing 1/2/3/4/5/6/8/16 bits. The manifest lists expected runtime IDs at named coordinates.

- [x] **Step 2: Write failing Rust golden tests**

  Assert every named coordinate, storage count, Y index, palette size, and malformed-input error. Add hand-built version-1 and version-8 compatibility fixtures and unsupported-version rejection.

- [x] **Step 3: Verify RED**

  Run: `go run ./tools/chunkfix -out ./crates/world/fixtures`

  Run: `cargo test -p world --test sub_chunk -- --nocapture`
  Expected: compile failure because the decoder does not exist.

- [x] **Step 4: Implement palette and sub-chunk decoding**

  Port the version 1/8/9 network layout and paletted-storage bit packing from pinned Dragonfly. Enforce coordinate bounds, storage count bounds, palette-length bounds, checked word arithmetic, exact EOF handling, and no panics on arbitrary input.

- [x] **Step 5: Implement minimal chunk store ingestion**

  Store chunk/sub-chunk data keyed by dimension and coordinates, replace changed sub-chunks atomically, and return a dirty-sub-chunk key for remeshing. Support full LevelChunk block payloads and individual SubChunk responses.

- [x] **Step 6: Verify GREEN**

  Run: `cargo test -p world -- --nocapture`
  Expected: all goldens and malformed-input tests pass.

- [x] **Step 7: Commit**

  Commit message: `feat: decode bedrock sub chunks`

---

### Task 7: Packed Bevy Chunk Renderer and Live Remeshing

**Complete at `f2a6a1c`** (400 workspace tests, strict all-target Clippy, and independent review approved).

**Files:**
- Create: `crates/render/src/color.rs`
- Create: `crates/render/src/mesh.rs`
- Create: `crates/render/src/plugin.rs`
- Create: `crates/render/src/chunk.wgsl`
- Create: `crates/render/tests/mesh.rs`
- Create: `crates/render/tests/plugin.rs`
- Create: `app/src/args.rs`
- Create: `app/src/camera.rs`
- Create: `app/src/culling.rs`
- Create: `app/src/network.rs`
- Create: `app/src/metrics.rs`
- Create: `app/src/world_stream.rs`
- Modify: `app/src/main.rs`
- Modify: `crates/render/src/lib.rs`
- Modify: `crates/world/src/mutation.rs`
- Modify: `crates/world/src/palette.rs`
- Modify: `crates/world/src/store.rs`
- Create: `crates/world/tests/mutation.rs`
- Modify: `crates/world/tests/store.rs`

**Interfaces:**
- Produces `mesh_sub_chunk(classifier: &BlockClassifier, neighbours: &Neighbourhood<'_>, sub_chunk: &SubChunk) -> ChunkMesh`, where `ChunkMesh` retains one 8-byte `PackedQuad` record per greedy quad plus face-to-face cave connectivity.
- Produces `ChunkRenderQueue::{insert, update, remove}` with nearest-first priorities and a per-frame upload cap.
- Produces `ChunkRenderPlugin`, a custom non-`Mesh` chunk render path backed by global packed-quad/origin/indirect buffers, one pipeline, and one bind group.
- Produces `WorldStream`, which applies ordered world events, dispatches bounded decode/mesh work to Rayon, and emits deduplicated mesh changes and sub-chunk requests.
- App flags: `--socket-dir`, `--acceptance-seconds`, `--metrics-out`, and `--auto-fly`.

- [x] **Step 1: Write packed-meshing, connectivity, and render-queue tests**

  Assert: `PackedQuad` is exactly 8 bytes; one opaque block emits six packed quads; identical adjacent blocks greedy-merge into six rectangular-prism quads; differing runtime IDs split coplanar merges while internal faces remain culled; all six boundary directions cull against neighbouring sub-chunks; uniform air emits no geometry and remains fully traversable; a uniform solid sub-chunk emits six 16x16 quads; storage layers preserve first-non-air selection; high-bit runtime IDs remain intact; empty tunnels and sealed cavities produce the expected face-to-face connectivity; runtime-ID debug colours are deterministic and opaque. Add plugin tests for nearest-first capped uploads, update/removal deduplication, frustum-cullable sub-chunk bounds, shader parsing, indirect-command construction, and capability-selected MDI/direct draw modes.

- [x] **Step 2: Verify RED**

  Run: `cargo test -p render --test mesh -- --nocapture`
  Run: `cargo test -p render --test plugin -- --nocapture`
  Expected: compile failure because the packed mesher, render queue, and custom draw path do not exist.

- [x] **Step 3: Implement packed-palette binary greedy meshing**

  Mesh directly from each sub-chunk's palette and packed indices without expanding to a flat per-block array. Keep uniform sub-chunks as one palette entry, skip uniform air immediately, and handle uniform solids without materializing 4096 values. Build per-axis-column `u64` occupancy masks; compute exposed faces with shifts and bitwise AND-NOT operations; greedy-merge equal-runtime-ID coplanar runs, splitting at material boundaries and culling against all six neighbouring sub-chunks. Emit one 8-byte `PackedQuad` containing local position, face, extents, and complete runtime ID, and compute the sub-chunk's six-face cave-connectivity matrix during the same worker job.

- [x] **Step 4: Verify mesh GREEN**

  Run: `cargo test -p render --test mesh -- --nocapture`
  Expected: all packing, greedy merge, boundary culling, uniform fast-path, runtime-ID, and connectivity assertions pass.

- [x] **Step 5: Implement the custom packed chunk render phase**

  Add one chunk render path/phase instead of allocating a Bevy `Mesh`, `StandardMaterial`, vertex buffer, or bind group per sub-chunk. Store all packed quads and per-draw origins in global GPU arenas, use one shared six-index buffer, and reconstruct quad corners and normals in `chunk.wgsl` with vertex pulling. Maintain one pipeline and bind group. Build one indexed-indirect command per visible sub-chunk and issue `multi_draw_indexed_indirect` when the adapter supports indirect execution and first-instance indexing; select a tested `draw_indexed` direct fallback otherwise. Account arena growth/reallocation against the upload budget and coalesce freed ranges so churn does not force unbounded buffers or unbudgeted full-buffer uploads.

- [x] **Step 6: Add bounded streaming, visibility, and live app integration**

  First add failing `world` tests and packed-palette mutation APIs for UpdateBlock/UpdateSubChunkBlocks plus full-column eviction; retain sparse/all-air storage and expand changed keys through `mesh_dependents`. Keep Bevy/winit and all GPU access on the main thread. Run Tokio login/packet receive on a dedicated thread, preserve FIFO world-event order, and deliver events through bounded channels. Apply LevelChunk, SubChunk, UpdateBlock, and UpdateSubChunkBlocks to `ChunkStore`; deduplicate dirty keys; run decode and binary-greedy mesh jobs only on bounded Rayon workers; discard stale completions by revision; and cap nearest-first GPU uploads each frame. Retain logical all-air nodes in the connectivity graph, perform per-sub-chunk frustum culling plus camera-rooted face-connectivity BFS, and fall back to frustum-only visibility when the camera sub-chunk is absent. Add WASD/space/shift fly camera, mouse look, no-vsync acceptance mode, and radius-16 request through the login session.

- [x] **Step 7: Add metrics**

  Record per-frame duration, packet/decode errors, resident/visible chunk counts, bounded queue depths, decode/mesh job duration, GPU upload volume, and update-to-visible latency. On timed acceptance exit, write deterministic JSON containing p50/p95/p99/max frame milliseconds, max decode/mesh/remesh latency, decode-error count, rendered chunks, peak queue depths, and session seconds.

- [x] **Step 8: Verify focused tests and workspace**

  Run: `cargo test -p world -- --nocapture`
  Expected: packed mutation, eviction, dependency, and world-store tests pass.

  Run: `cargo test -p render -- --nocapture`
  Expected: binary-greedy packing, custom render queue, shader, MDI selection, and direct-fallback tests pass.

  Run: `cargo test --workspace`
  Expected: all Rust tests pass.

  Run: `cargo run -p bedrock-client -- --help`
  Expected: documents all four app flags and exits zero.

- [x] **Step 9: Commit**

  Commit message: `feat: render packed live chunks`

---

### Task 8: Automated Acceptance Run and Phase Report

**Files:**
- Create: `scripts/acceptance.ps1`
- Create: `scripts/acceptance.sh`
- Create: `docs/phase-0-report.md`

**Interfaces:**
- `scripts/acceptance.ps1 -DurationSeconds 900 -BdsDir <path> -MetricsOut <path>` orchestrates Windows.
- `scripts/acceptance.sh --duration 900 --bds-dir <path> --metrics-out <path>` orchestrates macOS/Linux with an available BDS host/upstream.

- [ ] **Step 1: Write orchestration smoke tests**

  Add script dry-run modes that validate paths, print exact BDS/core/app commands, reject durations below 60 seconds, and never leave child processes running. Test dry-run success and missing-BDS failure.

- [ ] **Step 2: Verify script RED then GREEN**

  Run: `powershell -NoProfile -File scripts/acceptance.ps1 -DryRun -DurationSeconds 900 -BdsDir .local/bds/bedrock-server-1.26.32.2 -MetricsOut .local/acceptance/metrics.json`
  Expected before implementation: script missing. Expected after implementation: three commands printed, exit zero.

- [ ] **Step 3: Implement live orchestration**

  Copy BDS into a per-run directory, start it and wait for readiness, start `bedrock-core`, wait for endpoint publication, start the visible Bevy app in timed auto-fly/no-vsync mode, and issue alternating BDS `setblock` commands near spawn to force live remeshing. Always request graceful shutdown; force-kill only after a bounded timeout. Preserve logs and metrics under `.local/acceptance/<timestamp>/`.

- [ ] **Step 4: Run focused and full verification**

  Run:
  `$env:BEDROCK_BDS_DIR="$PWD/.local/bds/bedrock-server-1.26.32.2"; go test ./core/... -count=1`

  Run:
  `$env:BEDROCK_BDS_DIR="$PWD/.local/bds/bedrock-server-1.26.32.2"; cargo test --workspace -- --nocapture`

  Run:
  `cargo fmt --check`

  Run:
  `cargo clippy --workspace --all-targets -- -D warnings`

  Expected: all commands exit zero with no warnings.

- [ ] **Step 5: Run the 15-minute Windows acceptance session**

  Run:
  `powershell -NoProfile -File scripts/acceptance.ps1 -DurationSeconds 900 -BdsDir .local/bds/bedrock-server-1.26.32.2 -MetricsOut .local/acceptance/windows-metrics.json`

  Preliminary Windows gates: renders a 16-chunk radius, zero decode errors, modified sub-chunk visible within 100 ms, and records uncapped p99 frame time. The authoritative p99 ≤ 8 ms claim remains reserved for the specified dev MacBook.

- [ ] **Step 6: Record the go/no-go result**

  `docs/phase-0-report.md` records exact commits, machine/GPU/display, commands, session duration, radius, p50/p95/p99/max frame time, remesh latency, decode errors, deviations, Windows result, and the still-required MacBook result. State GO only if all architecture/decode/remesh gates pass and the MacBook p99 gate has evidence; otherwise state CONDITIONAL GO or NO-GO with the exact failed gate.

- [ ] **Step 7: Commit**

  Commit message: `test: record phase zero acceptance`
