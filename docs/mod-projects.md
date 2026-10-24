# Paired mod reference

Start with [the first-hour guide](mods/README.md). Full code lives in
[`examples/mods/cosmic`](../examples/mods/cosmic).

## Entry points

| Source | Contract |
| --- | --- |
| `build.rs` | Calls `experience_sdk::project::generate()` |
| `src/lib.rs` | Enables one side and includes `experience_sdk::include_shared!()` |
| `src/server.rs` | `#[experience_sdk::entry::server]` on `impl Experience`; generates registration and registry replies |
| `src/client.rs` | `#[experience_sdk::entry::client]` on `impl ClientPart`; exports client callbacks |
| `src/shared.rs` | Generator input: project metadata, imports, typed ID constants and message structs |

Build with exactly one of the `server` and `client` features, with defaults disabled.
Each forwards to its SDK feature. The SDK dependency also needs `messages`; the
build dependency needs `project`. Keep private logic in the server module.

Return `Some(&shared::REGISTRY)` from `ClientPart::registry`. Authored initialization
and callbacks wait for the server's matching fingerprint; a mismatch stops the
part and asks for a rebuild. Missing confirmation leaves it waiting. A recovered
guest negotiates again. Fields, directions, revisions and typed IDs affect the digest.

## Declarations

From Cosmic's `src/shared.rs`:

```rust
#![project(id = "cosmic", permissions(messaging, ui))]

use experience_sdk::{id, message::{BlockId, ItemId}};

#[block(name = "Moon rock", texture = assets::ASSET_MOON_PNG, hardness = 2.0)]
pub const MOON_ROCK: BlockId = id!("cosmic:moon_rock");

#[item(name = "Moon key", icon = assets::ASSET_MOON_PNG, max_stack = 1)]
pub const MOON_KEY: ItemId = id!("cosmic:moon_key");
```

Omit hardness for an unbreakable cube. Item stacks default to 64. Textures and icons
must name generated shared PNG constants. The builder runs server registration and
validates it before publishing. No handwritten Experience TOML or pack JSON is needed.
`EntityId` and `WorldId` name definitions; they do not register or create instances.

## Messages

```rust
#[message(id = "cosmic.ping", schema = 1, to_server)]
pub struct Ping {
    pub request: u32,
}
```

Use `to_client` for replies. The generator produces signed channels, Rust `Message`
implementations and Go bindings from the same ordered fields.

| Field | Required bounds |
| --- | --- |
| `bool`, `i8/i16/i32/i64`, `u8/u16/u32` | Optional integer `min`/`max`, within the Rust type |
| `String`, `BlockId`, `ItemId`, `EntityId`, `WorldId` | `#[field(max_bytes = N)]`, counting UTF-8 bytes |
| `Vec<T>` of a supported scalar | `#[field(max_items = N)]`; strings and IDs also need `max_bytes` |

Unsupported types and conditional definitions fail generation. Import
`experience_sdk::message::Message` to call `encode`/`decode` and read `CHANNEL`.
The codecs check field counts, types and bounds. Client code sends with
`client::send_message`; server code sends encoded values through `server::nodes`
to `Callback::send_client` or `send_control`. The host also checks the signed channel.
Always validate requests against server state; a matching registry is no player authorization.

## Assets

`assets/sky/stars.bin` generates `shared::assets::ASSET_SKY_STARS_BIN`, a typed `Asset`
with `path` and compile-checked `bytes`. Missing files, ambiguous constant names and
symlinks fail generation or compilation.

| Directory | Included in |
| --- | --- |
| `assets/server/` | Server artifact only; constants unavailable to client builds |
| `assets/client/` | Client bundle only |
| Other `assets/` paths | Both halves; use for block textures and item icons |

`examples/mods/cosmic/assets/moon.png` and the terminal fixture's
`examples/experiences/terminal-client/assets/terminal.png` are original sample art
covered by this repository's license. Game assets are not included.

## Packaging and installation

`cinnabar-cxb mod build --project <dir> [--release]` writes `dist/<id>.cmod`:

| Path | Contents |
| --- | --- |
| `server/` | Server WASM, generated manifest and server/shared assets |
| `client/<id>.cxb` | Signed client bundle |
| `resource.mcpack` | Generated block/item art |
| `registry.json`, `messages/` | Shared definitions and Go bindings |
| `package.json` | SHA-256 inventory |

The first build creates `.local/publisher.seed`; keep it private and stable.
Install with `cinnabar-cxb mod install <file.cmod> --server <world>`. Start
`bedrock-local-server -dir <world> -addr 127.0.0.1:19132`; it discovers installed mods.
Keep `experience-runtime` and `cinnabar-cxb` beside the server, or set
`-experience-runtime` and `CINNABAR_CXB_BIN`. To replace an installation, stop the
server and remove `<world>/mods/<id>` first. Preserve saved block identities.

## Existing Go servers

The generated module is `cinnabar.local/<id>/messages`. Use a local `replace` or
place it under your server module. The generated `go.mod` pins the helper and
compatible Dragonfly version. Import the helper as
`github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext`; regenerate older
bindings and their module together to update the module identity.

Before listening, call `messages.RegisterRegistry(ext)` and check its error.
Use `cinnabarext.OnMessage(ext, messages.PingType, callback)` to receive, and
`ext.Send`/`ext.Broadcast` with generated messages to send. Import the helper at
the path emitted by `messages.go`. Message/epoch hooks receive the originating
`cinnabarext.PlayerSession`; match it to the admitted session before routing input.
Player identity comes from the connection; world access still belongs in
Dragonfly transactions. `EncodeRecord` and
`DecodeRecord` expose canonical wire-v2 records for other transports.

`SendControl` retains a refused negotiation/acknowledgment reply until credit
refills; newer replies on the route replace it. Epoch changes and disconnects
clear pending replies. Check its error: `BurstExceeded` means it cannot fit the
maximum burst and was not retained. Ordinary `Send` and `Broadcast` are best effort.
