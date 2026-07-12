# Cinnabar

Cinnabar is a greenfield Minecraft Bedrock client. The Rust workspace owns the client,
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

## Architecture

The client is deliberately split at a local, packet-aware transport boundary:

```text
Cinnabar desktop client (Rust)
  Axolotl/Valentine protocol definitions + palette-native world + Bevy/WGPU renderer
                              |
                 local stream socket (`game.sock` on Unix;
                    loopback publication behind the same
                         streamnet interface on Windows)
                              |
bedrock-core (Go)
  gophertunnel session/auth/packet relay
       |-- go-raknet -------- third-party servers and BDS
       `-- go-nethernet ----- Realms and friend worlds
             `-- go-xsapi -- Xbox discovery, identity, and signaling
```

Rust never implements Microsoft/Xbox authentication, upstream encryption, RakNet, or
NetherNet. The Go core owns those moving network surfaces and translates them into the stable
local stream consumed by the client. This keeps the renderer and game simulation in Rust while
reusing the proven gophertunnel networking/authentication stack.

## Verification

```text
cargo fmt --check
cargo test --workspace
go test ./core/...
go vet ./core/...
```

Live BDS tests are enabled when `BEDROCK_BDS_DIR` is set and otherwise skip.

## Local protocol-1001 block data

The generated block catalog uses pinned, non-Mojang metadata from PMMP BedrockData and
PrismarineJS minecraft-data. Acquire and hash-check those inputs, plus the applicable
upstream license evidence, into the ignored local cache:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/acquire-block-data.ps1
```

The command validates Bedrock `1.26.30` / protocol `1001` and atomically publishes the
resolved inputs below `.local/assets/block-data/pmmp` and
`.local/assets/block-data/prismarine`. Axolotl and Dragonfly license evidence is retained
below `.local/assets/block-data/licenses`; the bounded verified download cache stays in the
sibling `.local/assets/block-data.downloads/` directory. The application never fetches these inputs at
startup. Pins, file hashes, source repositories, and the Prismarine license-evidence
exception are recorded in `assets/block-data-sources.json` and `THIRD_PARTY_NOTICES.md`.

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
