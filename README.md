# Cinnabar

> An independent, unofficial client compatible with Minecraft: Bedrock Edition. Not approved by
> or associated with Mojang or Microsoft. Minecraft is a trademark of Microsoft Corporation.

A Bedrock client written in Rust (Bevy/wgpu), targeting vanilla parity with the release pinned
in `assets/bedrock-target.json`. A small Go core handles Microsoft sign-in and upstream
networking.

[![Discord](https://img.shields.io/badge/Discord-Join%20us-5865F2?logo=discord&logoColor=white)](https://discord.gg/MeEz7BEHcM)
[![Website](https://img.shields.io/badge/Website-cinnabar.restartfu.com-B22222)](https://cinnabar.restartfu.com/)

<img width="2534" height="1446" alt="Cinnabar in game" src="https://github.com/user-attachments/assets/836cb337-3876-4b31-a97e-9cfb25227b11" />

## Download

Daily builds of `dev` for macOS, Windows and Linux: [nightly](https://github.com/bedrock-mc/cinnabar/releases/tag/nightly).
Stable: [latest release](https://github.com/bedrock-mc/cinnabar/releases/latest). First launch fetches
the vanilla resource pack after you accept the Minecraft EULA; the release notes cover unsigned builds.

## Play

```sh
make play
```

This downloads and compiles the vanilla assets on first run (and whenever they're stale), builds
the Go core, and opens the launcher menu. The first sign-in prints a Microsoft device code; the
token is cached in `.local/auth/`, which holds private credentials, so never share or commit it.
`make play` builds with the fast `play` profile (parallel codegen, incremental rebuilds, sccache when
installed); `make play PROFILE=release` builds the fully optimised shipped binary.

To join one server directly without the menu, run the core and client in two terminals:

```sh
make core UPSTREAM=zeqa.net:19132
make client
```

`make help` lists every target. On Debian/Ubuntu, install `libwayland-dev` and `libudev-dev` first; Linux picks
Wayland or X11 automatically.

## Discord presence

Discord presence is enabled by default using the built-in application. To use another application,
set `CINNABAR_DISCORD_APPLICATION_ID` to its numeric Application ID before launching.
No bot token or client secret is needed. Set the override to `0` to disable presence.

With the Discord desktop app running and activity sharing enabled, presence shows menus, joining,
or `Playing on host:port`, plus elapsed time and the original app icon served from GitHub.
Updates run over local IPC, reconnect automatically and follow Discord's rate limit. The current
server address is shown while playing; account details and join secrets are never included.

## Beyond vanilla

Vanilla parity is the default. On top of it, Cinnabar is growing into a platform. Everything
below is opt-in and off unless you, or the server you join, turn it on.

| | What it is | Status |
| --- | --- | --- |
| **Cinnabar Experiences** | A Roblox-style engine. Servers ship sandboxed client code that can replace the UI, rendering, input and game logic, turning a server into an entirely different game. | Preview, off by default: [docs/server-experiences.md](docs/server-experiences.md) |
| **Video streaming** | Servers can stream video with its own synced audio onto in-world screens, blocks, entities and UI. Video loads over HTTPS from any static host or CDN, not through the game connection. It's built into the client, so no server code is needed. | Preview, off by default: [docs/server-experiences.md](docs/server-experiences.md) |
| **Mods** | Client mods as WebAssembly components with versioned, capability-scoped APIs. Each mod runs sandboxed with no file, network or account access, and hot-reloads. A crashing mod is disabled instead of taking down the client. | Developer preview: [docs/modding-spike.md](docs/modding-spike.md) |
| **Mod marketplace** | Browse, install and update mods from inside Cinnabar. | Coming soon |
| **Live resource packs** | Add, remove or reorder resource packs without leaving the world. | Available |

## How it fits together

```text
bedrock-client (Rust)  ── local socket ──  bedrock-core (Go, gophertunnel)
                                             ├─ go-raknet ──── servers and BDS
                                             └─ go-nethernet ─ Realms and friend worlds
```

Rust never implements Xbox authentication, encryption, RakNet or NetherNet; the core owns those
and relays packets over a local stream.

| Library | Used for |
| --- | --- |
| [protocolgen](https://github.com/bedrock-mc/protocolgen) | Generates the Bedrock packet definitions behind `crates/protocol`. |
| [Axolotl Stack](https://github.com/axolotl-stack/axolotl-stack) | Valentine (packet codec) and Jolyne (client transport), vendored in `crates/protocol/vendor`. |
| [gophertunnel](https://github.com/Sandertv/gophertunnel) | Bedrock login, encryption, resource packs and the packet relay. |
| [go-raknet](https://github.com/Sandertv/go-raknet) | RakNet transport to servers, plus server-list pings. |
| [go-nethernet](https://github.com/df-mc/go-nethernet) | WebRTC transport for Realms and friend worlds. |
| [go-xsapi](https://github.com/df-mc/go-xsapi) | Xbox Live identity, friends, presence and signaling. |
| [go-playfab](https://github.com/df-mc/go-playfab) | PlayFab sign-in and the menu catalog (featured servers, marketplace). |
| [dragonfly](https://github.com/df-mc/dragonfly) | The built-in local-world server in `tools/localserver`. |

Mojang assets are never committed or embedded. `make assets` fetches Mojang's official
`bedrock-samples` pack (EULA-gated) and compiles it into carriers under the ignored `.local/`.

## Workspace

| Crate | What it does |
| --- | --- |
| `app` | The `bedrock-client` binary: Bevy app, networking glue, gameplay, menus and HUD. |
| `crates/asset-compiler` | `assetc`, which compiles the vanilla pack into the runtime carriers. |
| `crates/assets` | Readers for pack sources and compiled carriers. |
| `crates/bridge` | The local stream between the client and the Go core. |
| `crates/client-world` | Authoritative world state, actors, items, decoding and ordered commits. |
| `crates/chunk-pipeline` | Terrain residency, mesh scheduling and bounded publication. |
| `crates/experience-runtime`, `crates/experience-sdk` | Runs a server Experience out of process; the guest SDK generated from `wit/server.wit`. |
| `crates/input` | Device-independent input actions. |
| `crates/inventory` | Engine-independent inventory authority, prediction, crafting and commands. |
| `crates/json-ui` | Parser, resolver and layout engine for vanilla JSON-UI. |
| `crates/meshing` | CPU geometry for chunks, liquids, biomes and clouds. |
| `crates/mod-api` | Experimental guest SDK generated from the extension WIT contract. |
| `crates/mod-host` | Opt-in WASM component spike with bounded HUD and input imports. |
| `crates/pack-compiler` | Reusable pack compilation for runtime loading and `assetc`. |
| `crates/particles` | Engine-independent particle simulation: effects, Molang emitters and triggers. |
| `crates/protocol` | Bedrock packet definitions and codec. |
| `crates/render` | Chunk and entity rendering on Bevy/wgpu. |
| `crates/render-api` | Engine-independent contracts between world publication and rendering. |
| `crates/resource-pack` | Admission and decryption of server resource packs. |
| `crates/server-experience` | Opt-in Cinnabar extension negotiation; vanilla login and packet IDs are unchanged. |
| `crates/sim` | Deterministic Bedrock movement simulation. |
| `crates/ui` | Renderer-independent UI primitives and text layout. |
| `crates/world` | Palette-native chunk and world model. |
| `tools/architecture` | Architecture gate: line limits, dependency rules, markers. |
| `tools/cxb` | Publisher tooling for server Experiences: seeds, `.cxb` bundles, cache seeding. |
| `tools/jsonui-editor` | Browser JSON-UI editor on the client's own engine, live at <https://bedrock-mc.github.io/cinnabar/>. |
| `tools/jsonui-mcp` | The same editor core as an MCP server: resolve, validate, lay out, render and export packs. |
| `tools/devtool` | `verify-affected`, which tests only what a change touches. |
| `tools/dist` | Stages distributable bundles. |
| `tools/phase2-evidence`, `tools/visualcoverage` | Frozen evidence replays from earlier milestones. |

The [modding spike](docs/modding-spike.md) is a disabled-by-default Cinnabar extension; its
samples are `examples/mods/hello` and `examples/mods/time-changer`, and `examples/experiences/probe`
is the Experience runtime's test guest. None change the Bedrock wire protocol. The crate layering
plan is in `docs/architecture/`.

| Go package (`core/`) | What it does |
| --- | --- |
| `cmd/bedrock-core` | The core binary. |
| `proxy` | Upstream session, resource-pack download and packet relay. |
| `authflow`, `authcache` | Microsoft device sign-in and token cache. |
| `catalog`, `store`, `launcher`, `control` | Menu data: featured servers, Realms, friends, marketplace. |
| `localworld` | Local worlds on BDS (a container on macOS). |
| `packcache` | On-disk cache of server packs. |
| `update` | Signed update checks. |

## Headless chunk benchmarks

Criterion covers palette/column decode, full light solves, cube meshing, biome records,
bounded streaming bursts, dispatch-input capture, settled polling and metadata-only cohort
scans. Fixtures are synthetic and require no carriers, server, window or GPU.

```sh
cargo bench --locked -p world -p meshing -p chunk-pipeline --features chunk-pipeline/benchmark-support --bench chunk_costs -- --test
cargo bench --locked -p world -p meshing -p chunk-pipeline --features chunk-pipeline/benchmark-support --bench chunk_costs -- --save-baseline chunks
```

Use the repository-pinned toolchain and the shared build-slot limiter described in
[the workflow](docs/agents/multi-agent-workflow.md). Run measurements without competing
builds or gameplay. Append a name filter, such as `stored_sections/871`, to select a
workload; subsequent runs can use `--baseline chunks` for comparison. Results stay
under the ignored `target/criterion/`.

The 871 fixture means **stored sections**, packed into 218 columns, not 871 columns.
Streaming setup preloads an implicit-air boundary through ordinary ingress, excluding
absent-neighbour grace waits from timing. Timed work includes bounded byte submission,
decode/commit, lighting, meshing, worker waits and CPU publication acknowledgements;
stream construction, boundary setup and teardown are excluded. Polling is unpaced.
Preflight CPU-step percentiles are not game-frame percentiles. Metadata scans have
no terrain. Resident-slot and stale-work counters include boundary setup; production
logs remain enabled. Decode and meshing timings include output destruction.

`pipeline/dispatch_inputs` captures 4/16/64/871 overlapping light or mesh inputs,
including handle release but excluding worker scheduling and execution. Its
`DISPATCH_INPUTS` records count allocations on the dispatch thread. The `_reused`
light cases retain worker scratch between solves; completed output remains owned.

`flight_costs` streams procedurally generated terrain (dirt over ore-flecked stone with
sealed caves) into a settled radius-10 or radius-16 view while the camera flies along +X.
Each 240 Hz frame submits the server's position, view centre and new columns, polls, and
acknowledges meshes. The flight cases report main-thread stream time per frame, and the
preflight line prints its p50, p99 and maximum. Eviction cases time only the server
position update that retires the trailing row. Cave cases time one full
connectivity search from the surface and from a sealed pocket.

```sh
cargo bench --locked -p world -p meshing -p chunk-pipeline --features chunk-pipeline/benchmark-support --bench flight_costs
```

These are CPU baselines, not join-time or FPS evidence. They omit socket framing,
real server terrain, GPU preparation/uploads/draws and the rest of the Bevy frame.
Native performance acceptance still follows [live testing](docs/agents/live-testing.md).

## JSON-UI editor

[bedrock-mc.github.io/cinnabar](https://bedrock-mc.github.io/cinnabar/) previews and edits pack UI
exactly as Cinnabar renders it; open your own vanilla or server pack, nothing is bundled or
uploaded. Paste into the empty editor to start a scratch file; the Export tab packages edits as
`.mcpack`, `.zip` or `.mcaddon`, by default an overlay of only the changed controls.
`make jsonui-editor` builds it locally. For AI agents, `cargo build -p jsonui-mcp` gives a
stdio MCP server:

```json
{ "mcpServers": { "jsonui": {
  "command": "/path/to/cinnabar/target/debug/jsonui-mcp",
  "args": ["--font", "/path/to/cinnabar/.local/assets/compiled/ui-cinnangles-sans-v1.mcbefont"]
} } }
```

## Development

Work lands through pull requests into `dev`, whose CI runs the full matrix; `main` is the release
line. Before pushing, check only what your change affects:

```sh
cargo run -p devtool --locked -- verify-affected --base origin/dev
```

It runs fmt, the architecture gate, clippy and tests for the affected crates, `go test` and
`go vet` for changed Go modules, and the packaging tests when `packaging/` changes. Contributor and
agent rules live in `AGENTS.md` and `docs/agents/`.
