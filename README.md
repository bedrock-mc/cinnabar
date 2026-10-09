# Cinnabar

> An independent, unofficial client compatible with Minecraft: Bedrock Edition. Not approved by
> or associated with Mojang or Microsoft. Minecraft is a trademark of Microsoft Corporation.

A Minecraft: Bedrock Edition client written from scratch in Rust. It plays on real servers, Realms
and friends' worlds with vanilla behaviour, and it's built to run much faster than the official
client.

[![Discord](https://img.shields.io/badge/Discord-Join%20us-5865F2?logo=discord&logoColor=white)](https://discord.gg/MeEz7BEHcM)
[![Website](https://img.shields.io/badge/Website-cinnabar.restartfu.com-B22222)](https://cinnabar.restartfu.com/)

<img width="1282" height="752" alt="Cinnabar in game" src="https://github.com/user-attachments/assets/ca040799-e00e-4a6e-85f8-a0e28af6ea72" />

## Download

- **Stable:** [latest release](https://github.com/bedrock-mc/cinnabar/releases/latest)
- **Nightly:** daily builds of `dev` for macOS, Windows and Linux ([nightly](https://github.com/bedrock-mc/cinnabar/releases/tag/nightly))

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

## What's different

Vanilla behaviour is the default. Everything below is extra:

| Feature | What it does |
| --- | --- |
| **Performance** | Frame pacing, latency and chunk streaming built to beat vanilla. |
| **Live resource packs** | Add, remove or reorder packs without leaving the world. |
| **Multi-account manager** | Sign in with several Microsoft accounts and switch between them from the launcher, no signing out. |
| **Custom skins** | Import custom-geometry (4D/5D) skins and skin packs. See [docs/custom-skins.md](docs/custom-skins.md). |
| **Discord** | Rich presence, plus joining and inviting friends through Discord. See [docs/discord.md](docs/discord.md). |
| **Mods** *(preview)* | Sandboxed WebAssembly mods that hot-reload, for your own client and for servers. Servers can use it as a Roblox-style game engine, shipping client code that replaces the UI, rendering, input and game logic (in-world video included) to turn a server into an entirely different game. See [docs/modding-spike.md](docs/modding-spike.md) and [docs/server-experiences.md](docs/server-experiences.md). |
| **JSON-UI editor** | Preview and edit pack UI exactly as Cinnabar renders it, at [bedrock-mc.github.io/cinnabar](https://bedrock-mc.github.io/cinnabar/). |

## How it works

```text
bedrock-client (Rust, Bevy/wgpu)  ── local socket ──  bedrock-core (Go)
                                                        ├─ RakNet ──── servers
                                                        └─ NetherNet ─ Realms and friend worlds
```

The Rust client owns everything you see and play. The Go core owns Xbox sign-in, encryption and
the network transports. The target game version is pinned in `assets/bedrock-target.json`.

Built on these libraries:

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

Crate-by-crate layout: [docs/architecture/workspace.md](docs/architecture/workspace.md).
Benchmarks: [docs/benchmarks.md](docs/benchmarks.md).

## Contributing

Open pull requests against `dev`, the default and release branch. Before pushing, check only what
your change affects:

```sh
cargo run -p devtool --locked -- verify-affected --base origin/dev
```

Contributor rules are in [AGENTS.md](AGENTS.md) and [docs/agents/](docs/agents/).

## License

[PolyForm Noncommercial 1.0.0](LICENSE). Contact the maintainers for a commercial license.
Third-party code and assets keep their own licenses; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
