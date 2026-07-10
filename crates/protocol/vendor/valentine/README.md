# `valentine`

`valentine` is the Bedrock protocol surface for the workspace.

It re-exports generated version crates behind feature flags and keeps the shared Bedrock codec/runtime in `bedrock_core`.

## Current Workspace Version

The checked-in workspace currently exposes:

- `bedrock_1_26_30`
- `valentine::bedrock::protocol::v1_26_30::*`
- `valentine::bedrock::version::v1_26_30::*`
- `valentine::bedrock::v1_26_30::*` (compatibility alias)

`bedrock_1_26_30` is also the default feature in [Cargo.toml](/C:/Users/jvigu/OneDrive/Documents/rust/jyuggers/axolotl-stack/crates/valentine/Cargo.toml).

## Layout

- `src/bedrock/`: shared Bedrock-facing API, version aliases, codec/context/error re-exports
- `bedrock_core/`: shared codec/runtime primitives used by every generated version crate
- `bedrock_versions/v1_26_30/`: generated protocol/data crate for the checked-in version

## Import Paths

Prefer:

```rust
use valentine::bedrock::version::v1_26_30::*;
```

Compatibility aliases still exist:

```rust
use valentine::bedrock::v1_26_30::*;
use valentine::bedrock::protocol::v1_26_30::*;
```

Use `protocol::vX_Y_Z` when you explicitly want the raw generated version crate/module layout.

## Regenerating Protocol Code

From the repo root:

```bash
git submodule update --init --recursive
cargo run -p valentine_gen -- --latest
```

Generate multiple versions when you want cross-version type/packet dedup to be considered in a single run:

```bash
cargo run -p valentine_gen -- --versions 1.21.120,1.21.124,1.26.30
```

## Notes

- Bedrock strings are decoded lossily on purpose for wire compatibility with existing implementations such as `gophertunnel`.
- Prefer importing from `bedrock::version::vX_Y_Z` in application code unless you specifically need the raw generated protocol crate or a compatibility alias.
