# Phase 0 — Spike: prove the stack end-to-end (DETAILED, executable now)

[Roadmap](../../plan.md) · [Acceptance ledger](../evidence/phases-2-5-completion-ledger.md)

This is the retained phase checklist. Checked items describe their recorded scope only;
unchecked items and explicit limitations remain open. Historical verification statements
are not fresh test results. Follow current repository guidance when running checks.


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
- [ ] **0.8 Acceptance run.** Connect to BDS world, render 16-chunk radius, fly at speed, break/place blocks from a second client to force live remeshing. Repeat the Task 0.7 Computer Use interaction checklist in the live streamed world and record the result. Before the run, resolve the recorded `AvailableCommands` live drift and remaining protocol conformance coverage from `crates/protocol/DEVIATIONS.md`. **Gate: p99 frame time ≤ 8ms on the dev MacBook at 16 chunks; remesh of a modified sub-chunk visible ≤ 100ms; zero decode errors over a 15-minute session (or all errors adjudicated as 0.4-style findings and fixed).** Record numbers in the phase report.
  - Historical Windows evidence at `3898530` passed: 900.0015 s, radius 16/16/16, p99 5.1 ms, 432/432 visible mutations, max mutation-to-visible 45.4522 ms, zero decode errors, clean shutdown. At that revision Phase 0 was **CONDITIONAL GO**, pending only the authoritative dev MacBook p99 run.
  - **Current-candidate performance audit (2026-08-02, local and uncommitted):**
    a release baseline at `.local/acceptance/20260802T174927Z-10764` recorded
    p50/p95/p99 frame times of 13.4/17.3/60.8 ms and 263.79 ms maximum
    mutation-to-visible latency. The audited candidate enables thin LTO with one
    codegen unit, avoids unchanged extraction/GPU scans, reuses neighbour palette
    resolution, skips model voxel scans when the palette has no model geometry,
    reuses render-queue sorting storage, uses bounded direct deflate reads, and
    removes avoidable acceptance-diagnostics work. It also replaces runtime
    SipHash maps/sets only where keys are already bounded internal identities;
    untrusted/network-key maps retain their existing hashers.
  - The environment-gated stage profiler is retained as durable attribution
    telemetry. Instrumented run
    `.local/acceptance/20260802T191930Z-2188` passed the complete Windows gate
    with p50/p95/p99 10.0/14.0/14.9 ms, 76.9395 ms maximum
    mutation-to-visible latency, zero decode errors, and drained light/mesh
    queues. A later final-source profile attributed wall time primarily to
    transparent preparation (15.17%), the transparent worker (9.09%), cave
    visibility (8.57%), and opaque queue preparation (6.38%); chunk extraction,
    GPU preparation, and render-queue application were each below 1%.
  - Two consecutive uninstrumented final-source runs
    (`20260802T213056Z-4332`, `20260802T213439Z-8848`) passed the complete
    Windows gate with p50 10.2 ms, p95 17.0-18.2 ms, p99 22.8-31.7 ms,
    maximum mutation-to-visible latency 76.9016-86.3247 ms, and zero decode
    errors. Live block mutations now retain urgent priority through lighting,
    meshing, publication, and GPU upload; changes whose old and new palettes
    have identical emission/filter semantics skip redundant relighting. Maximum
    remesh latency was 78.9989-119.2461 ms. Phase 0 remains **CONDITIONAL GO**
    pending the authoritative dev MacBook p99 run; the Windows mutation and
    decode gates are cleared by these repeated final-source runs.

**Exit criteria:** acceptance gate met; deviations documented; go/no-go written up. Everything after this phase is "build the game", with the architecture de-risked.

---
