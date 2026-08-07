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
  `781dfcb0ab443476b62df3c983750c0c1527a95a`; see "Bedrock 1.26.40
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
`56a0f77dbbb2fb006b081ec38bb4bedf9cb95088` (`cinnabar`, module pseudo-version
`v1.25.3-0.20260807205305-56a0f77dbbb2`, Minecraft `1.26.40` / protocol 2168).
It is the behavioural authority for these patches; an unrelated local checkout
or later branch head is not. The protocol-1001 fixtures were generated against
commit `9948b1729395d2e819fce28e079d4a7bfc67716c`, which is what the still
unmigrated `v1_26_30` patches above were reviewed against.

## Bedrock 1.26.40 readiness

Cinnabar runs on Bedrock 1.26.40 / protocol 2168. `crates/protocol/Cargo.toml`
selects `bedrock_1_26_40` for the wire format and keeps `bedrock_1_26_30`
enabled alongside it purely as a content-registry data source; the two version
modules are additive and compile side by side.

Three things that used to be local patches are now upstream and survive
regeneration:

- `decode_inner_with_remaining` for borrowed packets (`axolotl-stack` `d075eb2`),
  replacing the hand-patched `v1_26_30/src/borrowed.rs` behaviour.
- Length-prefixed binary buffers are typed `Vec<u8>` instead of a lossily
  decoded `String` (`781dfcb`). Chunk, sub-chunk, blob-cache and item user-data
  payloads survive a decode/encode round trip; the framing is unchanged.
- `GameRuleRuleValue`'s `Int32` and `Float` arms encode through `I32LE`/`F32LE`
  rather than the big-endian bare primitive codecs (`781dfcb`). The root cause
  was union-arm lowering in the generator, so `DataStoreUpdate` Double and
  `ServerboundPackSettingChange` Float were corrected with them.

### Open: no allocation guards

**This remains deliberate debt.** `v1_26_40` contains none of the
`MAX_LOGIN_COLLECTION_ELEMENTS` / `MAX_WORLD_COLLECTION_ELEMENTS` /
`MAX_SUB_CHUNK_ENTRIES` / `MAX_PACKET_BYTE_ARRAY_BYTES` / `MAX_PLAYER_RECORDS`
bounds that `v1_26_30` applied before every eager collection allocation:
`grep -c ArrayLengthExceeded` over `bedrock_versions/v1_26_40/src/` returns 0
where `v1_26_30` had 44 sites. Every length-prefixed field decodes as a bare
`Vec::with_capacity(len)` over an attacker-controlled varint, so a peer can
force a large reservation before the read fails. These are hostile-input
protections no schema source emits, and they belong in the generator rather
than in another hand-patch. `tests/world_collection_bounds.rs` and
`tests/biome_definition_list.rs` are two-sided tripwires: they assert the read
fails with EOF today and fail loudly if `ArrayLengthExceeded` reappears, which
is the signal to restore the strict assertions.

Cinnabar's own pre-decode gates in `crates/protocol/src/{codec,inventory}.rs`
still bound counts and text before the owned decoder allocates, so the exposed
surface is the packets those gates do not cover.

### Resolved: BedrockSafetyRedactableString uses two adjacent strings

`ItemStackResponseSlotInfo::custom_name` and `StructureEditorData::structure_name`
are typed `BedrockSafetyRedactableString`. The generated correction keeps the
optional `redacted` value in memory but encodes and decodes the `unredacted` and
`redacted` halves as two unconditional adjacent VarInt-length-prefixed strings,
matching gophertunnel's `StackResponseSlotInfo.Marshal` and structure-editor
path (`packet/structure_block_update.go`). An absent or empty redacted value
uses the required zero-length second string and decodes as `None`; there is no
presence byte.

The existing response-name pre-decode scanner now walks and bounds both strings.
`fixtures/item_stack_response.bin` decodes, normalizes through the public world
event path, and re-encodes byte-for-byte. Direct owned and borrowed
`StructureEditorData` coverage pins the same shape and empty boundary; the
owned decoder also pins the truncated-second-string boundary.

### Open: borrowed LevelChunk payloads are owned

`LevelChunkPacketView::serialized_chunk_data` is an owned `Vec<u8>` rather than
a zero-copy `BorrowedStr`, an allocation regression on the hottest packet.
Acknowledged upstream as a generator follow-up (it could use
`take_varint_prefixed_bytes` for `Array{VarInt,U8}`). Relatedly, there is no
`TextPacketBodyView`, so the borrowed text path materialises the body strings
before `validate_borrowed_ui_packet` runs; `codec::validate_raw_text_packet` is
what actually enforces UTF-8 before allocation.

### Resolved: strict MovePlayer actor ID varints

The owned and borrowed `MovePlayer` runtime and ridden ID call sites now decode
through the reusable strict unsigned-64 wire helper. Canonical values through
`u64::MAX` are accepted and retain their exact bytes; overflowing tenth-byte
payloads and non-canonical ten-byte overlong encodings are rejected. Unrelated
generated `VarLong` fields are unchanged.

`v1_26_40` has no `blocks`, `items`, `states`, `entities`, `biomes`, or
`block_palette` modules: the BDS dumps describe the wire, not the content
registries. `crates/protocol/src/world.rs` reads `v1_26_30::biomes::ALL_BIOMES`
and `crates/protocol/src/item.rs` reads the generated `v1_26_30` items table, so
that data keeps coming from the prismarine-derived crate. Both sites are
commented as deliberate data pins.
