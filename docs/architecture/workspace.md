# Workspace layout

How the client, the Go core and their libraries fit together, and what each crate and package owns. The crate layering rules are in `tools/architecture/policy.toml`.

## Overview

```text
cinnabar (Rust)  ── local socket ──  bedrock-core (Go, gophertunnel)
                                             ├─ go-raknet ──── servers and BDS
                                             └─ go-nethernet ─ Realms and friend worlds
```

Rust never implements Xbox authentication, encryption, RakNet or NetherNet; the core owns those
and relays packets over a local stream.

| Library | Used for |
| --- | --- |
| [protocolgen](https://github.com/cinnabar-mc/protocolgen) | Generates the Bedrock packet codec vendored in `crates/protocol/vendor`. |
| [Axolotl Stack](https://github.com/axolotl-stack/axolotl-stack) | Jolyne (client session library), whose raw packet entries and spawn sequence `crates/protocol` adapts. MIT; see `THIRD_PARTY_NOTICES.md`. |
| [gophertunnel](https://github.com/Sandertv/gophertunnel) | Bedrock login, encryption, resource packs and the packet relay. |
| [go-raknet](https://github.com/Sandertv/go-raknet) | RakNet transport to servers, plus server-list pings. |
| [go-nethernet](https://github.com/df-mc/go-nethernet) | WebRTC transport for Realms and friend worlds. |
| [go-xsapi](https://github.com/df-mc/go-xsapi) | Xbox Live identity, friends, presence and signaling. |
| [go-playfab](https://github.com/df-mc/go-playfab) | PlayFab sign-in and the menu catalog (featured servers). |
| [dragonfly](https://github.com/df-mc/dragonfly) | The built-in local-world server in `tools/localserver`. |

Mojang assets are never committed or embedded. `make assets` fetches Mojang's official
`bedrock-samples` pack (EULA-gated) and compiles it into carriers under the ignored `.local/`.

## Crate ownership

Every workspace member has one row below. The first column is its directory; the second is
its Cargo package name, which can differ from the directory. Import APIs from their owning
crate directly. The architecture gate checks these rows against `tools/architecture/policy.toml`,
which also requires every workspace member to be registered. Package names are checked against
their Cargo manifests. Each entry needs its package name, caller guidance and related owners.
Keep rows current when adding, moving or removing a crate.

### Client composition and adapters

Domain crates below hold the game rules and state without Bevy. The `client-*` adapters bind
those owners to the client frame and session boundaries. `client-state` holds shared ECS
contracts; systems live in the adapter that owns their work. `app` assembles the plugins and
coordinates work that crosses these owners.

| Workspace member | Package | Ownership and caller guidance | Related owners |
| --- | --- | --- | --- |
| `app` | `cinnabar` | Client executable and Bevy composition; use it to launch the client and assemble its plugins. | `client-*` crates own subsystem adapters; domain crates own game rules. |
| `web/client` | `web-client` | Browser page control, delivery admission and physical input translation. | `assets` owns carrier validation; `web-transport` owns relay sessions; `semantic-input` owns device frames; `cinnabar` owns application startup. |
| `web/transport` | `web-transport` | Browser WebTransport and WebSocket channels, bounded control and reconnect state; the browser host drives timers and passes prepared play I/O to the existing session. | `bridge` owns framing and the shared relay contract. |
| `packaging/android/client` | `cinnabar-android` | Android NativeActivity entry and setup-activity natives; build it with `packaging/android/build-apk.sh`. | `cinnabar` runs the client; `launcher` owns the Android install layout; `first-run` prepares carriers. |
| `crates/client-state` | `client-state` | Shared ECS resource wrappers, frame sets and observations; use these contracts to connect plugins. | Domain crates own transitions; the other `client-*` crates own systems. |
| `crates/client-runtime` | `client-runtime` | Transport resources, world servicing and session lifecycle adapters; use its runtime plugin and ingress/publication APIs. | `client-session` owns transport; `client-world` and `chunk-pipeline` own world work. |
| `crates/client-session` | `client-session` | Session transport, join bootstrap and separate bounded control/world FIFOs; use its network handle and sequenced events. | `pack-runtime` prepares packs; presentation owners interpret the opaque prepared payload. |
| `crates/client-shell` | `client-shell` | Menu input, launcher services and desktop settings; install its shell plugin. | `launcher-host` owns services; `launcher` owns models; screen crates draw the menus. |
| `crates/client-interaction` | `client-interaction` | Input-to-movement, camera and interaction adapters; install its interaction plugin. | `gameplay`, `sim` and `inventory` own decisions; `client-presentation` owns visual results. |
| `crates/client-ui` | `client-ui` | Retained client UI state, input routing and frame publication; call its UI runtime from client adapters. | `ui-screens`, `oreui` and `mod-ui` draw screen families; `ui-engine` retains their nodes. |
| `crates/client-presentation` | `client-presentation` | Camera, actor, equipment and environment frame preparation; use its plugin, environment state and borrowed observation inputs. | `actor-animation` evaluates poses; `render-model` defines CPU data; `render` submits it. |
| `crates/client-audio` | `client-audio` | Sound admission, playback and local audio cues; install its audio plugin. | `assets` supplies sound data; interaction and session adapters publish observations. |
| `crates/client-extensions` | `client-extensions` | Local mod and server Experience adapters, including consent input; install its extension plugin. | `mod-host` executes guests; `server-experience` negotiates them; `mod-ui` draws their surfaces. |

### Game state and input

| Workspace member | Package | Ownership and caller guidance | Related owners |
| --- | --- | --- | --- |
| `crates/world` | `world` | Palette-native chunks and world storage; use its block, chunk and coordinate types. | `client-world` admits network state; `meshing` produces geometry. |
| `crates/client-world` | `client-world` | Authoritative world, actor and local-player facts, decoding and ordered commits; use its ingestion and committed views. | `world` owns storage; `inventory` admits numeric session item facts; `chunk-pipeline` schedules terrain; `player-state` joins player authorities. |
| `crates/chunk-pipeline` | `chunk-pipeline` | Bounded terrain residency, worker scheduling and publication; use its world stream and publication snapshots. | `client-world` owns admitted state; `render-api` owns shared request bounds and receipts; the app encodes outbound requests through `protocol`; `meshing` builds geometry; `render` consumes publications. |
| `crates/far-terrain` | `far-terrain` | Far-terrain level of detail from received terrain: per-column surface summaries, their caches and bounds; use its summaries and config. | `chunk-pipeline` extracts summaries on its workers; `render` draws the far pass. |
| `crates/inventory` | `inventory` | Inventory authority, prediction, crafting and commands; use its session, ledger and router APIs. | `player-state` shares it with gameplay and UI; `protocol` defines wire requests. |
| `crates/player-state` | `player-state` | Shared inventory and local-player authority; use `PlayerState` to keep both on one session generation. | `inventory` owns stacks and crafting; `client-world` owns player facts. |
| `crates/gameplay` | `gameplay` | Local interaction decisions, prediction and ordered admission; use its mining, placement, item-use and movement APIs. | `sim` owns physics; `client-interaction` binds decisions to ECS; `client-session` sends packets. |
| `crates/sim` | `sim` | Deterministic movement, collision and prediction; use its simulator with caller-supplied world queries. | `gameplay` chooses actions; `client-interaction` drives ticks; `world` supplies block state. |
| `crates/input` | `semantic-input` | Device-independent actions, bindings and finalized input; use semantic snapshots instead of raw device events. | `client-shell` reads devices; `client-interaction` consumes actions; UI owns modal input routing. |
| `crates/env-compat` | `env-compat` | Reads `CINNABAR_*` environment variables, accepting their pre-rename `RUST_MCBE_*` spelling with a one-time warning; use its `var` and `var_os` for those variables. | Each variable's name stays a constant with its owner, such as `diagnostics::markers`. |
| `crates/simulation-clock` | `simulation-clock` | Shared simulation tick values and duration conversion; use its tick constants and conversion function. | Callers supply elapsed time; world and gameplay owners advance their own state. |
| `crates/block-transform` | `block-transform` | Position-dependent block offsets and rotations; use its transforms for consistent geometry and queries. | `assets` describes blocks; `meshing` and `sim` consume their transformed shapes. |
| `crates/actor-animation` | `actor-animation` | Actor rigs, Molang evaluation, controllers and poses; use its animation evaluation over actor observations. | `client-world` owns actor authority; `client-presentation` prepares frames. |
| `crates/particles` | `particles` | Particle definitions, emitters, collision and draw lists; use its simulation and effect library. | `assets` supplies pack definitions; client adapters trigger effects; `render` draws them. |

### Protocol and packs

| Workspace member | Package | Ownership and caller guidance | Related owners |
| --- | --- | --- | --- |
| `crates/bridge` | `bridge` | Framing and I/O for the local stream to the Go core; use its bounded bridge messages. | The Go core owns upstream transports; `protocol` interprets Bedrock packets. |
| `crates/protocol` | `protocol` | Bedrock packet codecs, shared wire contracts and session adapters; use its packet and codec APIs. | `bridge` owns local framing; `client-session` owns client transport; the codec lives in its vendored tree. |
| `crates/protocol-ids` | `protocol-ids` | Dependency-free named wire IDs and pinned air identities; import its constants and enums directly. | `protocol` and `assets` share these facts; the editor can consume them on WebAssembly. |
| `crates/pack-json` | `pack-json` | Shared pack text decoding and JSONC parsing; use its decoder before reading authored pack documents. | Pack admission, compilers and UI readers share this format policy. |
| `crates/zip-footer` | `zip-footer` | ZIP end-record admission for in-memory archives; run `prepare_zip_bytes` before handing bytes to a ZIP parser. | `resource-pack` and `shaderpack` share this archive policy. |
| `crates/resource-pack` | `resource-pack` | Bounded archive admission, decryption and pack-stack views; use its read-only admitted packs. | The Go core downloads packs; `pack-runtime` prepares them; the pack compiler owners compile them. |
| `crates/shaderpack` | `shaderpack` | Java Edition OptiFine/Iris shader-pack loading from folders, ZIP archives and memory; use `PackSource` for sorted text and bounded asset reads. See its [loading contract](../../crates/shaderpack/README.md). | `resource-pack` owns Bedrock packs; shader configuration, translation and render-host integration remain unimplemented. |
| `crates/assets` | `assets` | Pack source readers, compiled carrier formats and the pinned item- and entity-facts owner; use bounded loaders and typed asset data. | The pack compiler owners write carriers; `protocol-ids` supplies query metadata IDs; `pack-runtime` prepares active session stacks. |
| `crates/pack-compiler` | `pack-compiler` | Terrain, materials and independent carriers such as atmosphere, audio and UI; import its compiler APIs directly. | `pack-compile-input` owns shared inputs; `assets` owns formats. |
| `crates/pack-compiler/input` | `pack-compile-input` | Bounded pack sources, source validation, JSON and texture decoding; use its shared input APIs. | Terrain and entity compilers consume these inputs; `assets` owns formats. |
| `crates/pack-compiler/entity` | `pack-entity-compiler` | Entity, actor, equipment, icons, starter skins and block-entity artwork; import its compilation APIs directly. | `pack-compile-input` owns shared inputs; `pack-runtime` prepares session packs. |
| `crates/pack-compiler/font` | `pack-font-compiler` | Bitmap and outline font compilation, including native rasterization; use its font APIs from offline tools. | `assets` owns font carrier formats; `asset-compiler` owns the CLI. |
| `crates/asset-compiler` | `asset-compiler` | The `assetc` executable for runtime carrier preparation; use its CLI from asset setup. | the pack compiler owners implement compilation; `protocol-ids` validates generated event routes; `first-run` coordinates initial preparation. |
| `crates/pack-runtime` | `pack-runtime` | Runtime pack admission, compilation cache and reload dependencies; use its pack preparation and reload APIs. | `resource-pack` admits archives; `pack-entity-compiler` compiles entities; client adapters install the results. |

`assets` owns `crates/assets/data/item-facts-v2193.bin`, generated by `registryc` from
the pinned item tables and typed rules. It contains identifiers and behavior metadata,
with a checked schema and hash, rather than artwork or executable code. Regenerate it
through the item-facts compiler; runtime pack carriers remain local inputs.

`assets` also owns `crates/assets/data/entity-facts-v2193.bin`, generated by `registryc`
from pinned entity component metadata and typed compatibility rules. It contains
identifiers and constructor facts with a checked schema and hash. Runtime artwork
and pack carriers remain local inputs.

### UI layers

`ui` provides shared UI values and draw primitives. `json-ui` parses, resolves, binds and lays
out pack JSON, then emits draw commands. `ui-engine` adds screen registration, cached layout,
text and texture lookup, and retained nodes around that pipeline. These layers do not own
client session authority.

`ui-screens` draws vanilla JSON-UI screen families from borrowed inputs. `oreui` implements
the OreUI theme and screens in our own code using shared UI primitives. `mod-ui` draws mod
and Experience surfaces through `ui-engine`, including trusted consent chrome. `client-ui`
projects runtime state into those screen inputs, routes actions and publishes the retained UI
frame. The client adapters apply resulting commands to the domain owners; `render` submits
the prepared visual data to the GPU.

| Workspace member | Package | Ownership and caller guidance | Related owners |
| --- | --- | --- | --- |
| `crates/ui` | `ui` | Renderer-independent UI values, actions, primitives and text layout; use its models and draw lists. | `json-ui` handles pack syntax; screen crates compose controls; `render` draws outputs. |
| `crates/json-ui` | `json-ui` | JSON-UI parsing, inheritance, binding, layout and draw-command emission; use its catalog and control pipeline. | `ui-engine` adds retained screens and resources; `ui-screens` supplies screen-specific inputs. |
| `crates/ui-engine` | `ui-engine` | Portable screen engine, cached layout, painting and texture lookup; register screens and provide resource sources here. | `json-ui` resolves controls; `ui` owns primitives; screen crates own screen behavior. |
| `crates/ui-screens` | `ui-screens` | Vanilla JSON-UI screen families, HUD renderers and menu bindings; draw them with typed borrowed inputs. | `ui-engine` paints; `client-ui` owns runtime state; domain crates apply commands. |
| `crates/oreui` | `oreui` | OreUI theme, controls and screens drawn in our own code; use its screen and artwork APIs. | `ui` supplies primitives; `launcher` supplies menu models; `client-ui` routes runtime actions. |
| `crates/mod-ui` | `mod-ui` | Mod and Experience presentation, consent chrome, panels and HUD editor; draw surfaces from borrowed host inputs. | `ui-engine` paints; `mod-host` owns guest execution; `client-extensions` owns runtime integration. |

### Rendering contracts and backend

| Workspace member | Package | Ownership and caller guidance | Related owners |
| --- | --- | --- | --- |
| `crates/meshing` | `meshing` | CPU chunk, fluid, biome and cloud geometry; use its mesh builders over world and asset inputs. | `chunk-pipeline` schedules terrain work; `render` uploads the results. |
| `crates/render-api` | `render-api` | Dependency-free publication, pacing and skin-layout contracts; use these values across publication boundaries. | `render-model` owns richer CPU geometry; `render-scene` owns ECS submission resources. |
| `crates/cosmetics` | `cosmetics` | Bounded catalog metadata, admitted item handles and slot loadouts; resolve catalog IDs here before rendering. | `launcher-host` fetches verified assets; `render-model` bakes attachment geometry; the API owns entitlements. |
| `crates/render-model` | `render-model` | Engine-free render data and CPU geometry for actors, equipment and text; use its frame and geometry types. | `client-presentation` prepares data; `render-scene` carries submissions; `render` owns GPU work. |
| `crates/render-scene` | `render-scene` | Shared ECS render submission resources and frame acknowledgments; use these at the presentation/backend boundary. | `render-model` defines CPU data; `client-presentation` publishes; `render` consumes it. |
| `crates/render` | `render` | Bevy/wgpu terrain, actor and UI rendering; install its GPU plugins and submit prepared frames. | `meshing` builds CPU terrain; `client-presentation` prepares actors; `client-ui` prepares UI. |

### Launcher and services

| Workspace member | Package | Ownership and caller guidance | Related owners |
| --- | --- | --- | --- |
| `crates/launcher` | `launcher` | Bevy-free account, menu and world models plus product identity; use these models across services and screens. | `json-ui` owns vanilla screen names; `launcher-host` owns process and persistence services; `client-shell` integrates them. |
| `crates/launcher-host` | `launcher-host` | Launcher processes, platform services, persistence and background work; use its service APIs without Bevy. | `launcher` owns models; `resource-pack` validates user pack imports; the Go core owns authentication and upstream services; `client-shell` owns ECS adapters. |
| `crates/first-run` | `first-run` | Consent, download and preparation of local asset carriers, with the setup window; use its setup runner before launching the game window. | `asset-compiler` builds carriers; `bootstrap-ui` draws the setup window; `launcher-host` owns ordinary launcher services. |
| `crates/bootstrap-ui` | `bootstrap-ui` | The shared pre-game window stack for the setup and crash windows: panorama presenter, CPU-drawn OreUI canvas and controls, focus input, preferences, window attributes and the UI font lookup; build Bevy-free windows on it. | `first-run` owns the setup window; `crash-monitor` owns the crash window; `ui` owns the OreUI theme; `render-model` owns the panorama shader. |
| `crates/crash-monitor` | `crash-monitor` | The `--crash-monitor` process: minidump server watching the client, crash report folders, the crash window and native-dialog fallback; run it beside the client and show reports through its window API. | `crash-report` owns the report format; `launcher-host` installs in-process crash handlers; `bootstrap-ui` draws the window. |
| `crates/rich-presence` | `rich-presence` | Optional Discord desktop activity and RPC worker; use its presence and join-request APIs. | `client-shell` projects menu/session state; `launcher` owns product identity. |
| `crates/update-manifest` | `update-manifest` | Signed release manifest format, channels, verification and build comparison; use its manifest APIs for update decisions. | `release-manifest` signs releases; `launcher-host` downloads them; `update-installer` verifies staged payloads. |
| `crates/update-installer` | `update-installer` | Resumable verified files and the offline native apply helper; use its staging API for downloads and its executable after client exit. | `update-manifest` owns trust and ordering; `launcher-host` owns consent and helper handoff. |

### Extensions and examples

Extensions are disabled by default and do not change vanilla packet IDs or login. Guests use
the SDK for their own host contract; they do not import client runtime authority.

| Workspace member | Package | Ownership and caller guidance | Related owners |
| --- | --- | --- | --- |
| `crates/server-experience` | `server-experience` | Experience negotiation, bundles, cache and trust policy; use its verified session and bundle APIs. | `experience-runtime` runs the server guest; `client-extensions` hosts the client part. |
| `crates/experience-sdk` | `experience-sdk` | Server and client Experience guest SDKs and declaration generation; use the feature for the guest being built. | `experience-runtime` hosts server guests; `mod-host` hosts client components. |
| `crates/experience-runtime` | `experience-runtime` | One server Experience per process over framed stdin/stdout; use its runner and callback protocol. | `experience-sdk` defines guest bindings; `tools/localserver` provides server-side world callbacks. |
| `crates/mod-api` | `mod-api` | Personal-mod guest SDK generated from the extension WIT contract; use its bindings and `PlayerMod` API. | `mod-host` implements imports; Experience client parts use `experience-sdk` instead. |
| `crates/mod-host` | `mod-host` | Bounded component execution and explicit guest imports; use its host, grants and validated outputs. | `client-extensions` integrates guests; `mod-render` and `mod-ui` prepare their visual outputs. |
| `crates/mod-render` | `mod-render` | Validation of guest post passes and world primitives; use its validated geometry and shader outputs. | `mod-api` declares limits; `mod-host` accepts outputs; `render` submits them. |
| `examples/experiences/probe` | `experience-probe` | Server guest fixture for callbacks, world access and failure cases; build it for runtime contract tests. | `experience-sdk` supplies bindings; `experience-runtime` runs it. |
| `examples/experiences/terminal-client` | `experience-terminal-client` | Client guest fixture for terminal UI, dispatch and fuel failures; build it for host tests. | `experience-sdk` supplies client bindings; `mod-host` runs it. |
| `examples/mods/hello` | `hello-mod` | Minimal retained HUD label and keybind example; use it as a personal-mod template. | `mod-api` supplies imports; `mod-host` runs it. |
| `examples/mods/time-changer` | `time-changer-mod` | Local visual-time override example; build it to exercise host environment controls. | `mod-api` defines the imports; `client-extensions` applies the visual output. |
| `examples/mods/render-sample` | `render-sample-mod` | Post-pass and world-decal example; build it to exercise guest rendering. | `mod-render` validates outputs; `render` draws them. |
| `examples/mods/screen-probe` | `screen-probe-mod` | Player-mod screen fixture for events, data reads and traps; build it for host contract tests. | `mod-api` defines screens; `mod-host` executes callbacks; `mod-ui` draws them. |

### Diagnostics, tests and tools

| Workspace member | Package | Ownership and caller guidance | Related owners |
| --- | --- | --- | --- |
| `crates/diagnostics` | `diagnostics` | Runtime metrics, the process `tracing` subscriber and log categories, the ordered console writer, bounded per-session log files and shared diagnostic markers; install logging through it before anything logs. | `acceptance` evaluates evidence; subsystem owners produce observations; `app` adds Bevy-only profiler layers. |
| `crates/crash-report` | `crash-report` | Crash report schema, `crashes/<UTC time>-<id>/` folder layout, session log sections and retention; write, read, list and prune reports through it with caller-supplied time. | `launcher-host` installs the crash handlers that write reports; `devtool` bundles them; `diagnostics` owns the session identity. |
| `crates/acceptance` | `acceptance` | Optional acceptance evidence over committed runtime observations; use its control and witness adapters. | `diagnostics` owns marker contracts; world and render owners publish committed observations. |
| `crates/developer-control` | `developer-control` | Authenticated loopback control protocol, client/server and recording primitives; use it to drive developer builds. | `app` applies commands; `cinnabar-mcp` exposes them as tools. |
| `crates/test-time` | `test-time` | Test-only manual clocks and bounded eventual waits; use these instead of sleeping in tests. | Domain owners accept caller time; edge owners read real clocks. |
| `crates/test-alloc` | `test-alloc` | Per-thread allocation request counting for tests, benchmarks and opt-in runtime diagnostics; optional process totals compose with the chosen allocator. | Owning crates define allocation contracts for their own hot paths. |
| `tools/architecture` | `architecture` | Repository gate for boundaries, ownership docs, source limits and declarations; run its `check` command. | `policy.toml` defines rules; `workspace-declarations` shares metadata with `devtool`. |
| `tools/devtool` | `devtool` | Affected verification, CI planning, captures, worktree tools and offline crash symbolication; use its commands for scoped checks and artifacts. | `architecture` enforces boundaries; `workspace-declarations` supplies metadata and path matching; `crash-report` owns the crash folders `symbolicate` reads. |
| `tools/workspace-declarations` | `workspace-declarations` | Shared Cargo metadata keys and repository path matching; use these declarations in gates and CI selection. | `architecture` validates metadata; `devtool` selects affected packages. |
| `tools/dist` | `dist-local` | Distribution staging, layout and hashes; use its CLI to assemble distributable bundles. | `launcher` owns install layout; `release-manifest` signs the published update manifest. |
| `tools/release-manifest` | `release-manifest` | Release key generation and manifest signing; use its CLI in release publishing. | `update-manifest` owns the format and verification; `dist-local` stages artifacts. |
| `tools/cxb` | `cinnabar-cxb` | Experience seeds, bundles, cache seeding and fixtures; use its publisher CLI. | `server-experience` owns formats, domains and limits; server tooling consumes its fixtures. |
| `tools/bedrock-replay` | `bedrock-replay` | Offline replay of saved server packets through a local session endpoint; use its replay CLI. | `bridge` owns framing; `protocol` owns packets; `client-session` consumes the replay. |
| `tools/jsonui-editor` | `jsonui-editor` | Shared JSON-UI editor API and browser frontend; use its editing, layout, rasterization and export APIs. | `ui-engine` owns screen resources; `json-ui` owns resolution; `jsonui-mcp` wraps the API. |
| `tools/jsonui-mcp` | `jsonui-mcp` | Stdio MCP frontend for JSON-UI editing and rendering; use its editor tools. | `jsonui-editor` owns editing operations; `mcp-stdio` owns the wire protocol. |
| `tools/ui-snapshot` | `ui-snapshot` | Named offline UI fixtures and PNG captures; use `--list` and `--state NAME --out PNG`. | `oreui` and `ui-screens` paint screens; `jsonui-editor` supplies the CPU target. |
| `tools/cinnabar-mcp` | `cinnabar-mcp` | Stdio MCP frontend that launches and drives a developer client; use its input, state, capture and recording tools. | `developer-control` owns loopback commands; `mcp-stdio` owns the wire protocol. |
| `tools/mcp-stdio` | `mcp-stdio` | Shared newline-delimited JSON-RPC MCP serving; implement its tool-server trait for a new stdio frontend. | `jsonui-mcp` and `cinnabar-mcp` own their tool definitions. |

| Go package (`core/`) | What it does |
| --- | --- |
| `cmd/bedrock-core` | The core binary; it runs `coreapp` without extensions. |
| `coreapp` | The core process: flags, authentication, the control endpoint, launcher services and the relay. A binary that extends the core calls `coreapp.Main` with its `Options`. |
| `proxy` | Upstream session, resource-pack download and packet relay, with an optional in-process packet `Interceptor`. |
| `authflow`, `authcache` | Microsoft device sign-in and token cache. |
| `catalog`, `launcher`, `control`, `xboxpresence` | Menu data and services: featured servers, Realms, friends and presence. |
| `localworld` | The launcher's open local world as the core routes it; `proxy` hosts it for Xbox friends. |
| `packcache` | On-disk cache of server packs. |

Cross-crate re-exports are forbidden: consumers import the owning crate directly.
Named exports must also have unambiguous ownership across Rust namespaces. Use
explicit dependency imports where external globs could supply an exported name.
The architecture gate also rejects glob re-exports with restricted visibility and
executable file headers regardless of extension. Files marked `binary` in Git
need a named ownership record; executable files cannot use that exemption.
Temporary, individually named protocol and session forwarding APIs are tracked
in `tools/architecture/policy.toml` for removal with #532.
