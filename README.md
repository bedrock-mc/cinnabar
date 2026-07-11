# rust-mcbe

`rust-mcbe` is a greenfield Minecraft Bedrock client. The Rust workspace owns the client,
world model, and rendering, while the Go core will own upstream networking and identity.

Phase 0 is pinned to Bedrock 1.26.30 (protocol 1001).

## Workspace

- `app/`: the `bedrock-client` application.
- `crates/bridge/`: the local client-to-core stream bridge.
- `crates/protocol/`: the pinned Bedrock packet codec.
- `crates/world/`: the client-side world model.
- `crates/render/`: Bevy rendering.
- `core/`: the `bedrock-core` Go service.

Local reference repositories and BDS installations under `.local/` are read-only development
inputs. Committed builds do not depend on them.

## Verification

```text
cargo fmt --check
cargo test --workspace
go test ./core/...
go vet ./core/...
```

Live BDS tests are enabled when `BEDROCK_BDS_DIR` is set and otherwise skip.

## Local vanilla block textures

The client never downloads or embeds Mojang assets. Fetch the pinned
`bedrock-samples` release after accepting its terms, then compile the ignored local runtime
blob:

```powershell
powershell -NoProfile -File scripts/fetch-vanilla-assets.ps1 -AcceptEula
cargo run -p assets --bin assetc -- compile --pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack --registry crates/assets/data/block-registry-v1001.bin --biome-registry crates/assets/data/biome-registry-v1001.bin --out .local/assets/compiled/vanilla-v1001.mcbea
```

Start the client with an explicit blob when needed:

```text
cargo run -p bedrock-client --locked -- --socket-dir .local/run --assets .local/assets/compiled/vanilla-v1001.mcbea
```

Asset selection uses `--assets`, then `RUST_MCBE_ASSETS`, then the ignored default
`.local/assets/compiled/vanilla-v1001.mcbea`. A missing file starts with the generated
magenta/black diagnostic texture and prints the commands above. A present but malformed blob
is rejected with its exact path.
