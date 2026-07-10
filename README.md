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
