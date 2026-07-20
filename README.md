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

## Authenticated remote RakNet servers

Run both commands from the repository root. For a generic authenticated Bedrock server,
start the Go core in one terminal (replace the example address with the server's host and
port):

```text
go run ./core/cmd/bedrock-core -socket-dir .local/run-remote -upstream play.example.net:19132 -auth-cache .local/auth/microsoft-token.json
```

Then start the current release client against the same local socket directory in another
terminal:

```text
cargo run --release -p bedrock-client --locked -- --socket-dir .local/run-remote
```

The equivalent Zeqa smoke commands are:

```text
go run ./core/cmd/bedrock-core -socket-dir .local/run-zeqa -upstream zeqa.net:19132 -auth-cache .local/auth/microsoft-token.json
cargo run --release -p bedrock-client --locked -- --socket-dir .local/run-zeqa
```

Some Bedrock networks route an authenticated entry connection to a regional server before
login finishes. The core follows these pre-login `Transfer` packets itself (with cycle and
hop limits) before it sends downstream `StartGame` and spawns the local Rust player; Zeqa
uses this path for its lobby.

On first use, `bedrock-core` prints the Microsoft device-login URL and code to stdout. Approve
that code in a browser; the core then writes the resulting token cache to
`.local/auth/microsoft-token.json`. A usable cache is refreshed and reused on later runs, so a
new prompt is not expected every time.

The cache contains private Microsoft authentication credentials. Keep it under the ignored
`.local/` tree, never commit or share it, and do not paste its contents into logs, issues, or
smoke-test evidence. Deleting the cache signs this local workflow out and requires a new device
login on the next authenticated run.

The authenticated join path is:

```text
bedrock-client (Rust: Axolotl/Valentine packets, world state, Bevy renderer)
  -> local streamnet socket (Unix socket; loopback transport on Windows)
  -> bedrock-core (Go: gophertunnel authentication, session, and packet relay)
  -> go-raknet
  -> remote Bedrock server (for example, Zeqa)
```

Realms and friend-world joins remain Go-owned as well, using the gophertunnel,
go-nethernet, and go-xsapi stack; they are separate from this direct RakNet command path.

## Verification

```text
cargo fmt --check
cargo test --workspace
go test ./core/...
go vet ./core/...
```

For a faster local loop, verify only packages affected by changes since an explicit Git base plus
their reverse workspace dependencies:

```text
cargo run -p devtool --locked -- verify-affected --base origin/phase2-textures
```

Add `--dry-run` to inspect the selected packages and exact commands without executing them. Changes
to workspace-global build inputs automatically select the full workspace gate. The verifier uses
`cargo-nextest` when available and retains doctests through a separate Cargo invocation; otherwise
it falls back to `cargo test`. Install the measured version with:

```text
cargo install cargo-nextest --version 0.9.140 --locked
```

Live BDS tests are enabled when `BEDROCK_BDS_DIR` is set and otherwise skip.

## Linux window backends

Linux builds include both native Wayland and X11 support. Winit prefers Wayland when
`WAYLAND_DISPLAY` or `WAYLAND_SOCKET` identifies an active compositor connection; otherwise,
it uses X11 when `DISPLAY` is available. Cinnabar does not force either backend, so native
GNOME Wayland sessions use Wayland automatically while X11 sessions and XWayland remain
supported.

Building the Wayland backend on Debian or Ubuntu requires the development library:

```text
sudo apt-get install libwayland-dev
```

To explicitly exercise the retained X11 path from a Wayland session, clear the Wayland
connection variables for that invocation:

```text
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET cargo run --release -p bedrock-client --locked -- --socket-dir .local/run
```

## Local protocol-1001 block data

The generated block catalog uses pinned, non-Mojang metadata from PMMP BedrockData and
PrismarineJS minecraft-data. A normal `make client` automatically acquires, hash-checks,
and compiles the required protocol-1001 physics registry. To prepare it without launching
the client, run:

```text
make physics-assets
```

The command validates Bedrock `1.26.30` / protocol `1001` and atomically publishes the
resolved inputs below `.local/assets/block-data/pmmp` and
`.local/assets/block-data/prismarine`. Axolotl and Dragonfly license evidence is retained
below `.local/assets/block-data/licenses`; the bounded verified download cache stays in the
sibling `.local/assets/block-data.downloads/` directory. The application itself never
fetches these inputs at runtime; the Make prerequisite does. Pins, file hashes, source
repositories, and the Prismarine license-evidence
exception are recorded in `assets/block-data-sources.json` and `THIRD_PARTY_NOTICES.md`.

The cross-platform Go acquirer emits the exact PMMP and Prismarine roots consumed by the
physics compiler. The equivalent low-level generation command is:

```powershell
go -C tools/registrygen run . `
  -out ../../.local/phase3/block-registry-v1001.bin `
  -light-out ../../.local/phase3/block-light-registry-v1001.bin `
  -light-breg ../../crates/assets/data/block-registry-v1001.bin `
  -physics-out ../../.local/assets/block-physics-v1001.bin `
  -physics-sha-out ../../.local/phase3/block-physics-v1001.sha256 `
  -physics-breg ../../crates/assets/data/block-registry-v1001.bin `
  -pmmp ../../.local/assets/block-data/pmmp `
  -prismarine ../../.local/assets/block-data/prismarine `
  -valentine-palette ../../crates/protocol/vendor/valentine/bedrock_versions/v1_26_30/src/block_palette.bin `
  -valentine-blocks ../../crates/protocol/vendor/valentine/bedrock_versions/v1_26_30/src/blocks.rs
```

`PREG1001` contains one explicit bounded physics record for each of the 16,913
protocol-1001 BREG states. Its header binds the exact BREG SHA-256 and the Rust
decoder rejects any identity, count, enum, scalar, topology, or trailing-byte
mismatch before publishing a registry.

Generation additionally requires the exact acquired PMMP physics table
(`c9eb2a1b7751ba874ddeb04237d2a0013121a1bf03e1d5c75a78a08bae020abd`)
and Prismarine behavior/state/collision files
(`12ff90b5094006b42d87ca7c296ed1bef0e1c2d6d67498aea85b6ece9408b494`,
`c0a94f5a32597aff028918e152c76280c1823a7840fdf73cd98d7b44814ea041`,
`72a7410456a1f5f556e8c91c07e1d1f61aea5d2fb555f2c0e33eba825247aa90`),
plus the exact Dragonfly module version ending in `dbbd8b787946` and module
content sum `h1:Qu7Qm7iBrLQWlZtz2KdouA4agQdhybV2abSdEN5NBRY=`. Replaced modules
are rejected. A sorted reviewed override
table is validated against Prismarine bounding-box/state counts and Dragonfly's
exact registered implementation-type set before any special movement fact is
encoded. Valid JSON with changed keys or values is rejected by source hash, and
malformed or ambiguous bubble direction fails closed.

## UI font assets

`make client` automatically downloads the pinned OFL-1.1 Inter source and license,
verifies their exact sizes and SHA-256 hashes, rasterizes a deterministic 32-pixel
UI atlas, and builds the ignored `ui-inter-v1.mcbefont` carrier. The source cache
stays below `.local/assets/ui-font/`; neither the outline font nor generated carrier
is committed. Run the font step alone with:

```text
make font-assets
```

An owned, reviewed local Bedrock bitmap-font pack can take precedence over the
generated Inter carrier without downloading or redistributing Mojangles:

```text
make font-assets-local FONT_PACK_DIR=/path/to/reviewed/resource_pack
```

The local pack must contain the bounded `font/catalog.json` descriptor and referenced
PNG pages expected by the compiler. This command writes the distinct ignored
`vanilla-v1.mcbefont` carrier; startup prefers that validated local carrier while
leaving `ui-inter-v1.mcbefont` intact as the fallback. Normal builds never fetch
Mojangles or another unlicensed Minecraft font mirror.

## Vanilla HUD sprites

The survival HUD uses exact sprites from Mojang's pinned official
`bedrock-samples-v1.26.30.32-preview-full.zip` release. A normal `make assets`
or `make client` downloads and verifies that archive through the same EULA-gated
vanilla asset acquisition step used by world assets, then automatically writes
the ignored `.local/assets/compiled/vanilla-v1.mcbehud` carrier and
`hud-assets.json` provenance report. Run only that step with:

```text
make hud-assets
```

The tracked non-copyright
[`assets/hud-source-v1001.json`](assets/hud-source-v1001.json) pins the official
release tag, commit, archive URL and hash, plus every required PNG byte count,
SHA-256, and decoded dimension. Wrong-version, custom, missing, or stale inputs
fail closed at compilation, and carriers with any other source identity fail
closed at startup. The PNGs, downloaded archive, and generated carrier remain
ignored local data; no Mojang image is embedded in this repository and no
third-party asset mirror is used.

All sample-pack consumers share the `make vanilla-assets` acquisition sentinel.
If the extracted cache is cleaned while compiled carriers remain, the next HUD,
world, atmosphere, entity, assets, or client build reacquires the official pack;
an intact cache does not rerun the fetch step.

An explicitly selected pack can be checked against the same immutable manifest:

```text
make hud-assets-local HUD_PACK_DIR=/path/to/matching/resource_pack
```

The source root must contain these exact PNG paths:

```text
textures/ui/heart_background.png
textures/ui/heart.png
textures/ui/heart_half.png
textures/ui/hunger_background.png
textures/ui/hunger_full.png
textures/ui/hunger_half.png
textures/ui/armor_empty.png
textures/ui/armor_full.png
textures/ui/armor_half.png
textures/ui/bubble.png
textures/ui/bubble_empty.png
textures/ui/hotbar_0.png through textures/ui/hotbar_8.png
textures/ui/selected_hotbar_slot.png
textures/ui/empty_progress_bar.png
textures/ui/filled_progress_bar.png
```

The pin also verifies the same official sample release's `manifest.json`,
`ui/scoreboards.json`, `ui/hud_screen.json`, `ui/ui_common.json`, and
`ui/_global_variables.json`, which retain the UI authority used by the HUD
layout implementation.

The base Mojangles bitmap font is not present in this sample pack, so this does
not change the open-licensed Inter default or the optional local bitmap-font
override described above. The HUD carrier is required at production startup:
if it is absent, the error names the exact path and the client exits instead of
silently hiding authoritative survival stats or substituting guessed art.
`make client` and `make assets` build it automatically from the pinned official
sample pack; use `make hud-assets` to repair only that carrier.

The pinned scoreboard definition supplies content-driven width, right-middle
placement, text colors, row geometry, and title geometry. Its two background
alphas are engine bindings (`#objective_background_opacity` and
`#scoreboard_objective_background_opacity`), not constants in the pack. The
sidebar therefore remains fail-closed until both values are supplied by native
runtime evidence; the adjacent HUD text-opacity option is not substituted.

## Local vanilla block textures

The client never downloads or embeds Mojang assets. Fetch the pinned
`bedrock-samples` release after accepting its terms, then compile the ignored local runtime
blob:

```powershell
powershell -NoProfile -File scripts/fetch-vanilla-assets.ps1 -AcceptEula
cargo run -p asset-compiler --bin assetc -- compile --pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack --registry crates/assets/data/block-registry-v1001.bin --light-registry crates/assets/data/block-light-registry-v1001.bin --biome-registry crates/assets/data/biome-registry-v1001.bin --out .local/assets/compiled/vanilla-v1001.mcbea
```

Start the client with an explicit blob when needed:

```text
cargo run -p bedrock-client --locked -- --socket-dir .local/run --assets .local/assets/compiled/vanilla-v1001.mcbea
```

To disable VSync through the Makefile client target, run `make client NO_VSYNC=1`.

Asset selection uses `--assets`, then `RUST_MCBE_ASSETS`, then the ignored default
`.local/assets/compiled/vanilla-v1001.mcbea`. A missing file starts with the generated
magenta/black diagnostic texture and prints the commands above. A present but malformed blob
is rejected with its exact path. `make client` treats that default blob as a real build target:
it fetches and compiles when the blob is missing or older than the tracked asset compiler,
registry, or lockfile inputs, while an unchanged blob launches without recompiling.
