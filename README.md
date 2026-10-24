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
[![Website](https://img.shields.io/badge/Website-cinnabar.gg-B22222)](https://cinnabar.gg/)

<img width="1282" height="752" alt="Cinnabar in game" src="https://github.com/user-attachments/assets/ca040799-e00e-4a6e-85f8-a0e28af6ea72" />

| | |
| --- | --- |
| <img alt="Sunset with a shader pack" src="https://github.com/user-attachments/assets/25a2e8fa-23d8-4aeb-8e80-aca9d09dcee3" /> | <img alt="Moonlit river with a shader pack" src="https://github.com/user-attachments/assets/c0a8cdd8-03f2-4188-ae3c-36f516260060" /> |
| <img alt="Light shafts in a cave with a shader pack" src="https://github.com/user-attachments/assets/1622104f-7ea2-4dde-a433-219a5f09103b" /> | <img alt="PvP on iOS" src="https://github.com/user-attachments/assets/b6135da0-ff6a-4b75-ade0-56439be56ca0" /> |

## Download

- **Stable:** [latest release](https://github.com/cinnabar-mc/cinnabar/releases/latest)
- **Nightly:** daily builds of `dev` for macOS, Windows and Linux ([nightly](https://github.com/cinnabar-mc/cinnabar/releases/tag/nightly))

On first launch, Cinnabar downloads the vanilla resource pack after you accept the Minecraft EULA.
No Mojang assets are bundled. The release notes explain how to open unsigned builds.

## Build and run

```sh
make play
```

This fetches and compiles the vanilla assets and opens the Rust client launcher. Sign-in
uses a Microsoft device code. Credentials remain in the private per-user account store; never
share or commit that store. `make play PROFILE=release` builds the fully optimised binary,
and `make help` lists every target.

On Debian/Ubuntu, install `libwayland-dev` and `libudev-dev` first.

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

The Rust client owns rendering, input and gameplay. Desktop builds use native Rust account and
network services directly, with addressed discovery selecting NetherNet or RakNet. A build
without the `native-networking` feature cannot open native sessions. The target game version
is pinned in `assets/bedrock-target.json`.

Launch the native menu, then sign in or select a saved account and join a server:

```sh
cargo run -p cinnabar
```

Menu account and local-world services run without a separate networking process or control endpoint.
Native mode uses the existing account menu for Microsoft device-code sign-in and saved-account
validation. Pending sign-in leaves the active account unchanged until identity verification and
account commit finish. Account changes, cancellation and expired credentials stop the original
join; they do not trigger a signed-out retry. Refresh rotation and private derived-service
checkpoints share the existing account-cache leases. Native sign-in can present the Xbox profile
setup page and resume the same pending sign-in.

Native account services include Realms and invitations, Xbox people and profiles, presence,
featured-server discovery and world signaling. Realms can resolve RakNet or authenticated
NetherNet. The session retains the selected account and connection key through Login, resource
packs, startup and play, including retries and transfers.

Direct startup also uses the selected saved account when one is available:

```sh
cargo run -p cinnabar -- --address HOST:PORT
```

Without a selected account, direct connections can use an offline identity where the server
permits it. `nethernet-lan` selects directed IPv4 LAN discovery. Selected local worlds keep their
original HTTP or LAN route and process generation. Desktop pack acquisition supports CDN
archives and a verified persistent cache before falling back to game-socket chunks.

Use `--native-networking raknet` to bypass addressed transport discovery, or
`--native-networking nethernet-lan` for directed LAN discovery. The client has no Go
account/network backend or socket-directory mode. The independent Dragonfly local-world
server remains a Go integration; official BDS worlds use the Rust process owner.

Friend-world admission and hosting use current opaque account selections, including legacy
and Player Messaging signaling, signed peer admission and original Dragonfly or BDS worlds.
Controlled initialization, live traffic and retirement pass; live retail friend joining and
advertised hosting remain under validation. A raw `friend_xuid/<xuid>` string supplies no
verified world selection and is rejected. Android includes native networking and platform
verification; physical-device acceptance remains open. The native HTTPS/WebSocket relay has
a separate operator; see [web/gateway/README.md](web/gateway/README.md) for configuration and
current transport limits.

The transport protocol cores are sans I/O. Runtime adapters own their sockets and timers;
Tokio is an explicit adapter dependency rather than a requirement of the cores. The NetherNet
repository also provides a Bevy adapter for caller-driven outbound sessions. Protocol/session,
service and client-integration responsibilities stay in their owning libraries.

Built on these libraries:

| Library | Used for |
| --- | --- |
| [bedrock-net](https://github.com/cinnabar-mc/bedrock-net) | Native framing, compression, encryption, Login, resource packs and typed session lifecycle. |
| [bedrock-services](https://github.com/cinnabar-mc/bedrock-services) | Native Microsoft/Xbox authentication, Minecraft credentials, Realms, profiles, presence and signaling. |
| [raknet](https://github.com/cinnabar-mc/raknet) | Native RakNet protocol core and runtime adapter. |
| [nethernet](https://github.com/cinnabar-mc/nethernet) | Sans-I/O ICE, DTLS and SCTP with explicit Tokio and Bevy adapters. |
| [protocolgen](https://github.com/cinnabar-mc/protocolgen) | Owns the pinned generated Bedrock packet codec consumed as a Git dependency. |
| [gophertunnel](https://github.com/Sandertv/gophertunnel) | Bedrock server sessions in the independent local-world server and Go fixture tools. |
| [go-raknet](https://github.com/Sandertv/go-raknet) | RakNet in the independent local-world server and Go fixture tools. |
| [dragonfly](https://github.com/df-mc/dragonfly) | The built-in local-world server in `tools/localserver`. |

The packet-entry and startup code in `crates/protocol` was adapted from Jolyne
([Axolotl Stack](https://github.com/axolotl-stack/axolotl-stack)); see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

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
