# Pinned upstream sources and retained asset snapshot

The protocol crate resolves Valentine and Jolyne from the checked-in
`vendor/valentine` and `vendor/jolyne` paths. This local source snapshot carries
the reviewed Cinnabar patches published by
[`HashimTheArab/axolotl-stack`](https://github.com/HashimTheArab/axolotl-stack)
on branch `cinnabar/protocol-1001-fixes`, and the same files remain the pinned
input used by `tools/registrygen` and its reproducible asset-generation
documentation.

Machine-checked provenance:

- Dependency resolution: local vendored paths
- Reviewed fork revision: `6cd8087fc3f0b500e41708a8afc94a0fa3291525`
- Upstream snapshot revision: `6f6806e821a579c183c44d786f76d9b358a2b825`
- Retained license: MIT at `crates/protocol/vendor/LICENSE` (normalized SHA-256 `62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184`)

The snapshot originated from
[`axolotl-stack/axolotl-stack`](https://github.com/axolotl-stack/axolotl-stack)
merge commit `6f6806e821a579c183c44d786f76d9b358a2b825` under the retained upstream MIT license.

Copied paths:

- `crates/valentine/src`
- `crates/valentine/bedrock_core`
- `crates/valentine/bedrock_versions/v1_26_0`
- `crates/valentine/bedrock_versions/v1_26_30`
- `crates/valentine/bedrock_versions/v1_26_40` (from `axolotl-stack` main
  `d075eb24063b2d3dadcbd8dbc3d0eea26e09b048`; see "Bedrock 1.26.40
  readiness" below)
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
  scanner and cap compound/list nesting at 512 in both variants. Also encode
  `Uuid` as two little-endian `u64` halves rather than the raw 16 bytes, which
  is what gophertunnel's `Writer.UUID` produces (it concatenates `x[8:]` and
  `x[:8]` and reverses all 16 bytes, i.e. each half byte-reversed in place).
  **This one is still not upstreamed** — `axolotl-stack` main writes
  `uuid.as_bytes()` verbatim, so every UUID field there is byte-swapped
  relative to BDS. It affects `v1_26_40` too, since `bedrock_core` is shared.
  `uuid_uses_bedrock_little_endian_halves` pins the expected bytes.
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

Task 0.8 adds two more reviewed protocol-1001 patches:

- `valentine/bedrock_versions/v1_26_30/src/proto.rs`: make
  `AvailableCommands.EnumValues` use the pinned gophertunnel single shared
  VarUInt count, reject more than 4,096 entries, and reserve fallibly.
- `valentine/bedrock_versions/v1_26_30/src/types.rs` and `borrowed.rs`: model
  every MaterialReducer output as a counted, bounded vector of ZigZag pairs in
  both owned and borrowed forms.

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
`be6713da4dc051a4197f897d04835e89e9c54321` (`lunar`, module pseudo-version
`v1.25.3-0.20260806044231-be6713da4dc0`, Minecraft `1.26.40` / protocol 2168).
It is the behavioural authority for these patches; an unrelated local checkout
or later `lunar` head is not. The protocol-1001 fixtures were generated against
commit `9948b1729395d2e819fce28e079d4a7bfc67716c`, which is what the still
unmigrated `v1_26_30` patches above were reviewed against.

## Bedrock 1.26.40 readiness

`crates/protocol/Cargo.toml` now selects `bedrock_1_26_40` for the wire format
and keeps `bedrock_1_26_30` enabled alongside it purely as a content-registry
data source. The two version modules are additive and compile side by side.
Jolyne's client path is fully migrated and its suite passes. **The protocol
crate itself is not migrated and this branch is not mergeable**: two generated
wire defects have to be fixed upstream first, because both change generated
field types and would invalidate any cinnabar code written against the current
shapes.

`decode_inner_with_remaining` is no longer a local patch. `axolotl-stack`
`d075eb2` generates it for borrowed packets, so the retained-remaining-bytes
behaviour that `v1_26_30/src/borrowed.rs` hand-patched is now generator-emitted
and survives regeneration.

### Blocking: length-prefixed binary buffers are typed `String`

The Endstone dumps declare these fields `string` because BDS uses C++
`std::string`, which is a byte string with no encoding guarantee. The generator
maps that to a Rust `String` and decodes it with `decode_utf8_lossy_owned`, so
every byte sequence that is not valid UTF-8 becomes U+FFFD. Decode is not
byte-preserving and re-encode changes the length:

    sent  00 80 9f ff fe 41 c3 28
    recv  00 efbfbd efbfbd efbfbd efbfbd 41 efbfbd 28

The framing is right and only the content is mangled, so nothing fails loudly;
encode and decode corrupt symmetrically. Affected fields carrying binary:

- `LevelChunkPacket.serialized_chunk_data` (chunk blob)
- `SubChunkPacketPayloadSubChunkPacketData.serialized_sub_chunk`
- `MissingBlobData.blob_data` (client cache miss response)
- `user_data_buffer` on both item stack descriptors (item NBT)
- `ResourcePackChunkDataPacket.chunk_data`

gophertunnel types the same LevelChunk field `RawPayload []byte` and writes it
with `io.ByteSlice` (`minecraft/protocol/packet/level_chunk.go`), which is the
identical varint-prefixed framing. `v1_26_30` had this right as
`payload: ByteArray`. This is the same defect `axolotl-stack` `d11ca37` fixed
for `LoginPacket.connection_request` through the `overrides-endstone`
correction layer; the rest of the class is still open. Chunk and sub-chunk
payloads are cinnabar's primary world data, so this blocks rendering outright.

### Blocking: GameRule integer values are written big-endian

`GameRuleRuleValue::Int32` encodes through the bare `i32` `BedrockCodec`, whose
`encode` is `buf.put_i32` — big-endian. gophertunnel writes the same value with
`w.Uint32` (`minecraft/protocol/writer.go`), which is little-endian in both the
big- and little-endian writer variants; its only deliberately big-endian helper
is `BEInt32`, used for fields such as PlayStatus. The values are therefore
byte-swapped relative to BDS wherever a game rule carries an int, which covers
`StartGamePacket`'s rule list and `GameRulesChangedPacket`, and so reaches the
`start_game.bin` fixture.

The bare `i32` codec being big-endian is not wrong by itself — PlayStatus
genuinely is big-endian — but it is the wrong codec for this field.

### Still open from before

- **The generated crate carries no allocation guards.** `v1_26_40` contains
  none of the `MAX_LOGIN_COLLECTION_ELEMENTS` / `MAX_WORLD_COLLECTION_ELEMENTS`
  / `MAX_SUB_CHUNK_ENTRIES` / `MAX_PACKET_BYTE_ARRAY_BYTES` / `MAX_PLAYER_RECORDS`
  bounds that `v1_26_30` applies before every eager collection allocation, nor
  the `ItemNew` air/empty-item handling. These are hostile-input protections no
  schema source emits; regenerating drops them. They belong in the generator,
  not in another hand-patch. **This caveat remains deliberate debt.**
- **Type names and shapes differ.** Only 207 of 528 generated structs keep
  their prismarine-era names, and many are reshaped rather than renamed: the
  three item wire formats collapse into one descriptor with an opaque user-data
  buffer, `TextPacket` becomes a three-arm body union, `SetScorePacket` moves
  its action to a per-entry union, entity metadata loses its named key enum and
  its bitflag types, `SubChunk` caching/non-caching entries merge behind an
  `Option<u64>` blob id, `GameMode` and `LevelEventPacketEvent` lose their named
  enums entirely, and `PlayerAuthInput` input flags become a list of set flag
  ids instead of a bitset.
- **The Go core has landed.** `core/go.mod` and `tools/fixturegen/go.mod` use
  `hashimthearab/gophertunnel v1.25.3-0.20260806044231-be6713da4dc0`, and the
  byte fixtures under `crates/protocol/fixtures` carry 1.26.40 bytes.

`v1_26_40` also has no `blocks`, `items`, `states`, `entities`, `biomes`, or
`block_palette` modules: the BDS dumps describe the wire, not the content
registries. `crates/protocol/src/world.rs` reads `v1_26_30::biomes::ALL_BIOMES`
and `crates/protocol/src/item.rs` reads the generated `v1_26_30` items table,
so that data keeps coming from the prismarine-derived crate. Both sites are
commented as deliberate data pins.
