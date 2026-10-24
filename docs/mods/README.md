# Make a paired mod

[Cosmic](../../examples/mods/cosmic) is one Rust project with a server guest and a
client part. It registers Moon rock and Moon key, sends a ping to the server,
and draws a moon label when the reply arrives. Both guests ship in one `.cmod`.

Personal client mods use a different API: see [personal mods](personal.md).

## Install

Use a Cinnabar source checkout with its [build prerequisites](../../README.md#build-and-run).
Install Rust, Go and Make, then run these commands from the checkout root:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked --path tools/cxb
```

Keep the checkout: the tool uses its SDK, local server and client build tools.

## Create from Cosmic

Create a standalone project with the tool's SDK path, then copy the example:

```sh
cinnabar-cxb mod new cosmic
cp -R examples/mods/cosmic/src/. cosmic/src/
cp -R examples/mods/cosmic/assets/. cosmic/assets/
```

| File | Edit here |
| --- | --- |
| `src/shared.rs` | Block/item declarations and message fields |
| `src/server.rs` | Server callbacks and replies |
| `src/client.rs` | Client initialization and drawing |
| `assets/moon.png` | Original block texture and item icon |

For example, this declaration in `src/shared.rs` registers the cube:

```rust
#[block(name = "Moon rock", texture = assets::ASSET_MOON_PNG, hardness = 2.0)]
pub const MOON_ROCK: BlockId = id!("cosmic:moon_rock");
```

Declare requests with `#[message(..., to_server)]` and replies with
`#[message(..., to_client)]` in `src/shared.rs`. Add `#[on_message]` to a method
inside each entry impl; its last argument is the received message type.
Send requests with `client::send(&Ping { request: 1 })` and replies with
`ctx.send(&player, &Pong { request: ping.request })`. Cosmic's Pong declares
`control` so a reply waits for transport credit. Direction mistakes fail compilation.

## Build and run

```sh
cinnabar-cxb mod build --project cosmic
cinnabar-cxb mod run --project cosmic
```

Build writes `cosmic/dist/cosmic.cmod`. Run rebuilds it, prepares the client through
`make play-build`, starts an offline loopback server and joins a creative world.
The first run also prepares the client's game carriers through the normal build.
Accept the resource pack and client-code consent prompt. Look for Moon rock and
Moon key in creative inventory and `☾ Cosmic reply 1` below the running indicator.

Run keeps the world in `cosmic/.local/world` and client data in
`cosmic/.local/client`. Closing the client stops the server and its helpers.
Unset `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR` and `CARGO_BUILD_TARGET` before
running; the development client and helpers share the checkout's target layout.

## Edit and reload

Change the label in `src/client.rs` or the texture, close the client, and rerun:

```sh
cinnabar-cxb mod run --project cosmic
```

This rebuilds both guests and replaces this mod's development installation.
It is a restart loop; client parts have no live bundle update. Rebuild both halves
when shared definitions change. Keep existing block IDs when reusing a saved world.
For server-code-only reload restrictions, see [runtime reload](../experience-runtime.md#reload).

## Package

```sh
cinnabar-cxb mod build --project cosmic --release
cinnabar-cxb mod install cosmic/dist/cosmic.cmod --server /path/to/world
```

Give the `.cmod` to the server operator. Keep `cosmic/.local/publisher.seed` private
and retain it for later builds. The server offers players only the signed client
bundle and ordinary resource pack. Installation refuses an existing mod ID.

See [project reference](../mod-projects.md) for declarations, assets, generated Go
bindings and server setup; [client parts](../server-experiences.md) for permissions.
