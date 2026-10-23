# Two-world Dragonfly client-part example

This example uses upstream Dragonfly v0.11.5 and Gophertunnel v1.62.0. Its only
module replacement points at the helper in this checkout. No server-side WASM
runtime is involved. Both temporary worlds use the overworld dimension type.
There are no bundled game assets.

From the repository root, build the client component and bundler:

```sh
cargo build --locked -p dragonfly-extension-client --target wasm32-unknown-unknown
cargo build --locked -p cinnabar-cxb
mkdir -p /tmp/worlds-example/bundles
target/debug/cinnabar-cxb keygen /tmp/worlds-example/server.seed
target/debug/cinnabar-cxb build --experience examples/dragonfly-extension/experience.toml --component target/wasm32-unknown-unknown/debug/dragonfly_extension_client.wasm --publisher-seed /tmp/worlds-example/server.seed --out /tmp/worlds-example/bundles/worlds.cxb
target/debug/cinnabar-cxb seed-cache --cxb /tmp/worlds-example/bundles/worlds.cxb --user-data <your-client-user-data>
GOWORK=off CINNABAR_CXB_BIN="$PWD/target/debug/cinnabar-cxb" go -C examples/dragonfly-extension run . -key /tmp/worlds-example/server.seed -bundles /tmp/worlds-example/bundles -data /tmp/worlds-example/data
```

Keep the server seed and generated output outside the checkout. The sample signs
with one development key for convenience; deployments can separate server and
publisher keys. Use the existing developer execution switch for client parts;
production execution remains subject to the client's existing policy.

Join `127.0.0.1:19132`, accept the optional offer, and use `/travel`. Each command
moves the player into the other world and calls `WorldChanged` after `AddEntity`
and `Teleport`. The helper waits for the new epoch, then sends a typed world view.
The Rust client acknowledges that choice on the declared return channel. The
server logs `client acknowledged world`. Use `/travel` again for the return trip.
The sample adds no custom visual surface; the changing world and acknowledgment
log are its observable behavior.

`experience.toml` owns the channel identities, revisions and bounds. The Rust
build generates its channel constants from that manifest. The Go sample reads
and checks the same declarations from the validated bundle, then implements the
helper's small message interface. Generated application message bindings can
replace these sample codecs without changing the transport.

The helper retains cache-only bundle delivery, so seed the exact bundle into the
client cache first. Authentication stays enabled. World state is temporary and
is discarded when the server exits.
