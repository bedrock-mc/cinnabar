# Workspace layout

How the client, the Go core and their libraries fit together, and what each crate and package owns. The crate layering rules are in `tools/architecture/policy.toml`.

## Overview

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

## Crates and packages

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
| `localworld` | Local worlds on BDS (a container on macOS); `proxy` hosts open ones for Xbox friends. |
| `packcache` | On-disk cache of server packs. |
| `update` | Signed update checks. |

Cross-crate re-exports are forbidden: consumers import the owning crate directly.
Named exports must also have unambiguous ownership across Rust namespaces. Use
explicit dependency imports where external globs could supply an exported name.
The architecture gate also rejects glob re-exports with restricted visibility and
executable file headers regardless of extension. Files marked `binary` in Git
need a named ownership record; executable files cannot use that exemption.
Temporary, individually named protocol and session forwarding APIs are tracked
in `tools/architecture/policy.toml` for removal with #532.
