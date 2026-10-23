# Build a client part and Go messages together

Create one Cargo project and build its package with the repository developer tool:

```sh
cargo run -p devtool -- mod new cosmic
cargo run -p devtool -- mod build --project cosmic
```

Inside the project, `cinnabar-cxb mod build` also works when the publisher tool is
on your path. Install Rust's `wasm32-unknown-unknown` target first. Add `--release`
for an optimized component. The package is `cosmic/dist/cosmic/`:

- `cosmic.cxb`: the signed client part, checked by the client's bundle verifier.
- `messages/`: a generated Go package with typed IDs, messages and codecs.
- `registry.json`: the generated shared definition schema.

The first build creates `.local/publisher.seed`; later builds reuse it. The scaffold
ignores `.local/`, `target/` and `dist/`. Keep the seed private. Only the `.cxb` goes
to clients. Go gameplay code stays on the server. Developer execution policy still applies.

## Shared definitions

Edit `src/shared.rs`. Its annotations are input to the SDK build script, which
generates the module included by `experience_sdk::include_shared!()`:

```rust
#![project(id = "cosmic", permissions(messaging, world_draw))]

use experience_sdk::{id, message::WorldId};

/// A world definition shared with server code.
pub const MOON: WorldId = id!("cosmic:moon");

/// Requests travel; server code decides whether it is allowed.
#[message(id = "cosmic.travel", schema = 1, to_server)]
#[derive(Clone, Debug, PartialEq)]
pub struct Travel {
    #[field(max_bytes = 96)]
    pub world: WorldId,
    pub request: u32,
}
```

Use `to_client` for server updates. The generator produces channel declarations,
Rust `Message` implementations and Go structs from the same ordered fields.
Supported fields are `bool`, `i8/i16/i32/i64`, `u8/u16/u32`, `String`, `EntityId`,
`WorldId`, `ItemId`, and `Vec<T>` of these scalar types. Strings and IDs require
`max_bytes`; lists require `max_items`. Optional integer `min` and `max` bounds
must fit the Rust type. A list of strings also needs `max_bytes` for each item.
Text bounds count UTF-8 bytes. Unsupported types and conditional definitions fail generation.

Keep imports, typed ID constants and message structs in the shared source; put
application functions in other modules. IDs name definitions and do not create live
worlds, entities, items or server handles. Send with
`experience_sdk::client::send_message(&Travel { ... })`. Import
`experience_sdk::message::Message` to decode records with `Travel::decode(record)`.
Both codecs check field counts, types and bounds; the host still validates signed channels.

## Go server integration

Import the generated package into your Go server. Its module path is
`cinnabar.local/<project>/messages`; use a local `replace` directive or move the
generated package under your server module. Rebuild both together when definitions change.
Keep the generated helper-source `replace` directive in the main server module too;
Go does not inherit dependency modules' replacements.

Each message implements the Dragonfly helper's `cinnabarext.Message`: `Route()`
selects its bundle, channel and revision, and `Encode()` returns its native record.
Use `ext.Send` and `ext.Broadcast` directly with these messages. `messages.TravelType`
provides the generated route and decoder for `cinnabarext.OnMessage`. The generated
Go module pins the helper and a compatible upstream Dragonfly version. Import it as
`github.com/hashimthearab/rust-mcbe/tools/localserver/cinnabarext`.

Before listening, register the definition handshake with
`messages.RegisterRegistry(ext)` and handle its returned error. Register application
receivers with `cinnabarext.OnMessage(ext, messages.TravelType, callback)`. Player
identity comes from the authenticated connection; schedule world access through
Dragonfly transactions. The helper owns routing, epochs, sequences and fragmentation.

`EncodeRecord()` and `(*Travel).DecodeRecord([]byte)` expose the same canonical wire
v2 positional payload for other transports. They use the helper's codec. Use the
generated `messages.PackageID` when a transport needs the owning bundle ID.

The scaffold's `ClientPart::registry` returns `shared::REGISTRY`. Initialization
sends a generated registry request, which the existing client sends after Ready.
`RegisterRegistry` answers it with the server's generated fingerprint. Authored
initialization and callbacks wait for that exact fingerprint.
A mismatch refuses initialization with a diagnostic; missing confirmation leaves
it waiting. Duplicate matching confirmations do not initialize twice. Changes to
fields, revisions, directions and typed IDs affect the digest. This check does not
authenticate players or replace server-side validation.

## Assets and personal mods

Put files in `assets/`. Complete paths generate constants: `assets/sky/stars.bin`
becomes `shared::assets::ASSET_SKY_STARS_BIN`. Each typed `Asset` has an indexed
`path` and compile-checked `bytes`. Missing or renamed files remove the old constant,
so stale references fail compilation. Ambiguous constant names and symlinks fail generation.

To move a personal mod into a client part:

1. Move portable drawing and state code into the scaffold's modules.
2. Move setup into `ClientPart::init`, keeping the generated registry confirmation.
3. Replace asset literals with generated constants and local-only inputs with typed messages.
4. Use the canonical drawing interfaces available to client parts, retaining shared
   personal SDK drawing types and following their grants and callback lifecycle.
5. Replace `export_player_mod!` with the scaffold's `export_client_part!` entry point and rebuild.

A personal-mod component is not a client-part component. Personal packet interception,
native extensions and local settings do not become server privileges. This build
command adds no drawing interfaces or server world/entity execution APIs.
