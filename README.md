# Cinnabar

> An independent, unofficial client compatible with Minecraft: Bedrock Edition. Not approved by
> or associated with Mojang or Microsoft. Minecraft is a trademark of Microsoft Corporation.

> [!WARNING]
> Cinnabar is a non-vanilla client and still in development. Its movement and combat may not yet
> match the official client exactly, so server anticheats may flag you and some servers may ban
> you. Play on servers at your own risk.

A Minecraft: Bedrock Edition client written from scratch in Rust. It plays on real servers, Realms
and friends' worlds with vanilla behaviour, and it's built to run much faster than the official
client.

[![Discord](https://img.shields.io/badge/Discord-Join%20us-5865F2?logo=discord&logoColor=white)](https://discord.gg/MeEz7BEHcM)
[![Website](https://img.shields.io/badge/Website-cinnabar.restartfu.com-B22222)](https://cinnabar.restartfu.com/)

<img width="1282" height="752" alt="Cinnabar in game" src="https://github.com/user-attachments/assets/ca040799-e00e-4a6e-85f8-a0e28af6ea72" />

## Download

- **Stable:** [latest release](https://github.com/cinnabar-mc/cinnabar/releases/latest)
- **Nightly:** daily builds of `dev` for macOS, Windows and Linux ([nightly](https://github.com/cinnabar-mc/cinnabar/releases/tag/nightly))

On first launch, Cinnabar downloads the vanilla resource pack after you accept the Minecraft EULA.
No Mojang assets are bundled. The release notes explain how to open unsigned builds.

## Build and run

```sh
make play
```

This fetches and compiles the vanilla assets, builds the Go core, and opens the launcher. Sign-in
uses a Microsoft device code. The token is cached in `.local/auth/`, so never share or commit
that folder. `make play PROFILE=release` builds the fully optimised binary, and `make help` lists
every target.

On Debian/Ubuntu, install `libwayland-dev` and `libudev-dev` first.

Experimental Java (OptiFine/Iris) shader packs build with `cargo run -p cinnabar --features shaderpacks`.
Put pack zips or folders in the `shaderpacks` folder of the client's data directory and pick one
under Settings > Video > Renderer, or run a pack for one session with `--shader-pack <path>`.
A pack that fails to load draws Vanilla.

## What's different

Vanilla behaviour is the default. Everything below is extra:

| Feature | What it does |
| --- | --- |
| **Performance** | Frame pacing, latency and chunk streaming built to beat vanilla. |
| **1.12 PvP feel** | Ships with Bedrock 1.12-style combat visuals: the classic attack swing, hurt flash, enchantment glint and view bob, plus Java-style first-person hand sway and nametags. |
| **Live resource packs** | Add, remove or reorder packs without leaving the world. |
| **Multi-account manager** | Sign in with several Microsoft accounts and switch between them from the launcher, no signing out. |
| **Custom skins** | Import custom-geometry (4D/5D) skins and skin packs. See [docs/custom-skins.md](docs/custom-skins.md). |
| **Discord** | Rich presence, plus joining and inviting friends through Discord. See [docs/discord.md](docs/discord.md). |
| **Cinnabar players** | A badge on other Cinnabar players in the tab list, and update notices as soon as a release ships. See [docs/cinnabar-api.md](docs/cinnabar-api.md). |
| **Mods** *(preview)* | Sandboxed WebAssembly mods that hot-reload, for your own client and for servers. Servers can use it as a Roblox-style game engine, shipping client code that replaces the UI, rendering, input and game logic (in-world video included) to turn a server into an entirely different game. See [docs/modding-spike.md](docs/modding-spike.md) and [docs/server-experiences.md](docs/server-experiences.md). |
| **JSON-UI editor** | Preview and edit pack UI exactly as Cinnabar renders it, at [jsonui.cinnabar.gg](https://jsonui.cinnabar.gg/). |
| **Figma to JSON-UI** | A Figma plugin that turns a frame into a JSON-UI screen and resource pack. [Download it](https://jsonui.cinnabar.gg/figma-plugin.zip), or see [tools/figma-plugin](tools/figma-plugin/README.md). |

## How it works

```text
cinnabar (Rust, Bevy/wgpu)  ── local socket ──  bedrock-core (Go)
                                                        ├─ RakNet ──── servers
                                                        └─ NetherNet ─ Realms and friend worlds
```

The Rust client owns everything you see and play. The Go core owns Xbox sign-in, encryption and
the network transports. The target game version is pinned in `assets/bedrock-target.json`.

Built on these libraries:

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

Crate-by-crate layout: [docs/architecture/workspace.md](docs/architecture/workspace.md).
Benchmarks: [docs/benchmarks.md](docs/benchmarks.md).
Roadmap: [plan.md](plan.md). Change history: [CHANGELOG.md](CHANGELOG.md).

## Contributing

Open pull requests against `dev`, the default and release branch. Before pushing, check only what
your change affects:

```sh
cargo run -p devtool --locked -- verify-affected --base origin/dev
```

Contributor rules are in [AGENTS.md](AGENTS.md) and [docs/agents/](docs/agents/).

## Star history

[![Star history chart](https://api.star-history.com/svg?repos=cinnabar-mc/cinnabar&type=Date)](https://star-history.com/#cinnabar-mc/cinnabar&Date)

## License

[PolyForm Noncommercial 1.0.0](LICENSE). Contact the maintainers for a commercial license.
Third-party code and assets keep their own licenses; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
