# Vendored upstream sources

The protocol crate vendors Valentine and Jolyne from
[`axolotl-stack/axolotl-stack`](https://github.com/axolotl-stack/axolotl-stack)
merge commit `6f6806e821a579c183c44d786f76d9b358a2b825` under the upstream MIT license.

Copied paths:

- `crates/valentine/src`
- `crates/valentine/bedrock_core`
- `crates/valentine/bedrock_versions/v1_26_0`
- `crates/valentine/bedrock_versions/v1_26_30`
- `crates/valentine/Cargo.toml` and `README.md`
- `crates/jolyne/src`
- `crates/jolyne/Cargo.toml` and `README.md`
- root `LICENSE`

Upstream examples, tests, benches, generator executable, unrelated workspace
crates, and uninitialised generator-input submodules are omitted. Local Cargo
manifests replace workspace inheritance with direct versions and local paths;
upstream-only development dependencies and targets are removed. Jolyne doctests
are disabled because its copied README demonstrates the omitted RakNet transport.
Jolyne defaults to no features and retains its feature names for cfg checking.

## Local source patch inventory

Task 0.4 made three Jolyne source changes: cfg guards around its RakNet-only
import, transport import, and client connection implementation.

Task 0.5 adds the following reviewed local patches:

- `jolyne/src/batch.rs` and `jolyne/src/stream/transport/inner.rs`: retain the
  negotiated Deflate or Snappy algorithm for outbound batches and implement
  Snappy encoding; enforce the 16 MiB decoded-batch and 1,600-packet limits on
  raw/borrowed ingress; and restore packets deferred by the login state machine
  ahead of unread packets from the same batch.
- `jolyne/src/stream/client.rs`: fail rather than silently skip decode/resource
  pack errors, negotiate Deflate, Snappy, or protocol no-compression, apply one
  120-second login deadline, dispatch by raw packet ID before decoding, stop on
  Disconnect, reject non-empty resource-pack stacks except gophertunnel's
  pinned client-built-in exemptions, update the shield runtime ID from
  ItemRegistry, request radius 16 immediately after
  StartGame, preserve unrelated pre-spawn packets for play under aggregate
  1,600-packet/16 MiB limits, accept either spawn/radius response order, reject
  conflicting StartGame runtime IDs, and acknowledge loading/initialisation
  with that exact ID.
- `jolyne/src/gamedata.rs`: document that optional biome/entity/creative
  definition packets stay queued for budgeted play-time decoding; only
  StartGame and ItemRegistry are eagerly materialised for the spawn gate.
- `jolyne/src/error.rs` and `jolyne/src/raw.rs`: preserve packet identity, body
  length, and a bounded 32-byte preview on decode errors. No full-packet dump or
  environment-controlled diagnostic hook is shipped. Owned and borrowed
  decoders reject bytes left inside a declared packet entry; deferred raw
  frames are compacted into frame-sized allocations and successful decode logs
  contain sizes/IDs rather than payload bytes.
- `jolyne/Cargo.toml`: enable Tokio macros/runtime only for vendored tests so
  the client-feature suite builds independently.
- `valentine/bedrock_core/src/bedrock/codec.rs`: add a fixed-width
  little-endian NBT scanner alongside the existing network-little-endian
  scanner and cap compound/list nesting at 512 in both variants.
- `valentine/bedrock_versions/v1_26_30/src/borrowed.rs`: retain the exact number
  of unconsumed payload bytes from generated borrowed decoders so Jolyne can
  enforce declared-entry boundaries without materialising owned packets, and
  bound borrowed ItemRegistry entries before allocation.
- `valentine/bedrock_versions/v1_26_30/src/proto.rs`: apply gophertunnel's
  4,096-element collection bound plus remaining-byte sanity before allocating
  every generated collection eagerly decoded by the Task 0.5 login path.
- `valentine/bedrock_versions/v1_26_30/src/types.rs`: correct PlayerList counts,
  bound PlayerList entries at 4,096 before allocation, reject contradictory
  PlayerList encodings, treat ItemLegacy IDs `0` and `-1` as empty, decode item
  extra-data NBT with fixed-width little endian, and encode/decode shaped recipe
  input as exactly `width * height` descriptors without length prefixes.

The generated Valentine changes are deliberate manual protocol-1001 patches
and would be overwritten by regeneration. In particular, the upstream
generator currently collapses fixed little-endian `lnbt` and network
little-endian `nbt`. The pinned conformance fixtures in `crates/protocol/tests`
must remain green across any regeneration.

The upstream commit records these generator-input gitlinks:

- PrismarineJS `minecraft-data` commit
  `6ec59288287e4045331eaa47ee8fb104278f6b98` (MIT)
- pmmp `BedrockData` commit
  `7d74ffbdd620dc1e31af0a645d3eea738c820c0b` (CC0-1.0)

Wire behaviour and byte fixtures use the exact project pin
`hashimthearab/gophertunnel` commit
`9948b1729395d2e819fce28e079d4a7bfc67716c`. It is the behavioural authority
for these patches; an unrelated local checkout or later `lunar` head is not.
