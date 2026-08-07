# `valentine`

`valentine` is the Bedrock protocol surface for the workspace. It re-exports
the generated protocol crate behind a feature flag and keeps shared codec and
runtime support in `bedrock_core`.

## Current workspace version

The checked-in workspace exposes `bedrock_1_26_40` by default through:

- `valentine::bedrock::protocol::v1_26_40::*`
- `valentine::bedrock::version::v1_26_40::*`
- `valentine::bedrock::v1_26_40::*`

Prefer the version-pinned import:

```rust
use valentine::bedrock::version::v1_26_40::*;
```

The generated crate is under `bedrock_versions/v1_26_40/`; shared Bedrock
codec, context, and error types live under `bedrock_core/`.

Bedrock strings are decoded lossily for wire compatibility with existing
implementations.
